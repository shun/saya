use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::runtime::live::{ReadonlyBufferSnapshot, ReadonlyEditorSnapshot, RuntimeCommandError};

pub const LSP_RUNTIME_BRIDGE_DECISION: &str =
    "LSP is a built-in host capability exposed through a stable TypeScript runtime API.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LspRuntimeBridgeSource {
    Lsp,
    Lsif,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LspRuntimeTextDocument {
    pub uri: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LspRuntimePosition {
    pub line: usize,
    pub character: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LspRuntimeServerDefinition {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
    pub cwd: Option<String>,
    #[serde(default)]
    pub root_markers: Vec<String>,
    pub initialization_options: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LspRuntimeBridgeRequest {
    pub source: LspRuntimeBridgeSource,
    #[serde(rename = "lspVersion", alias = "protocolVersion")]
    pub protocol_version: String,
    pub method: String,
    pub client_name: String,
    pub root_uri: Option<String>,
    pub language_id: String,
    pub trace: String,
    pub position_encoding: String,
    pub dump_path: String,
    pub text_document: Option<LspRuntimeTextDocument>,
    pub server: Option<LspRuntimeServerDefinition>,
    pub position: LspRuntimePosition,
    pub params: Option<Value>,
    pub buffer: ReadonlyBufferSnapshot,
    pub editor: ReadonlyEditorSnapshot,
    pub event: Option<Value>,
}

impl LspRuntimeBridgeRequest {
    pub fn validate(&self) -> Result<(), RuntimeCommandError> {
        let missing = if self.protocol_version.trim().is_empty() {
            Some("lspVersion")
        } else if self.method.trim().is_empty() {
            Some("method")
        } else if self.client_name.trim().is_empty() {
            Some("clientName")
        } else if self.language_id.trim().is_empty() {
            Some("languageId")
        } else {
            None
        };
        if let Some(field) = missing {
            return Err(RuntimeCommandError::CommandFailed {
                name: "lsp.request".to_string(),
                message: format!("invalid LSP bridge request: missing {field}"),
            });
        }
        if !matches!(
            self.position_encoding.as_str(),
            "utf-16" | "utf-8" | "utf-32"
        ) {
            return Err(RuntimeCommandError::CommandFailed {
                name: "lsp.request".to_string(),
                message: format!(
                    "invalid LSP bridge request: unsupported positionEncoding {}",
                    self.position_encoding
                ),
            });
        }
        if let Some(root_uri) = self.root_uri.as_ref() {
            if !root_uri.starts_with("file://") {
                return Err(RuntimeCommandError::CommandFailed {
                    name: "lsp.request".to_string(),
                    message: format!("invalid LSP bridge request: invalid rootUri {root_uri}"),
                });
            }
        }
        if let Some(server) = self.server.as_ref() {
            let invalid_server_field = if server.name.trim().is_empty() {
                Some("server.name")
            } else if server.command.trim().is_empty() {
                Some("server.command")
            } else {
                None
            };
            if let Some(field) = invalid_server_field {
                return Err(RuntimeCommandError::CommandFailed {
                    name: "lsp.request".to_string(),
                    message: format!("invalid LSP bridge request: missing {field}"),
                });
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LspRuntimeBridgeResponse {
    pub source: LspRuntimeBridgeSource,
    pub method: String,
    pub result: Value,
}
