use std::collections::{BTreeMap, BTreeSet};

use crate::core::bridge::CoreBridge;
use crate::features::search::capability::SearchCapabilityContract;
use crate::features::search::query::{SearchStateError, SearchVisibleQuery, SearchVisibleState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchModeHint {
    Idle,
    Hlsearch,
    Incsearch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchRefreshInput {
    pub window_id: i32,
    pub revision: u64,
    pub viewport_top: usize,
    pub viewport_height: usize,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub prompt_revision: Option<u64>,
    pub search_mode_hint: SearchModeHint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchRenderCacheKey {
    pub window_id: i32,
    pub revision: u64,
    pub viewport_top: usize,
    pub viewport_height: usize,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub prompt_revision: Option<u64>,
    pub search_mode_hint: SearchModeHint,
    pub live_state_query_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchRefreshOutcome {
    pub cache_key: SearchRenderCacheKey,
    pub render_state: Option<SearchVisibleState>,
    pub query_error: Option<SearchStateError>,
    pub query_executed: bool,
    pub capability: SearchCapabilityContract,
}

pub trait SearchRefreshQueryBackend {
    fn search_capability_contract(&self) -> SearchCapabilityContract;
    fn query_visible_search_state(
        &mut self,
        query: SearchVisibleQuery,
    ) -> Result<SearchVisibleState, SearchStateError>;
    fn query_visible_search_state_for_window(
        &mut self,
        window_id: i32,
        query: SearchVisibleQuery,
    ) -> Result<SearchVisibleState, SearchStateError>;
}

impl SearchRefreshQueryBackend for CoreBridge {
    fn search_capability_contract(&self) -> SearchCapabilityContract {
        CoreBridge::search_capability_contract(self)
    }

    fn query_visible_search_state(
        &mut self,
        query: SearchVisibleQuery,
    ) -> Result<SearchVisibleState, SearchStateError> {
        CoreBridge::query_visible_search_state(self, query)
    }

    fn query_visible_search_state_for_window(
        &mut self,
        window_id: i32,
        query: SearchVisibleQuery,
    ) -> Result<SearchVisibleState, SearchStateError> {
        CoreBridge::query_visible_search_state_for_window(self, window_id, query)
    }
}

#[derive(Debug, Default)]
pub struct SearchRefreshCoordinator {
    last_cache_key: Option<SearchRenderCacheKey>,
    last_render_state: Option<SearchVisibleState>,
}

impl SearchRefreshCoordinator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update<B>(&mut self, backend: &mut B, input: SearchRefreshInput) -> SearchRefreshOutcome
    where
        B: SearchRefreshQueryBackend,
    {
        let capability_contract = backend.search_capability_contract();
        let cache_key = SearchRenderCacheKey {
            window_id: input.window_id,
            revision: input.revision,
            viewport_top: input.viewport_top,
            viewport_height: input.viewport_height,
            cursor_row: input.cursor_row,
            cursor_col: input.cursor_col,
            prompt_revision: input.prompt_revision,
            search_mode_hint: input.search_mode_hint,
            live_state_query_available: capability_contract.live_state_query_available,
        };

        log::debug!(
            "[search_refresh] update requested: key={:?}, previous_key={:?}",
            cache_key,
            self.last_cache_key
        );

        if matches!(input.search_mode_hint, SearchModeHint::Idle) {
            self.last_cache_key = Some(cache_key);
            self.last_render_state = None;
            log::debug!("[search_refresh] idle search mode clears render state without query");
            return SearchRefreshOutcome {
                cache_key,
                render_state: None,
                query_error: None,
                query_executed: false,
                capability: capability_contract,
            };
        }

        if self.last_cache_key == Some(cache_key) {
            log::debug!("[search_refresh] cache hit for search query; reusing last render state");
            return SearchRefreshOutcome {
                cache_key,
                render_state: self.last_render_state.clone(),
                query_error: None,
                query_executed: false,
                capability: capability_contract,
            };
        }

        let query = SearchVisibleQuery {
            start_row: input.viewport_top + 1,
            end_row: input.viewport_top + input.viewport_height.max(1),
        };
        match backend.query_visible_search_state_for_window(input.window_id, query) {
            Ok(render_state) => {
                log::debug!(
                    "[search_refresh] query executed successfully: key={:?}, mode={:?}, matches={}, incsearch_active={}, input_pattern={:?}",
                    cache_key,
                    render_state.mode,
                    render_state.matches.len(),
                    render_state.incsearch_active,
                    render_state.input_pattern
                );
                self.last_cache_key = Some(cache_key);
                self.last_render_state = Some(render_state.clone());
                SearchRefreshOutcome {
                    cache_key,
                    render_state: Some(render_state),
                    query_error: None,
                    query_executed: true,
                    capability: capability_contract,
                }
            }
            Err(error) => {
                log::debug!(
                    "[search_refresh] query failed and render state cleared: key={:?}, error={:?}",
                    cache_key,
                    error
                );
                self.last_cache_key = Some(cache_key);
                self.last_render_state = None;
                SearchRefreshOutcome {
                    cache_key,
                    render_state: None,
                    query_error: Some(error),
                    query_executed: true,
                    capability: capability_contract,
                }
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct WindowSearchRefreshStore {
    coordinators: BTreeMap<i32, SearchRefreshCoordinator>,
}

impl WindowSearchRefreshStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update_window<B>(
        &mut self,
        backend: &mut B,
        input: SearchRefreshInput,
    ) -> SearchRefreshOutcome
    where
        B: SearchRefreshQueryBackend,
    {
        self.coordinators
            .entry(input.window_id)
            .or_default()
            .update(backend, input)
    }

    pub fn retain_windows(&mut self, window_ids: &[i32]) {
        let live_ids = window_ids.iter().copied().collect::<BTreeSet<_>>();
        self.coordinators
            .retain(|window_id, _| live_ids.contains(window_id));
    }
}

#[cfg(test)]
#[path = "refresh_test.rs"]
mod tests;
