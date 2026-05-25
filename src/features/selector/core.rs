use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorItem<TDetail = ()> {
    pub id: String,
    pub value: String,
    pub kind: String,
    pub detail: TDetail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchedItem<TDetail = ()> {
    pub item: SelectorItem<TDetail>,
    pub score: Option<i64>,
    pub highlights: Vec<SelectorHighlight>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorHighlight {
    pub column: usize,
    pub width: usize,
    pub kind: HighlightKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightKind {
    Match,
    Selection,
    Diagnostic,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedItem {
    pub id: String,
    pub label: String,
    pub highlights: Vec<SelectorHighlight>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewContent {
    pub item_id: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorSessionData<TDetail = ()> {
    pub query: String,
    pub matched_items: Vec<MatchedItem<TDetail>>,
    pub rendered_items: Vec<RenderedItem>,
    pub cursor: usize,
    pub offset: usize,
    pub collect_status: SelectorCollectStatus,
    pub match_status: SelectorMatchStatus,
    pub preview_status: SelectorPreviewStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorQuery {
    terms: Vec<String>,
}

impl SelectorQuery {
    pub fn terms(&self) -> &[String] {
        &self.terms
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }
}

pub fn parse_ascii_space_and_query(input: &str) -> SelectorQuery {
    let terms = input
        .split(' ')
        .filter(|term| !term.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    log::debug!(
        "[selector_core] parsed query: input_len={}, terms={}",
        input.len(),
        terms.len()
    );
    SelectorQuery { terms }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkState {
    Idle,
    Running,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectorStorageMode {
    Memory,
    TempFile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorResultStoreStatus {
    pub storage: SelectorStorageMode,
    pub total_stored: usize,
    pub estimated_bytes: Option<usize>,
    pub temp_file_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorCollectStatus {
    pub state: WorkState,
    pub total_seen: usize,
    pub total_stored: usize,
    pub storage: SelectorStorageMode,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorMatchStatus {
    pub state: WorkState,
    pub total_matched: usize,
    pub total_rendered: usize,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorPreviewStatus {
    pub state: WorkState,
    pub item_id: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectorLimits {
    pub max_rendered_items: usize,
}

impl Default for SelectorLimits {
    fn default() -> Self {
        Self {
            max_rendered_items: 1000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectorViewState {
    pub cursor: usize,
    pub offset: usize,
    pub rendered_items_len: usize,
    pub hidden: bool,
    pub cancelled: bool,
}

impl SelectorViewState {
    pub fn new(rendered_items_len: usize) -> Self {
        log::debug!(
            "[selector_core] initialize selector view state: rendered_items_len={rendered_items_len}"
        );
        Self {
            cursor: 0,
            offset: 0,
            rendered_items_len,
            hidden: false,
            cancelled: false,
        }
    }

    pub fn update_rendered_items_len(&mut self, rendered_items_len: usize, page_size: usize) {
        self.rendered_items_len = rendered_items_len;
        self.clamp(page_size);
        log::debug!(
            "[selector_core] view rendered length updated: rendered_items_len={}, cursor={}, offset={}",
            self.rendered_items_len,
            self.cursor,
            self.offset
        );
    }

    pub fn selected_index(&self) -> Option<usize> {
        (self.cursor < self.rendered_items_len).then_some(self.cursor)
    }

    fn clamp(&mut self, page_size: usize) {
        if self.rendered_items_len == 0 {
            self.cursor = 0;
            self.offset = 0;
            return;
        }

        let last = self.rendered_items_len - 1;
        self.cursor = self.cursor.min(last);
        self.offset = self.offset.min(last);
        self.keep_cursor_visible(page_size);
    }

    fn keep_cursor_visible(&mut self, page_size: usize) {
        let page_size = page_size.max(1);
        if self.cursor < self.offset {
            self.offset = self.cursor;
        } else if self.cursor >= self.offset.saturating_add(page_size) {
            self.offset = self.cursor.saturating_add(1).saturating_sub(page_size);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectorControllerCommand {
    CursorNext,
    CursorPrevious,
    CursorFirst,
    CursorLast,
    PageDown,
    PageUp,
    Show,
    Hide,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectorController {
    page_size: usize,
}

impl SelectorController {
    pub fn new(page_size: usize) -> Self {
        Self {
            page_size: page_size.max(1),
        }
    }

    pub fn apply(&self, view: &mut SelectorViewState, command: SelectorControllerCommand) {
        log::debug!(
            "[selector_core] apply selector controller command: command={command:?}, cursor_before={}, offset_before={}, rendered_items_len={}",
            view.cursor,
            view.offset,
            view.rendered_items_len
        );

        match command {
            SelectorControllerCommand::CursorNext => {
                if view.rendered_items_len > 0 {
                    view.cursor = (view.cursor + 1).min(view.rendered_items_len - 1);
                    view.keep_cursor_visible(self.page_size);
                }
            }
            SelectorControllerCommand::CursorPrevious => {
                view.cursor = view.cursor.saturating_sub(1);
                view.keep_cursor_visible(self.page_size);
            }
            SelectorControllerCommand::CursorFirst => {
                view.cursor = 0;
                view.offset = 0;
            }
            SelectorControllerCommand::CursorLast => {
                if view.rendered_items_len > 0 {
                    view.cursor = view.rendered_items_len - 1;
                    view.keep_cursor_visible(self.page_size);
                }
            }
            SelectorControllerCommand::PageDown => {
                if view.rendered_items_len > 0 {
                    view.cursor = (view.cursor + self.page_size).min(view.rendered_items_len - 1);
                    view.offset = view.cursor;
                }
            }
            SelectorControllerCommand::PageUp => {
                view.cursor = view.cursor.saturating_sub(self.page_size);
                view.offset = view.cursor;
            }
            SelectorControllerCommand::Show => {
                if !view.cancelled {
                    view.hidden = false;
                }
            }
            SelectorControllerCommand::Hide => {
                view.hidden = true;
            }
            SelectorControllerCommand::Cancel => {
                view.hidden = true;
                view.cancelled = true;
            }
        }

        view.clamp(self.page_size);
        log::debug!(
            "[selector_core] selector controller command applied: command={command:?}, cursor_after={}, offset_after={}, hidden={}, cancelled={}",
            view.cursor,
            view.offset,
            view.hidden,
            view.cancelled
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectorError {
    Cancelled,
    Failed(String),
}

impl fmt::Display for SelectorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SelectorError::Cancelled => write!(f, "selector work cancelled"),
            SelectorError::Failed(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for SelectorError {}

#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn cancel(&self) {
        log::debug!("[selector_core] cancellation requested");
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

pub trait ResultStore<TDetail: Clone> {
    fn append(&mut self, item: SelectorItem<TDetail>) -> Result<(), SelectorError>;
    fn scan(&self, signal: &CancellationToken)
    -> Result<Vec<SelectorItem<TDetail>>, SelectorError>;
    fn get(&self, id: &str) -> Result<Option<SelectorItem<TDetail>>, SelectorError>;
    fn count(&self) -> usize;
    fn status(&self) -> SelectorResultStoreStatus;
    fn dispose(&mut self) -> Result<(), SelectorError>;
}

#[derive(Debug, Clone)]
pub struct InMemoryResultStore<TDetail = ()> {
    items: Vec<SelectorItem<TDetail>>,
    estimated_bytes: usize,
}

impl<TDetail> InMemoryResultStore<TDetail> {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            estimated_bytes: 0,
        }
    }
}

impl<TDetail> Default for InMemoryResultStore<TDetail> {
    fn default() -> Self {
        Self::new()
    }
}

impl<TDetail: Clone> ResultStore<TDetail> for InMemoryResultStore<TDetail> {
    fn append(&mut self, item: SelectorItem<TDetail>) -> Result<(), SelectorError> {
        self.estimated_bytes += item.id.len() + item.value.len() + item.kind.len();
        log::debug!(
            "[selector_core] memory store append: id={}, count_after={}",
            item.id,
            self.items.len() + 1
        );
        self.items.push(item);
        Ok(())
    }

    fn scan(
        &self,
        signal: &CancellationToken,
    ) -> Result<Vec<SelectorItem<TDetail>>, SelectorError> {
        if signal.is_cancelled() {
            log::debug!("[selector_core] memory store scan cancelled before start");
            return Ok(Vec::new());
        }

        let mut scanned = Vec::with_capacity(self.items.len());
        for item in &self.items {
            if signal.is_cancelled() {
                log::debug!(
                    "[selector_core] memory store scan cancelled: scanned={}",
                    scanned.len()
                );
                return Ok(scanned);
            }
            scanned.push(item.clone());
        }
        log::debug!(
            "[selector_core] memory store scan completed: scanned={}",
            scanned.len()
        );
        Ok(scanned)
    }

    fn get(&self, id: &str) -> Result<Option<SelectorItem<TDetail>>, SelectorError> {
        Ok(self.items.iter().rev().find(|item| item.id == id).cloned())
    }

    fn count(&self) -> usize {
        self.items.len()
    }

    fn status(&self) -> SelectorResultStoreStatus {
        SelectorResultStoreStatus {
            storage: SelectorStorageMode::Memory,
            total_stored: self.items.len(),
            estimated_bytes: Some(self.estimated_bytes),
            temp_file_path: None,
        }
    }

    fn dispose(&mut self) -> Result<(), SelectorError> {
        log::debug!(
            "[selector_core] memory store dispose: count_before={}",
            self.items.len()
        );
        self.items.clear();
        self.estimated_bytes = 0;
        Ok(())
    }
}

pub trait SelectorMatcher<TDetail: Clone> {
    fn match_item(
        &self,
        item: &SelectorItem<TDetail>,
        query: &SelectorQuery,
    ) -> Option<MatchedItem<TDetail>>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct PrefixAndMatcher;

#[derive(Debug, Default, Clone, Copy)]
pub struct SubstringAndMatcher;

#[derive(Debug, Default, Clone, Copy)]
pub struct SuffixAndMatcher;

impl<TDetail: Clone> SelectorMatcher<TDetail> for PrefixAndMatcher {
    fn match_item(
        &self,
        item: &SelectorItem<TDetail>,
        query: &SelectorQuery,
    ) -> Option<MatchedItem<TDetail>> {
        token_match(item, query, TokenMatchMode::Prefix)
    }
}

impl PrefixAndMatcher {
    pub fn match_item<TDetail: Clone>(
        &self,
        item: &SelectorItem<TDetail>,
        query: &SelectorQuery,
    ) -> Option<MatchedItem<TDetail>> {
        <Self as SelectorMatcher<TDetail>>::match_item(self, item, query)
    }
}

impl<TDetail: Clone> SelectorMatcher<TDetail> for SubstringAndMatcher {
    fn match_item(
        &self,
        item: &SelectorItem<TDetail>,
        query: &SelectorQuery,
    ) -> Option<MatchedItem<TDetail>> {
        value_match(item, query)
    }
}

impl SubstringAndMatcher {
    pub fn match_item<TDetail: Clone>(
        &self,
        item: &SelectorItem<TDetail>,
        query: &SelectorQuery,
    ) -> Option<MatchedItem<TDetail>> {
        <Self as SelectorMatcher<TDetail>>::match_item(self, item, query)
    }
}

impl<TDetail: Clone> SelectorMatcher<TDetail> for SuffixAndMatcher {
    fn match_item(
        &self,
        item: &SelectorItem<TDetail>,
        query: &SelectorQuery,
    ) -> Option<MatchedItem<TDetail>> {
        token_match(item, query, TokenMatchMode::Suffix)
    }
}

impl SuffixAndMatcher {
    pub fn match_item<TDetail: Clone>(
        &self,
        item: &SelectorItem<TDetail>,
        query: &SelectorQuery,
    ) -> Option<MatchedItem<TDetail>> {
        <Self as SelectorMatcher<TDetail>>::match_item(self, item, query)
    }
}

#[derive(Debug, Clone, Copy)]
enum TokenMatchMode {
    Prefix,
    Suffix,
}

fn value_match<TDetail: Clone>(
    item: &SelectorItem<TDetail>,
    query: &SelectorQuery,
) -> Option<MatchedItem<TDetail>> {
    let mut highlights = Vec::new();
    for term in query.terms() {
        let column = item.value.find(term)?;
        highlights.push(SelectorHighlight {
            column,
            width: term.len(),
            kind: HighlightKind::Match,
        });
    }
    Some(matched_item(item, highlights))
}

fn token_match<TDetail: Clone>(
    item: &SelectorItem<TDetail>,
    query: &SelectorQuery,
    mode: TokenMatchMode,
) -> Option<MatchedItem<TDetail>> {
    let tokens = ascii_space_tokens(&item.value);
    let mut highlights = Vec::new();
    for term in query.terms() {
        let highlight = tokens.iter().find_map(|token| match mode {
            TokenMatchMode::Prefix if token.value.starts_with(term) => Some(SelectorHighlight {
                column: token.start,
                width: term.len(),
                kind: HighlightKind::Match,
            }),
            TokenMatchMode::Suffix if token.value.ends_with(term) => Some(SelectorHighlight {
                column: token.end - term.len(),
                width: term.len(),
                kind: HighlightKind::Match,
            }),
            _ => None,
        })?;
        highlights.push(highlight);
    }
    Some(matched_item(item, highlights))
}

fn matched_item<TDetail: Clone>(
    item: &SelectorItem<TDetail>,
    highlights: Vec<SelectorHighlight>,
) -> MatchedItem<TDetail> {
    let score = Some(
        highlights
            .iter()
            .map(|highlight| highlight.width as i64)
            .sum(),
    );
    MatchedItem {
        item: item.clone(),
        score,
        highlights,
    }
}

#[derive(Debug, Clone, Copy)]
struct ValueToken<'a> {
    value: &'a str,
    start: usize,
    end: usize,
}

fn ascii_space_tokens(value: &str) -> Vec<ValueToken<'_>> {
    let mut tokens = Vec::new();
    let mut start = None;
    for (index, byte) in value.bytes().enumerate() {
        if byte == b' ' {
            if let Some(token_start) = start.take() {
                tokens.push(ValueToken {
                    value: &value[token_start..index],
                    start: token_start,
                    end: index,
                });
            }
        } else if start.is_none() {
            start = Some(index);
        }
    }
    if let Some(token_start) = start {
        tokens.push(ValueToken {
            value: &value[token_start..],
            start: token_start,
            end: value.len(),
        });
    }
    tokens
}

pub trait SelectorRenderer<TDetail: Clone> {
    fn render(&self, item: &MatchedItem<TDetail>) -> Result<RenderedItem, SelectorError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultRenderer;

impl<TDetail: Clone> SelectorRenderer<TDetail> for DefaultRenderer {
    fn render(&self, item: &MatchedItem<TDetail>) -> Result<RenderedItem, SelectorError> {
        Ok(RenderedItem {
            id: item.item.id.clone(),
            label: item.item.value.clone(),
            highlights: item.highlights.clone(),
        })
    }
}

pub trait SelectorPreviewer<TDetail: Clone> {
    fn preview(
        &self,
        item: &SelectorItem<TDetail>,
    ) -> Result<Option<PreviewContent>, SelectorError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoopPreviewer;

impl<TDetail: Clone> SelectorPreviewer<TDetail> for NoopPreviewer {
    fn preview(
        &self,
        _item: &SelectorItem<TDetail>,
    ) -> Result<Option<PreviewContent>, SelectorError> {
        Ok(None)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectOutput {
    pub status: SelectorCollectStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchOutput<TDetail = ()> {
    pub items: Vec<MatchedItem<TDetail>>,
    pub status: SelectorMatchStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderOutput {
    pub items: Vec<RenderedItem>,
    pub status: SelectorMatchStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewOutput {
    pub content: Option<PreviewContent>,
    pub status: SelectorPreviewStatus,
}

#[derive(Debug, Default)]
pub struct CollectProcessor {
    status: Mutex<Option<SelectorCollectStatus>>,
}

impl CollectProcessor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn collect_items<TDetail: Clone, S: ResultStore<TDetail>>(
        &self,
        store: &mut S,
        items: impl IntoIterator<Item = SelectorItem<TDetail>>,
        signal: &CancellationToken,
    ) -> Result<CollectOutput, SelectorError> {
        let mut status = SelectorCollectStatus {
            state: WorkState::Running,
            total_seen: 0,
            total_stored: store.count(),
            storage: store.status().storage,
            error_message: None,
        };
        self.set_collect_status(status.clone());
        log::debug!(
            "[selector_core] collect started: stored_before={}",
            status.total_stored
        );

        for item in items {
            if signal.is_cancelled() {
                status.state = WorkState::Cancelled;
                self.set_collect_status(status.clone());
                log::debug!(
                    "[selector_core] collect cancelled: seen={}, stored={}",
                    status.total_seen,
                    status.total_stored
                );
                return Ok(CollectOutput { status });
            }
            status.total_seen += 1;
            if let Err(error) = store.append(item) {
                status.state = WorkState::Failed;
                status.error_message = Some(error.to_string());
                self.set_collect_status(status.clone());
                return Err(error);
            }
            let store_status = store.status();
            status.total_stored = store_status.total_stored;
            status.storage = store_status.storage;
        }

        status.state = WorkState::Completed;
        self.set_collect_status(status.clone());
        log::debug!(
            "[selector_core] collect completed: seen={}, stored={}",
            status.total_seen,
            status.total_stored
        );
        Ok(CollectOutput { status })
    }

    pub fn status(&self) -> Option<SelectorCollectStatus> {
        self.status.lock().expect("collect status poisoned").clone()
    }

    fn set_collect_status(&self, status: SelectorCollectStatus) {
        *self.status.lock().expect("collect status poisoned") = Some(status);
    }
}

#[derive(Debug, Default)]
pub struct MatchProcessor {
    status: Mutex<Option<SelectorMatchStatus>>,
}

impl MatchProcessor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn match_store<TDetail: Clone, S: ResultStore<TDetail>, M: SelectorMatcher<TDetail>>(
        &self,
        store: &S,
        matcher: &M,
        query: &str,
        signal: &CancellationToken,
    ) -> Result<MatchOutput<TDetail>, SelectorError> {
        let mut status = SelectorMatchStatus {
            state: WorkState::Running,
            total_matched: 0,
            total_rendered: 0,
            error_message: None,
        };
        self.set_match_status(status.clone());
        log::debug!(
            "[selector_core] match started: query_len={}, store_count={}",
            query.len(),
            store.count()
        );

        if signal.is_cancelled() {
            status.state = WorkState::Cancelled;
            self.set_match_status(status.clone());
            log::debug!("[selector_core] match cancelled before scan");
            return Ok(MatchOutput {
                items: Vec::new(),
                status,
            });
        }

        let parsed = parse_ascii_space_and_query(query);
        let scanned = match store.scan(signal) {
            Ok(items) => items,
            Err(error) => {
                status.state = WorkState::Failed;
                status.error_message = Some(error.to_string());
                self.set_match_status(status.clone());
                return Err(error);
            }
        };
        let mut items = Vec::new();
        for item in scanned {
            if signal.is_cancelled() {
                status.state = WorkState::Cancelled;
                status.total_matched = items.len();
                self.set_match_status(status.clone());
                log::debug!(
                    "[selector_core] match cancelled while scanning: matched={}",
                    status.total_matched
                );
                return Ok(MatchOutput { items, status });
            }
            if let Some(matched) = matcher.match_item(&item, &parsed) {
                items.push(matched);
            }
        }

        status.state = WorkState::Completed;
        status.total_matched = items.len();
        self.set_match_status(status.clone());
        log::debug!(
            "[selector_core] match completed: matched={}, rendered={}",
            status.total_matched,
            status.total_rendered
        );
        Ok(MatchOutput { items, status })
    }

    pub fn status(&self) -> Option<SelectorMatchStatus> {
        self.status.lock().expect("match status poisoned").clone()
    }

    fn set_match_status(&self, status: SelectorMatchStatus) {
        *self.status.lock().expect("match status poisoned") = Some(status);
    }
}

#[derive(Debug)]
pub struct RenderProcessor {
    limits: SelectorLimits,
    status: Mutex<Option<SelectorMatchStatus>>,
}

impl RenderProcessor {
    pub fn new(limits: SelectorLimits) -> Self {
        Self {
            limits,
            status: Mutex::new(None),
        }
    }

    pub fn render<TDetail: Clone, R: SelectorRenderer<TDetail>>(
        &self,
        matched_items: &[MatchedItem<TDetail>],
        renderer: &R,
        signal: &CancellationToken,
    ) -> Result<RenderOutput, SelectorError> {
        let mut status = SelectorMatchStatus {
            state: WorkState::Running,
            total_matched: matched_items.len(),
            total_rendered: 0,
            error_message: None,
        };
        self.set_render_status(status.clone());
        log::debug!(
            "[selector_core] render started: matched={}, max_rendered={}",
            matched_items.len(),
            self.limits.max_rendered_items
        );

        let mut rendered = Vec::new();
        for item in matched_items.iter().take(self.limits.max_rendered_items) {
            if signal.is_cancelled() {
                status.state = WorkState::Cancelled;
                status.total_rendered = rendered.len();
                self.set_render_status(status.clone());
                log::debug!(
                    "[selector_core] render cancelled: rendered={}",
                    status.total_rendered
                );
                return Ok(RenderOutput {
                    items: rendered,
                    status,
                });
            }
            match renderer.render(item) {
                Ok(item) => rendered.push(item),
                Err(error) => {
                    status.state = WorkState::Failed;
                    status.error_message = Some(error.to_string());
                    self.set_render_status(status.clone());
                    return Err(error);
                }
            }
        }

        status.state = WorkState::Completed;
        status.total_rendered = rendered.len();
        self.set_render_status(status.clone());
        log::debug!(
            "[selector_core] render completed: rendered={}, matched={}",
            status.total_rendered,
            status.total_matched
        );
        Ok(RenderOutput {
            items: rendered,
            status,
        })
    }

    pub fn status(&self) -> Option<SelectorMatchStatus> {
        self.status.lock().expect("render status poisoned").clone()
    }

    fn set_render_status(&self, status: SelectorMatchStatus) {
        *self.status.lock().expect("render status poisoned") = Some(status);
    }
}

#[derive(Debug, Default)]
pub struct PreviewProcessor {
    status: Mutex<Option<SelectorPreviewStatus>>,
}

impl PreviewProcessor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn preview<TDetail: Clone, P: SelectorPreviewer<TDetail>>(
        &self,
        item: &SelectorItem<TDetail>,
        previewer: &P,
        signal: &CancellationToken,
    ) -> Result<PreviewOutput, SelectorError> {
        let mut status = SelectorPreviewStatus {
            state: WorkState::Running,
            item_id: Some(item.id.clone()),
            error_message: None,
        };
        self.set_preview_status(status.clone());
        log::debug!("[selector_core] preview started: item_id={}", item.id);

        if signal.is_cancelled() {
            status.state = WorkState::Cancelled;
            self.set_preview_status(status.clone());
            log::debug!("[selector_core] preview cancelled before previewer call");
            return Ok(PreviewOutput {
                content: None,
                status,
            });
        }

        match previewer.preview(item) {
            Ok(content) => {
                status.state = if signal.is_cancelled() {
                    WorkState::Cancelled
                } else {
                    WorkState::Completed
                };
                self.set_preview_status(status.clone());
                log::debug!(
                    "[selector_core] preview finished: item_id={}, state={:?}",
                    item.id,
                    status.state
                );
                Ok(PreviewOutput { content, status })
            }
            Err(error) => {
                status.state = WorkState::Failed;
                status.error_message = Some(error.to_string());
                self.set_preview_status(status.clone());
                Err(error)
            }
        }
    }

    pub fn status(&self) -> Option<SelectorPreviewStatus> {
        self.status.lock().expect("preview status poisoned").clone()
    }

    fn set_preview_status(&self, status: SelectorPreviewStatus) {
        *self.status.lock().expect("preview status poisoned") = Some(status);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectorWorkId(u64);

#[derive(Debug, Default)]
pub struct SelectorSessionCore {
    active_work: AtomicU64,
}

impl SelectorSessionCore {
    pub fn new() -> Self {
        Self {
            active_work: AtomicU64::new(0),
        }
    }

    pub fn begin_match_work(&mut self) -> SelectorWorkId {
        let id = self.active_work.fetch_add(1, Ordering::SeqCst) + 1;
        log::debug!("[selector_core] begin match work: work_id={id}");
        SelectorWorkId(id)
    }

    pub fn cancel_active_work(&mut self) {
        let id = self.active_work.fetch_add(1, Ordering::SeqCst) + 1;
        log::debug!("[selector_core] active work cancelled: next_work_id={id}");
    }

    pub fn is_active_work(&self, work_id: SelectorWorkId) -> bool {
        self.active_work.load(Ordering::SeqCst) == work_id.0
    }
}
