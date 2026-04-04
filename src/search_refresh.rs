use std::collections::{BTreeMap, BTreeSet};

use crate::core_bridge::CoreBridge;
use crate::search_capability::SearchCapabilityContract;
use crate::search_query::{SearchStateError, SearchVisibleQuery, SearchVisibleState};

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
mod tests {
    use std::cell::{Cell, RefCell};

    use super::*;
    use crate::search_query::{SearchMatch, SearchMatchKind, SearchQueryMode, SearchVisibleRows};

    #[derive(Debug)]
    struct FakeBackend {
        contract: SearchCapabilityContract,
        query_count: Cell<usize>,
        window_query_count: Cell<usize>,
        last_window_id: Cell<Option<i32>>,
        last_query: RefCell<Option<SearchVisibleQuery>>,
        response: RefCell<Result<SearchVisibleState, SearchStateError>>,
    }

    impl FakeBackend {
        fn new(contract: SearchCapabilityContract, response: SearchVisibleState) -> Self {
            Self {
                contract,
                query_count: Cell::new(0),
                window_query_count: Cell::new(0),
                last_window_id: Cell::new(None),
                last_query: RefCell::new(None),
                response: RefCell::new(Ok(response)),
            }
        }

        fn query_count(&self) -> usize {
            self.query_count.get()
        }

        fn window_query_count(&self) -> usize {
            self.window_query_count.get()
        }

        fn last_window_id(&self) -> Option<i32> {
            self.last_window_id.get()
        }

        fn last_query(&self) -> Option<SearchVisibleQuery> {
            *self.last_query.borrow()
        }
    }

    impl SearchRefreshQueryBackend for FakeBackend {
        fn search_capability_contract(&self) -> SearchCapabilityContract {
            self.contract
        }

        fn query_visible_search_state(
            &mut self,
            query: SearchVisibleQuery,
        ) -> Result<SearchVisibleState, SearchStateError> {
            self.query_count.set(self.query_count.get() + 1);
            *self.last_query.borrow_mut() = Some(query);
            self.response.borrow().clone()
        }

        fn query_visible_search_state_for_window(
            &mut self,
            window_id: i32,
            query: SearchVisibleQuery,
        ) -> Result<SearchVisibleState, SearchStateError> {
            self.window_query_count
                .set(self.window_query_count.get() + 1);
            self.last_window_id.set(Some(window_id));
            self.query_visible_search_state(query)
        }
    }

    fn sample_state(mode: SearchQueryMode) -> SearchVisibleState {
        SearchVisibleState {
            capability: SearchCapabilityContract::baseline_ready_contract(),
            window_id: 1,
            visible_rows: SearchVisibleRows {
                start_row: 1,
                end_row: 2,
            },
            mode,
            pattern: Some("alpha".to_string()),
            input_pattern: None,
            hlsearch_enabled: true,
            hlsearch_suspended: false,
            incsearch_active: matches!(mode, SearchQueryMode::IncsearchPreview),
            matches: vec![SearchMatch {
                kind: SearchMatchKind::Current,
                start_row: 1,
                start_col: 0,
                end_row: 1,
                end_col: 5,
            }],
        }
    }

    fn sample_input() -> SearchRefreshInput {
        SearchRefreshInput {
            window_id: 1,
            revision: 10,
            viewport_top: 0,
            viewport_height: 2,
            cursor_row: 0,
            cursor_col: 0,
            prompt_revision: None,
            search_mode_hint: SearchModeHint::Hlsearch,
        }
    }

    #[test]
    fn reuses_cached_state_when_refresh_key_is_unchanged() {
        let contract = SearchCapabilityContract::baseline_ready_contract();
        let mut backend = FakeBackend::new(contract, sample_state(SearchQueryMode::Hlsearch));
        let mut coordinator = SearchRefreshCoordinator::new();
        let input = sample_input();

        let first = coordinator.update(&mut backend, input);
        let second = coordinator.update(&mut backend, input);

        assert!(first.query_executed);
        assert!(!second.query_executed);
        assert_eq!(backend.query_count(), 1);
        assert_eq!(second.render_state, first.render_state);
        assert_eq!(second.cache_key, first.cache_key);
    }

    #[test]
    fn invalidates_refresh_when_revision_prompt_or_viewport_changes() {
        let contract = SearchCapabilityContract::baseline_ready_contract();
        let mut backend = FakeBackend::new(contract, sample_state(SearchQueryMode::Hlsearch));
        let mut coordinator = SearchRefreshCoordinator::new();
        let base = SearchRefreshInput {
            prompt_revision: Some(1),
            ..sample_input()
        };

        let _ = coordinator.update(&mut backend, base);
        let revision_changed = SearchRefreshInput {
            revision: 11,
            ..base
        };
        let viewport_changed = SearchRefreshInput {
            viewport_top: 1,
            ..base
        };
        let prompt_changed = SearchRefreshInput {
            prompt_revision: Some(2),
            ..base
        };
        let cursor_changed = SearchRefreshInput {
            cursor_row: 1,
            ..base
        };

        assert!(
            coordinator
                .update(&mut backend, revision_changed)
                .query_executed
        );
        assert!(
            coordinator
                .update(&mut backend, viewport_changed)
                .query_executed
        );
        assert!(
            coordinator
                .update(&mut backend, prompt_changed)
                .query_executed
        );
        assert!(
            coordinator
                .update(&mut backend, cursor_changed)
                .query_executed
        );
        assert_eq!(backend.query_count(), 5);
    }

    #[test]
    fn prompt_active_always_queries_core_owned_live_state() {
        let contract = SearchCapabilityContract::baseline_ready_contract();
        let mut backend =
            FakeBackend::new(contract, sample_state(SearchQueryMode::IncsearchPreview));
        let mut coordinator = SearchRefreshCoordinator::new();
        let input = SearchRefreshInput {
            prompt_revision: Some(9),
            search_mode_hint: SearchModeHint::Incsearch,
            ..sample_input()
        };

        let outcome = coordinator.update(&mut backend, input);

        assert!(outcome.query_executed);
        assert_eq!(backend.query_count(), 1);
        assert_eq!(
            backend.last_query(),
            Some(SearchVisibleQuery {
                start_row: 1,
                end_row: 2,
            })
        );
        assert!(
            outcome
                .render_state
                .as_ref()
                .is_some_and(|state| state.incsearch_active)
        );
    }

    #[test]
    fn idle_mode_clears_cached_render_state_without_query() {
        let contract = SearchCapabilityContract::baseline_ready_contract();
        let mut backend = FakeBackend::new(contract, sample_state(SearchQueryMode::Hlsearch));
        let mut coordinator = SearchRefreshCoordinator::new();
        let active = sample_input();
        let idle = SearchRefreshInput {
            search_mode_hint: SearchModeHint::Idle,
            ..active
        };

        let _ = coordinator.update(&mut backend, active);
        let outcome = coordinator.update(&mut backend, idle);

        assert!(!outcome.query_executed);
        assert!(outcome.render_state.is_none());
        assert!(outcome.query_error.is_none());
        assert_eq!(backend.query_count(), 1);
    }

    #[test]
    fn propagates_window_not_found_and_invalid_viewport_errors() {
        let contract = SearchCapabilityContract::baseline_ready_contract();
        let mut backend = FakeBackend::new(contract, sample_state(SearchQueryMode::Hlsearch));
        let mut coordinator = SearchRefreshCoordinator::new();

        *backend.response.borrow_mut() = Err(SearchStateError::WindowNotFound { window_id: 9 });
        let window_not_found = coordinator.update(&mut backend, sample_input());
        assert_eq!(
            window_not_found.query_error,
            Some(SearchStateError::WindowNotFound { window_id: 9 })
        );
        assert!(window_not_found.render_state.is_none());

        *backend.response.borrow_mut() = Err(SearchStateError::InvalidViewport {
            start_row: 0,
            end_row: 0,
        });
        let invalid_viewport = coordinator.update(
            &mut backend,
            SearchRefreshInput {
                revision: 11,
                ..sample_input()
            },
        );
        assert_eq!(
            invalid_viewport.query_error,
            Some(SearchStateError::InvalidViewport {
                start_row: 0,
                end_row: 0,
            })
        );
        assert!(invalid_viewport.render_state.is_none());
    }

    #[test]
    fn window_search_refresh_store_keeps_cache_separate_per_window() {
        let contract = SearchCapabilityContract::baseline_ready_contract();
        let mut backend = FakeBackend::new(contract, sample_state(SearchQueryMode::Hlsearch));
        let mut store = WindowSearchRefreshStore::new();

        let first = store.update_window(&mut backend, sample_input());
        let second = store.update_window(
            &mut backend,
            SearchRefreshInput {
                window_id: 2,
                ..sample_input()
            },
        );

        assert!(first.query_executed);
        assert!(second.query_executed);
        assert_eq!(backend.query_count(), 2);
        assert_eq!(backend.window_query_count(), 2);
        assert_eq!(backend.last_window_id(), Some(2));
    }
}
