use std::collections::HashMap;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::features::completion::float::{CompletionCandidate, CompletionMenuFloatRequest};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionPosition {
    pub line: usize,
    pub character: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionRange {
    pub start: CompletionPosition,
    pub end: CompletionPosition,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostCompletionCandidate {
    pub label: String,
    #[serde(default, deserialize_with = "optional_string_from_json")]
    pub insert_text: Option<String>,
    #[serde(default, deserialize_with = "optional_string_from_json")]
    pub kind: Option<String>,
    #[serde(default, deserialize_with = "optional_string_from_json")]
    pub detail: Option<String>,
    #[serde(default, deserialize_with = "documentation_from_json")]
    pub documentation: Vec<String>,
    #[serde(default, deserialize_with = "optional_string_from_json")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompletionShowRequest {
    pub session_id: String,
    pub request_id: u64,
    pub replace_range: CompletionRange,
    #[serde(default)]
    pub candidates: Vec<HostCompletionCandidate>,
    #[serde(default)]
    pub selected_index: usize,
    #[serde(default = "default_max_visible_items")]
    pub max_visible_items: usize,
    #[serde(default = "default_documentation_max_width")]
    pub documentation_max_width: u16,
    #[serde(default = "default_documentation_max_height")]
    pub documentation_max_height: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptedCompletionRequest {
    pub session_id: String,
    pub request_id: u64,
    pub replace_range: CompletionRange,
    pub candidates: Vec<HostCompletionCandidate>,
    pub selected_index: usize,
    pub max_visible_items: usize,
    pub documentation_max_width: u16,
    pub documentation_max_height: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionEdit {
    pub replacement_text: String,
    pub updated_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleCompletionRequest {
    pub session_id: String,
    pub request_id: u64,
    pub current_request_id: u64,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CompletionSessionManager {
    latest_request_by_session: HashMap<String, u64>,
}

impl CompletionSessionManager {
    pub fn accept_show_request(
        &mut self,
        request: CompletionShowRequest,
    ) -> Result<AcceptedCompletionRequest, StaleCompletionRequest> {
        let current_request_id = self
            .latest_request_by_session
            .get(&request.session_id)
            .copied()
            .unwrap_or(0);
        if request.request_id < current_request_id {
            log::debug!(
                "[completion_session] stale completion rejected: session_id={}, request_id={}, current_request_id={}",
                request.session_id,
                request.request_id,
                current_request_id
            );
            return Err(StaleCompletionRequest {
                session_id: request.session_id,
                request_id: request.request_id,
                current_request_id,
            });
        }
        self.latest_request_by_session
            .insert(request.session_id.clone(), request.request_id);
        let selected_index = request
            .selected_index
            .min(request.candidates.len().saturating_sub(1));
        log::debug!(
            "[completion_session] completion request accepted: session_id={}, request_id={}, candidates={}, selected_index={}, replace=({}:{})-({}:{})",
            request.session_id,
            request.request_id,
            request.candidates.len(),
            selected_index,
            request.replace_range.start.line,
            request.replace_range.start.character,
            request.replace_range.end.line,
            request.replace_range.end.character
        );
        Ok(AcceptedCompletionRequest {
            session_id: request.session_id,
            request_id: request.request_id,
            replace_range: request.replace_range,
            candidates: request.candidates,
            selected_index,
            max_visible_items: request.max_visible_items,
            documentation_max_width: request.documentation_max_width,
            documentation_max_height: request.documentation_max_height,
        })
    }
}

impl AcceptedCompletionRequest {
    pub fn to_float_request(
        &self,
        window_id: i32,
        cursor_row: usize,
        cursor_col: usize,
    ) -> CompletionMenuFloatRequest {
        CompletionMenuFloatRequest {
            window_id,
            cursor_row,
            cursor_col,
            candidates: self
                .candidates
                .iter()
                .cloned()
                .map(|candidate| {
                    let mut candidate: CompletionCandidate = candidate.into();
                    candidate.replace_range = Some(self.replace_range.clone());
                    candidate
                })
                .collect(),
            selected_index: self.selected_index,
            max_visible_items: self.max_visible_items,
            documentation_max_width: self.documentation_max_width,
            documentation_max_height: self.documentation_max_height,
        }
    }

    pub fn confirm(&self, selected_index: usize, buffer_text: &str) -> Option<CompletionEdit> {
        let candidate = self.candidates.get(selected_index)?;
        let replacement_text = candidate
            .insert_text
            .as_deref()
            .filter(|text| !text.is_empty())
            .unwrap_or(candidate.label.as_str())
            .to_string();
        let updated_text =
            apply_replace_range(buffer_text, &self.replace_range, &replacement_text)?;
        log::debug!(
            "[completion_session] completion confirmed: session_id={}, request_id={}, selected_index={}, label={:?}, replacement_len={}",
            self.session_id,
            self.request_id,
            selected_index,
            candidate.label,
            replacement_text.len()
        );
        Some(CompletionEdit {
            replacement_text,
            updated_text,
        })
    }
}

impl From<HostCompletionCandidate> for CompletionCandidate {
    fn from(value: HostCompletionCandidate) -> Self {
        Self {
            label: value.label,
            insert_text: value.insert_text,
            detail: value.detail,
            kind: value.kind,
            documentation: value.documentation,
            source: value.source,
            replace_range: None,
        }
    }
}

pub fn apply_replace_range(
    buffer_text: &str,
    range: &CompletionRange,
    replacement_text: &str,
) -> Option<String> {
    if range.start.line > range.end.line {
        return None;
    }
    let mut lines = buffer_text
        .split_inclusive('\n')
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if lines.is_empty() {
        lines.push(String::new());
    }
    let start_line = lines.get(range.start.line)?;
    let end_line = lines.get(range.end.line)?;
    let start = range
        .start
        .character
        .min(start_line.trim_end_matches('\n').len());
    let end = range
        .end
        .character
        .min(end_line.trim_end_matches('\n').len());
    if range.start.line == range.end.line && start > end {
        return None;
    }
    let prefix = lines[range.start.line][..start].to_string();
    let suffix_start = if range.start.line == range.end.line {
        end
    } else {
        end.min(lines[range.end.line].len())
    };
    let suffix = lines[range.end.line][suffix_start..].to_string();
    let merged = format!("{prefix}{replacement_text}{suffix}");
    lines.splice(range.start.line..=range.end.line, [merged]);
    Some(lines.concat())
}

fn default_max_visible_items() -> usize {
    8
}

fn default_documentation_max_width() -> u16 {
    72
}

fn default_documentation_max_height() -> u16 {
    12
}

fn optional_string_from_json<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(value.and_then(|value| match value {
        Value::String(text) => {
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }))
}

fn documentation_from_json<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(match value {
        Some(Value::String(text)) => normalize_documentation_lines(&text),
        Some(Value::Array(items)) => items
            .into_iter()
            .filter_map(|item| match item {
                Value::String(text) => Some(normalize_documentation_lines(&text)),
                _ => None,
            })
            .flatten()
            .collect(),
        Some(Value::Object(object)) => object
            .get("value")
            .and_then(Value::as_str)
            .map(normalize_documentation_lines)
            .unwrap_or_default(),
        _ => Vec::new(),
    })
}

fn normalize_documentation_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .map(ToString::to_string)
        .collect()
}
