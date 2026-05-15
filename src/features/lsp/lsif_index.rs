use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;

use serde_json::{Value, json};

use crate::features::lsp::runtime_bridge::{
    LspRuntimeBridgeRequest, LspRuntimeBridgeResponse, LspRuntimeBridgeSource,
};
use crate::runtime::live::RuntimeCommandError;

#[derive(Debug, Default)]
pub struct LsifIndexCache {
    indexes: HashMap<PathBuf, CachedLsifIndex>,
}

impl LsifIndexCache {
    pub fn execute_request(
        &mut self,
        request: LspRuntimeBridgeRequest,
        diagnostic_events: &Arc<Mutex<Vec<String>>>,
    ) -> Result<LspRuntimeBridgeResponse, RuntimeCommandError> {
        request.validate()?;
        let dump_path = resolve_dump_path(&request)?;
        let index = self.index_for_path(&dump_path, diagnostic_events)?;
        let result = match request.method.as_str() {
            "textDocument/hover" => index.lookup_hover(&request)?,
            "textDocument/definition" => Value::Array(index.lookup_definition(&request)?),
            method => {
                return Err(lsif_failed(format!("unsupported LSIF method: {method}")));
            }
        };
        emit_lsif_event(
            diagnostic_events,
            format!(
                "lsif lookup complete: method={}, dump={}",
                request.method,
                dump_path.display()
            ),
        );
        Ok(LspRuntimeBridgeResponse {
            source: LspRuntimeBridgeSource::Lsif,
            method: request.method,
            result,
        })
    }

    fn index_for_path(
        &mut self,
        path: &Path,
        diagnostic_events: &Arc<Mutex<Vec<String>>>,
    ) -> Result<&LsifIndex, RuntimeCommandError> {
        let signature = LsifDumpSignature::from_path(path)?;
        let reload = self
            .indexes
            .get(path)
            .map(|cached| cached.signature != signature)
            .unwrap_or(true);
        if reload {
            if self.indexes.contains_key(path) {
                emit_lsif_event(
                    diagnostic_events,
                    format!("lsif index cache invalidated: dump={}", path.display()),
                );
            }
            let index = LsifIndex::load_path(path, diagnostic_events)?;
            self.indexes
                .insert(path.to_path_buf(), CachedLsifIndex { signature, index });
        } else {
            emit_lsif_event(
                diagnostic_events,
                format!("lsif index cache hit: dump={}", path.display()),
            );
        }
        self.indexes
            .get(path)
            .map(|cached| &cached.index)
            .ok_or_else(|| lsif_failed("LSIF index cache entry is missing after load"))
    }
}

#[derive(Debug)]
struct CachedLsifIndex {
    signature: LsifDumpSignature,
    index: LsifIndex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LsifDumpSignature {
    len: u64,
    modified_nanos: Option<u128>,
}

impl LsifDumpSignature {
    fn from_path(path: &Path) -> Result<Self, RuntimeCommandError> {
        let metadata = std::fs::metadata(path).map_err(|error| {
            lsif_failed(format!(
                "failed to stat LSIF dump {}: {error}",
                path.display()
            ))
        })?;
        let modified_nanos = metadata
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos());
        Ok(Self {
            len: metadata.len(),
            modified_nanos,
        })
    }
}

#[derive(Debug, Default)]
struct LsifIndex {
    documents: HashMap<LsifId, LsifDocument>,
    ranges: HashMap<LsifId, LsifRange>,
    hover_results: HashMap<LsifId, Value>,
    document_ranges: HashMap<LsifId, Vec<LsifId>>,
    range_documents: HashMap<LsifId, LsifId>,
    range_result_sets: HashMap<LsifId, LsifId>,
    hover_edges: HashMap<LsifId, LsifId>,
    definition_edges: HashMap<LsifId, LsifId>,
    definition_items: HashMap<LsifId, Vec<LsifDefinitionItem>>,
}

impl LsifIndex {
    fn load_path(
        path: &Path,
        diagnostic_events: &Arc<Mutex<Vec<String>>>,
    ) -> Result<Self, RuntimeCommandError> {
        emit_lsif_event(
            diagnostic_events,
            format!(
                "lsif index loading: strategy=line-scan, dump={}",
                path.display()
            ),
        );
        let file = File::open(path).map_err(|error| {
            lsif_failed(format!(
                "failed to open LSIF dump {}: {error}",
                path.display()
            ))
        })?;
        let mut index = Self::default();
        for (line_number, line) in BufReader::new(file).lines().enumerate() {
            let line = line.map_err(|error| {
                lsif_failed(format!(
                    "failed to read LSIF dump {} line {}: {error}",
                    path.display(),
                    line_number + 1
                ))
            })?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let entry = serde_json::from_str::<Value>(trimmed).map_err(|error| {
                lsif_failed(format!(
                    "failed to parse LSIF dump {} line {}: {error}",
                    path.display(),
                    line_number + 1
                ))
            })?;
            index.accept_entry(entry, line_number + 1)?;
        }
        emit_lsif_event(
            diagnostic_events,
            format!(
                "lsif index loaded: dump={}, documents={}, ranges={}, hoverResults={}, definitionResults={}",
                path.display(),
                index.documents.len(),
                index.ranges.len(),
                index.hover_results.len(),
                index.definition_items.len()
            ),
        );
        Ok(index)
    }

    fn accept_entry(
        &mut self,
        entry: Value,
        line_number: usize,
    ) -> Result<(), RuntimeCommandError> {
        let entry_type = entry
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| lsif_failed(format!("LSIF line {line_number} is missing type")))?;
        let label = entry
            .get("label")
            .and_then(Value::as_str)
            .ok_or_else(|| lsif_failed(format!("LSIF line {line_number} is missing label")))?;
        match entry_type {
            "vertex" => self.accept_vertex(&entry, label, line_number),
            "edge" => self.accept_edge(&entry, label, line_number),
            other => Err(lsif_failed(format!(
                "unsupported LSIF entry type on line {line_number}: {other}"
            ))),
        }
    }

    fn accept_vertex(
        &mut self,
        entry: &Value,
        label: &str,
        line_number: usize,
    ) -> Result<(), RuntimeCommandError> {
        let id = lsif_id(entry, "id", line_number)?;
        match label {
            "document" => {
                let uri = entry
                    .get("uri")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        lsif_failed(format!(
                            "LSIF document vertex on line {line_number} is missing uri"
                        ))
                    })?
                    .to_string();
                self.documents.insert(id, LsifDocument { uri });
            }
            "range" => {
                self.ranges.insert(
                    id,
                    LsifRange {
                        start: lsif_position(entry.get("start"), line_number, "start")?,
                        end: lsif_position(entry.get("end"), line_number, "end")?,
                    },
                );
            }
            "hoverResult" => {
                let result = entry
                    .get("result")
                    .cloned()
                    .unwrap_or_else(|| json!({ "contents": null }));
                self.hover_results.insert(id, result);
            }
            "resultSet" | "definitionResult" | "metaData" | "project" => {}
            _ => {}
        }
        Ok(())
    }

    fn accept_edge(
        &mut self,
        entry: &Value,
        label: &str,
        line_number: usize,
    ) -> Result<(), RuntimeCommandError> {
        match label {
            "contains" => {
                let document = lsif_id(entry, "outV", line_number)?;
                for range in lsif_id_array(entry, "inVs", line_number)? {
                    self.document_ranges
                        .entry(document.clone())
                        .or_default()
                        .push(range.clone());
                    self.range_documents.insert(range, document.clone());
                }
            }
            "next" => {
                self.range_result_sets.insert(
                    lsif_id(entry, "outV", line_number)?,
                    lsif_id(entry, "inV", line_number)?,
                );
            }
            "textDocument/hover" => {
                self.hover_edges.insert(
                    lsif_id(entry, "outV", line_number)?,
                    lsif_id(entry, "inV", line_number)?,
                );
            }
            "textDocument/definition" => {
                self.definition_edges.insert(
                    lsif_id(entry, "outV", line_number)?,
                    lsif_id(entry, "inV", line_number)?,
                );
            }
            "item" => {
                let out = lsif_id(entry, "outV", line_number)?;
                let document = optional_lsif_id(entry, "document");
                let property = entry
                    .get("property")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                for range in lsif_id_array(entry, "inVs", line_number)? {
                    self.definition_items.entry(out.clone()).or_default().push(
                        LsifDefinitionItem {
                            range,
                            document: document.clone(),
                            property: property.clone(),
                        },
                    );
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn lookup_hover(
        &self,
        request: &LspRuntimeBridgeRequest,
    ) -> Result<Value, RuntimeCommandError> {
        let range_id = self.resolve_range(request, "hover")?;
        let result_set = self.result_set_for_range(&range_id);
        let hover_id = result_set
            .as_ref()
            .and_then(|id| self.hover_edges.get(id))
            .or_else(|| self.hover_edges.get(&range_id))
            .ok_or_else(|| lsif_failed("LSIF hover not found for current range"))?;
        self.hover_results
            .get(hover_id)
            .cloned()
            .ok_or_else(|| lsif_failed("LSIF hover result vertex is missing"))
    }

    fn lookup_definition(
        &self,
        request: &LspRuntimeBridgeRequest,
    ) -> Result<Vec<Value>, RuntimeCommandError> {
        let range_id = self.resolve_range(request, "definition")?;
        let result_set = self.result_set_for_range(&range_id);
        let definition_id = result_set
            .as_ref()
            .and_then(|id| self.definition_edges.get(id))
            .or_else(|| self.definition_edges.get(&range_id))
            .ok_or_else(|| lsif_failed("LSIF definition not found for current range"))?;
        let items = self
            .definition_items
            .get(definition_id)
            .ok_or_else(|| lsif_failed("LSIF definition item edge is missing"))?;
        let locations = items
            .iter()
            .filter(|item| item.property.as_deref().unwrap_or("definitions") == "definitions")
            .map(|item| self.location_for_item(item))
            .collect::<Result<Vec<_>, _>>()?;
        if locations.is_empty() {
            return Err(lsif_failed("LSIF definition locations are empty"));
        }
        Ok(locations)
    }

    fn location_for_item(&self, item: &LsifDefinitionItem) -> Result<Value, RuntimeCommandError> {
        let range = self
            .ranges
            .get(&item.range)
            .ok_or_else(|| lsif_failed("LSIF definition range vertex is missing"))?;
        let document_id = item
            .document
            .as_ref()
            .or_else(|| self.range_documents.get(&item.range))
            .ok_or_else(|| lsif_failed("LSIF definition document vertex is missing"))?;
        let document = self
            .documents
            .get(document_id)
            .ok_or_else(|| lsif_failed("LSIF definition document vertex is missing"))?;
        Ok(json!({
            "uri": document.uri,
            "range": {
                "start": {
                    "line": range.start.line,
                    "character": range.start.character
                },
                "end": {
                    "line": range.end.line,
                    "character": range.end.character
                }
            }
        }))
    }

    fn resolve_range(
        &self,
        request: &LspRuntimeBridgeRequest,
        lookup: &str,
    ) -> Result<LsifId, RuntimeCommandError> {
        let uri = request
            .text_document
            .as_ref()
            .map(|document| document.uri.as_str())
            .ok_or_else(|| {
                lsif_failed(format!("LSIF {lookup} request is missing textDocument.uri"))
            })?;
        let document_id = self
            .documents
            .iter()
            .find_map(|(id, document)| (document.uri == uri).then(|| id.clone()))
            .ok_or_else(|| lsif_failed(format!("LSIF document not found: {uri}")))?;
        let ranges = self
            .document_ranges
            .get(&document_id)
            .ok_or_else(|| lsif_failed(format!("LSIF document has no ranges: {uri}")))?;
        ranges
            .iter()
            .filter_map(|id| self.ranges.get(id).map(|range| (id, range)))
            .filter(|(_, range)| range.contains(request.position.line, request.position.character))
            .min_by_key(|(_, range)| range.span_key())
            .map(|(id, _)| id.clone())
            .ok_or_else(|| {
                lsif_failed(format!(
                    "LSIF {lookup} range not found: uri={uri}, line={}, character={}",
                    request.position.line, request.position.character
                ))
            })
    }

    fn result_set_for_range(&self, range_id: &LsifId) -> Option<LsifId> {
        self.range_result_sets
            .get(range_id)
            .cloned()
            .unwrap_or_else(|| range_id.clone())
            .into()
    }
}

#[derive(Debug, Clone)]
struct LsifDocument {
    uri: String,
}

#[derive(Debug, Clone)]
struct LsifRange {
    start: LsifPosition,
    end: LsifPosition,
}

impl LsifRange {
    fn contains(&self, line: usize, character: usize) -> bool {
        position_cmp(line, character, self.start.line, self.start.character).is_ge()
            && position_cmp(line, character, self.end.line, self.end.character).is_lt()
    }

    fn span_key(&self) -> (usize, usize) {
        (
            self.end.line.saturating_sub(self.start.line),
            self.end.character.saturating_sub(self.start.character),
        )
    }
}

#[derive(Debug, Clone)]
struct LsifPosition {
    line: usize,
    character: usize,
}

#[derive(Debug, Clone)]
struct LsifDefinitionItem {
    range: LsifId,
    document: Option<LsifId>,
    property: Option<String>,
}

type LsifId = String;

fn lsif_id(entry: &Value, field: &str, line_number: usize) -> Result<LsifId, RuntimeCommandError> {
    optional_lsif_id(entry, field).ok_or_else(|| {
        lsif_failed(format!(
            "LSIF line {line_number} is missing id field {field}"
        ))
    })
}

fn optional_lsif_id(entry: &Value, field: &str) -> Option<LsifId> {
    let value = entry.get(field)?;
    if let Some(id) = value.as_str() {
        Some(id.to_string())
    } else if let Some(id) = value.as_u64() {
        Some(id.to_string())
    } else if let Some(id) = value.as_i64() {
        Some(id.to_string())
    } else {
        None
    }
}

fn lsif_id_array(
    entry: &Value,
    field: &str,
    line_number: usize,
) -> Result<Vec<LsifId>, RuntimeCommandError> {
    entry
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| {
            lsif_failed(format!(
                "LSIF line {line_number} is missing id array field {field}"
            ))
        })?
        .iter()
        .map(|value| {
            if let Some(id) = value.as_str() {
                Ok(id.to_string())
            } else if let Some(id) = value.as_u64() {
                Ok(id.to_string())
            } else if let Some(id) = value.as_i64() {
                Ok(id.to_string())
            } else {
                Err(lsif_failed(format!(
                    "LSIF line {line_number} has invalid id in {field}"
                )))
            }
        })
        .collect()
}

fn lsif_position(
    value: Option<&Value>,
    line_number: usize,
    field: &str,
) -> Result<LsifPosition, RuntimeCommandError> {
    let value =
        value.ok_or_else(|| lsif_failed(format!("LSIF line {line_number} is missing {field}")))?;
    Ok(LsifPosition {
        line: value.get("line").and_then(Value::as_u64).ok_or_else(|| {
            lsif_failed(format!("LSIF line {line_number} is missing {field}.line"))
        })? as usize,
        character: value
            .get("character")
            .and_then(Value::as_u64)
            .ok_or_else(|| {
                lsif_failed(format!(
                    "LSIF line {line_number} is missing {field}.character"
                ))
            })? as usize,
    })
}

fn resolve_dump_path(request: &LspRuntimeBridgeRequest) -> Result<PathBuf, RuntimeCommandError> {
    if request.dump_path.trim().is_empty() {
        return Err(lsif_failed("LSIF request is missing dumpPath"));
    }
    if let Some(path) = request.dump_path.strip_prefix("file://") {
        return Ok(PathBuf::from(percent_decode_path(path)));
    }
    let path = PathBuf::from(&request.dump_path);
    if path.is_absolute() {
        return Ok(path);
    }
    if let Some(root_uri) = request
        .root_uri
        .as_deref()
        .and_then(|uri| uri.strip_prefix("file://"))
    {
        return Ok(PathBuf::from(percent_decode_path(root_uri)).join(path));
    }
    std::env::current_dir()
        .map(|current_dir| current_dir.join(path))
        .map_err(|error| {
            lsif_failed(format!(
                "failed to resolve relative LSIF dump path: {error}"
            ))
        })
}

fn percent_decode_path(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[index + 1..index + 3]) {
                if let Ok(value) = u8::from_str_radix(hex, 16) {
                    decoded.push(value);
                    index += 3;
                    continue;
                }
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).to_string()
}

fn position_cmp(
    left_line: usize,
    left_character: usize,
    right_line: usize,
    right_character: usize,
) -> std::cmp::Ordering {
    left_line
        .cmp(&right_line)
        .then_with(|| left_character.cmp(&right_character))
}

fn lsif_failed(message: impl Into<String>) -> RuntimeCommandError {
    RuntimeCommandError::CommandFailed {
        name: "lsif.request".to_string(),
        message: message.into(),
    }
}

fn emit_lsif_event(diagnostic_events: &Arc<Mutex<Vec<String>>>, message: String) {
    log::debug!("[lsif_index] {message}");
    if let Ok(mut events) = diagnostic_events.lock() {
        events.push(message);
    }
}
