use crate::features::search::capability::SearchCapabilityContract;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchVisibleQuery {
    pub start_row: usize,
    pub end_row: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchVisibleRows {
    pub start_row: usize,
    pub end_row: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchQueryMode {
    Disabled,
    Hlsearch,
    IncsearchPreview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMatchKind {
    Regular,
    Current,
    Incremental,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchMatch {
    pub kind: SearchMatchKind,
    pub start_row: usize,
    pub start_col: usize,
    pub end_row: usize,
    pub end_col: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchVisibleState {
    pub capability: SearchCapabilityContract,
    pub window_id: i32,
    pub visible_rows: SearchVisibleRows,
    pub mode: SearchQueryMode,
    pub pattern: Option<String>,
    pub input_pattern: Option<String>,
    pub hlsearch_enabled: bool,
    pub hlsearch_suspended: bool,
    pub incsearch_active: bool,
    pub matches: Vec<SearchMatch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchStateError {
    InvalidViewport { start_row: usize, end_row: usize },
    ActiveWindowMissing,
    WindowNotFound { window_id: i32 },
}

impl fmt::Display for SearchStateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SearchStateError::InvalidViewport { start_row, end_row } => {
                write!(
                    f,
                    "invalid viewport: start_row={start_row}, end_row={end_row}"
                )
            }
            SearchStateError::ActiveWindowMissing => write!(f, "active window missing"),
            SearchStateError::WindowNotFound { window_id } => {
                write!(f, "window not found: window_id={window_id}")
            }
        }
    }
}
