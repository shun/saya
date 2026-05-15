use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::features::selector::core::{
    CancellationToken, CollectProcessor, DefaultRenderer, InMemoryResultStore, MatchProcessor,
    PrefixAndMatcher, RenderProcessor, ResultStore, SelectorCollectStatus, SelectorController,
    SelectorControllerCommand, SelectorError, SelectorHighlight, SelectorItem, SelectorLimits,
    SelectorMatchStatus, SelectorResultStoreStatus, SelectorSessionCore, SelectorStorageMode,
    SelectorViewState, SubstringAndMatcher, SuffixAndMatcher, WorkState,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSelectorOpenRequest {
    pub source: RuntimeSelectorSourceRequest,
    #[serde(default = "default_runtime_selector_matcher")]
    pub matcher: RuntimeSelectorMatcherName,
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub limits: RuntimeSelectorLimits,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RuntimeSelectorSourceRequest {
    Static {
        #[serde(default)]
        items: Vec<RuntimeSelectorItem>,
    },
    Rg {
        #[serde(default)]
        root: Option<String>,
        pattern: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSelectorUpdateRequest {
    pub query: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSelectorControlRequest {
    pub command: RuntimeSelectorControllerCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSelectorItem {
    pub id: String,
    pub value: String,
    pub kind: String,
    #[serde(default)]
    pub detail: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSelectorLimits {
    #[serde(default = "default_max_rendered_items")]
    pub max_rendered_items: usize,
}

impl Default for RuntimeSelectorLimits {
    fn default() -> Self {
        Self {
            max_rendered_items: default_max_rendered_items(),
        }
    }
}

impl From<RuntimeSelectorLimits> for SelectorLimits {
    fn from(value: RuntimeSelectorLimits) -> Self {
        Self {
            max_rendered_items: value.max_rendered_items,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeSelectorMatcherName {
    PrefixAnd,
    SubstringAnd,
    SuffixAnd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeSelectorControllerCommand {
    CursorNext,
    CursorPrevious,
    CursorFirst,
    CursorLast,
    PageDown,
    PageUp,
    Hide,
    Cancel,
}

fn default_runtime_selector_matcher() -> RuntimeSelectorMatcherName {
    RuntimeSelectorMatcherName::SubstringAnd
}

fn default_max_rendered_items() -> usize {
    SelectorLimits::default().max_rendered_items
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSelectorSnapshot {
    pub id: u64,
    pub query: String,
    pub rendered_items: Vec<RuntimeRenderedSelectorItem>,
    pub selected_item: Option<RuntimeRenderedSelectorItem>,
    pub view: RuntimeSelectorViewState,
    pub status: RuntimeSelectorStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeRenderedSelectorItem {
    pub id: String,
    pub label: String,
    pub kind: String,
    #[serde(default)]
    pub detail: Value,
    pub highlights: Vec<RuntimeSelectorHighlight>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeRgLocation {
    pub path: PathBuf,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSelectorHighlight {
    pub column: usize,
    pub width: usize,
    pub kind: RuntimeSelectorHighlightKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSelectorViewState {
    pub cursor: usize,
    pub offset: usize,
    pub rendered_items_len: usize,
    pub hidden: bool,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeSelectorHighlightKind {
    Match,
    Selection,
    Diagnostic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSelectorStatus {
    pub collect: RuntimeSelectorCollectStatus,
    #[serde(rename = "match")]
    pub match_status: RuntimeSelectorMatchStatus,
    pub store: RuntimeSelectorStoreStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSelectorCollectStatus {
    pub state: RuntimeSelectorWorkState,
    pub total_seen: usize,
    pub total_stored: usize,
    pub storage: RuntimeSelectorStorageMode,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSelectorMatchStatus {
    pub state: RuntimeSelectorWorkState,
    pub total_matched: usize,
    pub total_rendered: usize,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSelectorStoreStatus {
    pub storage: RuntimeSelectorStorageMode,
    pub total_stored: usize,
    pub estimated_bytes: Option<usize>,
    pub temp_file_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeSelectorStorageMode {
    Memory,
    TempFile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeSelectorWorkState {
    Idle,
    Running,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeSelectorError {
    InvalidRequest(String),
    SourceUnavailable(String),
    SourceFailed(String),
    SessionNotFound(u64),
    SessionCancelled(u64),
    Core(String),
}

impl std::fmt::Display for RuntimeSelectorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest(message) => write!(f, "invalid selector request: {message}"),
            Self::SourceUnavailable(message) => write!(f, "selector source unavailable: {message}"),
            Self::SourceFailed(message) => write!(f, "selector source failed: {message}"),
            Self::SessionNotFound(id) => write!(f, "selector session not found: {id}"),
            Self::SessionCancelled(id) => write!(f, "selector session is cancelled: {id}"),
            Self::Core(message) => write!(f, "selector core failed: {message}"),
        }
    }
}

impl std::error::Error for RuntimeSelectorError {}

impl From<SelectorError> for RuntimeSelectorError {
    fn from(value: SelectorError) -> Self {
        Self::Core(value.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorViewBackendInput {
    pub session_id: u64,
    pub query: String,
    pub rendered_items: Vec<RuntimeRenderedSelectorItem>,
    pub selected_item: Option<RuntimeRenderedSelectorItem>,
    pub cursor: usize,
    pub offset: usize,
    pub hidden: bool,
    pub cancelled: bool,
    pub status: RuntimeSelectorStatus,
}

pub trait SelectorViewBackend: Send + Sync + 'static {
    fn render(&self, input: SelectorViewBackendInput);
}

#[derive(Debug, Default)]
pub struct HeadlessSelectorViewBackend {
    frames: Mutex<Vec<SelectorViewBackendInput>>,
}

impl HeadlessSelectorViewBackend {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn frames(&self) -> Vec<SelectorViewBackendInput> {
        self.frames
            .lock()
            .expect("headless selector view backend poisoned")
            .clone()
    }
}

impl SelectorViewBackend for HeadlessSelectorViewBackend {
    fn render(&self, input: SelectorViewBackendInput) {
        log::debug!(
            "[selector_runtime] headless view backend render: id={}, query_len={}, rendered={}, cursor={}, offset={}, hidden={}, cancelled={}, match_state={:?}",
            input.session_id,
            input.query.len(),
            input.rendered_items.len(),
            input.cursor,
            input.offset,
            input.hidden,
            input.cancelled,
            input.status.match_status.state
        );
        self.frames
            .lock()
            .expect("headless selector view backend poisoned")
            .push(input);
    }
}

#[derive(Default)]
pub struct RuntimeSelectorSessions {
    next_id: u64,
    sessions: HashMap<u64, RuntimeSelectorSession>,
    view_backend: Option<Arc<dyn SelectorViewBackend>>,
}

impl std::fmt::Debug for RuntimeSelectorSessions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeSelectorSessions")
            .field("next_id", &self.next_id)
            .field("sessions", &self.sessions)
            .field("has_view_backend", &self.view_backend.is_some())
            .finish()
    }
}

impl RuntimeSelectorSessions {
    pub fn new(view_backend: Option<Arc<dyn SelectorViewBackend>>) -> Self {
        Self {
            next_id: 0,
            sessions: HashMap::new(),
            view_backend,
        }
    }

    pub fn open(
        &mut self,
        request: RuntimeSelectorOpenRequest,
    ) -> Result<RuntimeSelectorSnapshot, RuntimeSelectorError> {
        let id = self.allocate_id();
        log::debug!(
            "[selector_runtime] open headless selector session: id={id}, matcher={:?}, query_len={}, max_rendered={}",
            request.matcher,
            request.query.len(),
            request.limits.max_rendered_items
        );

        let mut store = InMemoryResultStore::new();
        let collect_token = CancellationToken::new();
        let items = match request.source {
            RuntimeSelectorSourceRequest::Static { items } => items
                .into_iter()
                .map(|item| SelectorItem {
                    id: item.id,
                    value: item.value,
                    kind: item.kind,
                    detail: item.detail,
                })
                .collect::<Vec<_>>(),
            RuntimeSelectorSourceRequest::Rg { .. } => {
                return Err(RuntimeSelectorError::InvalidRequest(
                    "rg selector source must be resolved by the runtime host before opening"
                        .to_string(),
                ));
            }
        };
        let collect_status = CollectProcessor::new()
            .collect_items(&mut store, items, &collect_token)?
            .status;
        let mut session = RuntimeSelectorSession {
            id,
            query: request.query,
            matcher: request.matcher,
            limits: request.limits.into(),
            store,
            rendered_items: Vec::new(),
            view: SelectorViewState::new(0),
            collect_status,
            match_status: idle_match_status(),
            core: SelectorSessionCore::new(),
            cancelled: false,
        };
        session.refresh()?;
        let snapshot = session.snapshot();
        self.sessions.insert(id, session);
        self.publish_view(&snapshot);
        Ok(snapshot)
    }

    pub fn update(
        &mut self,
        id: u64,
        request: RuntimeSelectorUpdateRequest,
    ) -> Result<RuntimeSelectorSnapshot, RuntimeSelectorError> {
        let session = self.session_mut(id)?;
        if session.cancelled {
            log::debug!("[selector_runtime] reject update for cancelled selector session: id={id}");
            return Err(RuntimeSelectorError::SessionCancelled(id));
        }
        log::debug!(
            "[selector_runtime] update selector query: id={id}, query_len={}",
            request.query.len()
        );
        session.query = request.query;
        session.refresh()?;
        let snapshot = session.snapshot();
        self.publish_view(&snapshot);
        Ok(snapshot)
    }

    pub fn current(&self, id: u64) -> Result<RuntimeSelectorSnapshot, RuntimeSelectorError> {
        Ok(self.session(id)?.snapshot())
    }

    pub fn control(
        &mut self,
        id: u64,
        request: RuntimeSelectorControlRequest,
    ) -> Result<RuntimeSelectorSnapshot, RuntimeSelectorError> {
        let session = self.session_mut(id)?;
        log::debug!(
            "[selector_runtime] control selector session: id={id}, command={:?}",
            request.command
        );
        session.control(request.command.into());
        let snapshot = session.snapshot();
        self.publish_view(&snapshot);
        Ok(snapshot)
    }

    pub fn cancel(&mut self, id: u64) -> Result<RuntimeSelectorSnapshot, RuntimeSelectorError> {
        let session = self.session_mut(id)?;
        log::debug!("[selector_runtime] cancel selector session: id={id}");
        session.control(SelectorControllerCommand::Cancel);
        let snapshot = session.snapshot();
        self.publish_view(&snapshot);
        Ok(snapshot)
    }

    pub fn dispose(&mut self, id: u64) -> Result<bool, RuntimeSelectorError> {
        let Some(mut session) = self.sessions.remove(&id) else {
            return Err(RuntimeSelectorError::SessionNotFound(id));
        };
        log::debug!("[selector_runtime] dispose selector session: id={id}");
        session.core.cancel_active_work();
        session.store.dispose()?;
        Ok(true)
    }

    fn allocate_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn session(&self, id: u64) -> Result<&RuntimeSelectorSession, RuntimeSelectorError> {
        self.sessions
            .get(&id)
            .ok_or(RuntimeSelectorError::SessionNotFound(id))
    }

    fn session_mut(
        &mut self,
        id: u64,
    ) -> Result<&mut RuntimeSelectorSession, RuntimeSelectorError> {
        self.sessions
            .get_mut(&id)
            .ok_or(RuntimeSelectorError::SessionNotFound(id))
    }

    fn publish_view(&self, snapshot: &RuntimeSelectorSnapshot) {
        let Some(backend) = &self.view_backend else {
            log::debug!(
                "[selector_runtime] skip selector view backend render: id={}, reason=no-backend",
                snapshot.id
            );
            return;
        };
        log::debug!(
            "[selector_runtime] publish selector view backend input: id={}, rendered={}, cursor={}, offset={}, hidden={}, cancelled={}",
            snapshot.id,
            snapshot.rendered_items.len(),
            snapshot.view.cursor,
            snapshot.view.offset,
            snapshot.view.hidden,
            snapshot.view.cancelled
        );
        backend.render(snapshot.clone().into());
    }
}

pub fn parse_rg_vimgrep_output(
    output: &str,
) -> Result<Vec<RuntimeSelectorItem>, RuntimeSelectorError> {
    let mut items = Vec::new();
    for (index, line) in output.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(4, ':');
        let path = parts
            .next()
            .ok_or_else(|| invalid_rg_vimgrep_line(index, line))?;
        let line_number = parts
            .next()
            .ok_or_else(|| invalid_rg_vimgrep_line(index, line))?
            .parse::<u64>()
            .map_err(|_| invalid_rg_vimgrep_line(index, line))?;
        let column = parts
            .next()
            .ok_or_else(|| invalid_rg_vimgrep_line(index, line))?
            .parse::<u64>()
            .map_err(|_| invalid_rg_vimgrep_line(index, line))?;
        let text = parts
            .next()
            .ok_or_else(|| invalid_rg_vimgrep_line(index, line))?;
        let value = format!("{path}:{line_number}:{column}:{text}");
        items.push(RuntimeSelectorItem {
            id: format!("rg:{path}:{line_number}:{column}:{index}"),
            value,
            kind: "rg".to_string(),
            detail: serde_json::json!({
                "path": path,
                "line": line_number,
                "column": column,
                "text": text,
            }),
        });
    }
    Ok(items)
}

pub fn parse_rg_selector_location_detail(
    item: &RuntimeRenderedSelectorItem,
) -> Result<RuntimeRgLocation, RuntimeSelectorError> {
    if item.kind != "rg" {
        return Err(RuntimeSelectorError::InvalidRequest(format!(
            "selector item kind is not supported for jump action: {}",
            item.kind
        )));
    }
    let path = item
        .detail
        .get("path")
        .and_then(Value::as_str)
        .filter(|path| !path.trim().is_empty())
        .ok_or_else(|| {
            RuntimeSelectorError::InvalidRequest(
                "rg selector item detail.path is required".to_string(),
            )
        })?;
    let line = rg_location_positive_usize(&item.detail, "line")?;
    let column = rg_location_positive_usize(&item.detail, "column")?;

    Ok(RuntimeRgLocation {
        path: PathBuf::from(path),
        line,
        column,
    })
}

fn rg_location_positive_usize(
    detail: &Value,
    field: &'static str,
) -> Result<usize, RuntimeSelectorError> {
    let value = detail.get(field).and_then(Value::as_u64).ok_or_else(|| {
        RuntimeSelectorError::InvalidRequest(format!("rg selector item detail.{field} is required"))
    })?;
    if value == 0 {
        return Err(RuntimeSelectorError::InvalidRequest(format!(
            "rg selector item detail.{field} must be 1-based"
        )));
    }
    usize::try_from(value).map_err(|_| {
        RuntimeSelectorError::InvalidRequest(format!(
            "rg selector item detail.{field} is too large"
        ))
    })
}

fn invalid_rg_vimgrep_line(index: usize, line: &str) -> RuntimeSelectorError {
    RuntimeSelectorError::SourceFailed(format!(
        "rg --vimgrep output line {index} is not file:line:column:text: {line:?}"
    ))
}

#[derive(Debug)]
struct RuntimeSelectorSession {
    id: u64,
    query: String,
    matcher: RuntimeSelectorMatcherName,
    limits: SelectorLimits,
    store: InMemoryResultStore<Value>,
    rendered_items: Vec<RuntimeRenderedSelectorItem>,
    view: SelectorViewState,
    collect_status: SelectorCollectStatus,
    match_status: SelectorMatchStatus,
    core: SelectorSessionCore,
    cancelled: bool,
}

impl RuntimeSelectorSession {
    fn refresh(&mut self) -> Result<(), RuntimeSelectorError> {
        let work_id = self.core.begin_match_work();
        let token = CancellationToken::new();
        let matched = match self.matcher {
            RuntimeSelectorMatcherName::PrefixAnd => MatchProcessor::new().match_store(
                &self.store,
                &PrefixAndMatcher,
                &self.query,
                &token,
            )?,
            RuntimeSelectorMatcherName::SubstringAnd => MatchProcessor::new().match_store(
                &self.store,
                &SubstringAndMatcher,
                &self.query,
                &token,
            )?,
            RuntimeSelectorMatcherName::SuffixAnd => MatchProcessor::new().match_store(
                &self.store,
                &SuffixAndMatcher,
                &self.query,
                &token,
            )?,
        };
        if !self.core.is_active_work(work_id) {
            log::debug!(
                "[selector_runtime] suppress stale match result: id={}, query_len={}",
                self.id,
                self.query.len()
            );
            return Ok(());
        }

        let rendered =
            RenderProcessor::new(self.limits).render(&matched.items, &DefaultRenderer, &token)?;
        if !self.core.is_active_work(work_id) {
            log::debug!(
                "[selector_runtime] suppress stale render result: id={}, query_len={}",
                self.id,
                self.query.len()
            );
            return Ok(());
        }

        self.match_status = rendered.status;
        self.rendered_items = rendered
            .items
            .into_iter()
            .map(|rendered_item| {
                let source_item = self.store.get(&rendered_item.id)?;
                let (kind, detail) = source_item
                    .map(|item| (item.kind, item.detail))
                    .unwrap_or_else(|| (String::new(), Value::Null));
                Ok(RuntimeRenderedSelectorItem {
                    id: rendered_item.id,
                    label: rendered_item.label,
                    kind,
                    detail,
                    highlights: rendered_item
                        .highlights
                        .into_iter()
                        .map(Into::into)
                        .collect(),
                })
            })
            .collect::<Result<Vec<_>, RuntimeSelectorError>>()?;
        self.view.update_rendered_items_len(
            self.rendered_items.len(),
            runtime_selector_controller_page_size(),
        );
        log::debug!(
            "[selector_runtime] refresh completed: id={}, matched={}, rendered={}",
            self.id,
            self.match_status.total_matched,
            self.match_status.total_rendered
        );
        Ok(())
    }

    fn snapshot(&self) -> RuntimeSelectorSnapshot {
        RuntimeSelectorSnapshot {
            id: self.id,
            query: self.query.clone(),
            rendered_items: self.rendered_items.clone(),
            selected_item: self
                .view
                .selected_index()
                .and_then(|index| self.rendered_items.get(index).cloned()),
            view: self.view.clone().into(),
            status: RuntimeSelectorStatus {
                collect: self.collect_status.clone().into(),
                match_status: self.match_status.clone().into(),
                store: self.store.status().into(),
            },
        }
    }

    fn control(&mut self, command: SelectorControllerCommand) {
        SelectorController::new(runtime_selector_controller_page_size())
            .apply(&mut self.view, command);
        if command == SelectorControllerCommand::Cancel {
            self.cancelled = true;
            self.core.cancel_active_work();
            self.match_status.state = WorkState::Cancelled;
        }
    }
}

fn runtime_selector_controller_page_size() -> usize {
    10
}

fn idle_match_status() -> SelectorMatchStatus {
    SelectorMatchStatus {
        state: WorkState::Idle,
        total_matched: 0,
        total_rendered: 0,
        error_message: None,
    }
}

impl From<SelectorCollectStatus> for RuntimeSelectorCollectStatus {
    fn from(value: SelectorCollectStatus) -> Self {
        Self {
            state: value.state.into(),
            total_seen: value.total_seen,
            total_stored: value.total_stored,
            storage: value.storage.into(),
            error_message: value.error_message,
        }
    }
}

impl From<SelectorMatchStatus> for RuntimeSelectorMatchStatus {
    fn from(value: SelectorMatchStatus) -> Self {
        Self {
            state: value.state.into(),
            total_matched: value.total_matched,
            total_rendered: value.total_rendered,
            error_message: value.error_message,
        }
    }
}

impl From<SelectorResultStoreStatus> for RuntimeSelectorStoreStatus {
    fn from(value: SelectorResultStoreStatus) -> Self {
        Self {
            storage: value.storage.into(),
            total_stored: value.total_stored,
            estimated_bytes: value.estimated_bytes,
            temp_file_path: value.temp_file_path,
        }
    }
}

impl From<SelectorStorageMode> for RuntimeSelectorStorageMode {
    fn from(value: SelectorStorageMode) -> Self {
        match value {
            SelectorStorageMode::Memory => Self::Memory,
            SelectorStorageMode::TempFile => Self::TempFile,
        }
    }
}

impl From<WorkState> for RuntimeSelectorWorkState {
    fn from(value: WorkState) -> Self {
        match value {
            WorkState::Idle => Self::Idle,
            WorkState::Running => Self::Running,
            WorkState::Completed => Self::Completed,
            WorkState::Cancelled => Self::Cancelled,
            WorkState::Failed => Self::Failed,
        }
    }
}

impl From<RuntimeSelectorControllerCommand> for SelectorControllerCommand {
    fn from(value: RuntimeSelectorControllerCommand) -> Self {
        match value {
            RuntimeSelectorControllerCommand::CursorNext => Self::CursorNext,
            RuntimeSelectorControllerCommand::CursorPrevious => Self::CursorPrevious,
            RuntimeSelectorControllerCommand::CursorFirst => Self::CursorFirst,
            RuntimeSelectorControllerCommand::CursorLast => Self::CursorLast,
            RuntimeSelectorControllerCommand::PageDown => Self::PageDown,
            RuntimeSelectorControllerCommand::PageUp => Self::PageUp,
            RuntimeSelectorControllerCommand::Hide => Self::Hide,
            RuntimeSelectorControllerCommand::Cancel => Self::Cancel,
        }
    }
}

impl From<RuntimeSelectorSnapshot> for SelectorViewBackendInput {
    fn from(value: RuntimeSelectorSnapshot) -> Self {
        Self {
            session_id: value.id,
            query: value.query,
            rendered_items: value.rendered_items,
            selected_item: value.selected_item,
            cursor: value.view.cursor,
            offset: value.view.offset,
            hidden: value.view.hidden,
            cancelled: value.view.cancelled,
            status: value.status,
        }
    }
}

impl From<SelectorViewState> for RuntimeSelectorViewState {
    fn from(value: SelectorViewState) -> Self {
        Self {
            cursor: value.cursor,
            offset: value.offset,
            rendered_items_len: value.rendered_items_len,
            hidden: value.hidden,
            cancelled: value.cancelled,
        }
    }
}

impl From<SelectorHighlight> for RuntimeSelectorHighlight {
    fn from(value: SelectorHighlight) -> Self {
        Self {
            column: value.column,
            width: value.width,
            kind: match value.kind {
                crate::features::selector::core::HighlightKind::Match => {
                    RuntimeSelectorHighlightKind::Match
                }
                crate::features::selector::core::HighlightKind::Selection => {
                    RuntimeSelectorHighlightKind::Selection
                }
                crate::features::selector::core::HighlightKind::Diagnostic => {
                    RuntimeSelectorHighlightKind::Diagnostic
                }
            },
        }
    }
}
