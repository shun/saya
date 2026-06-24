#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchCapabilityContract {
    pub live_state_query_available: bool,
    pub visible_rows_only: bool,
    pub start_col_inclusive: bool,
    pub end_col_exclusive: bool,
}

impl SearchCapabilityContract {
    pub const fn baseline_ready_contract() -> Self {
        Self {
            live_state_query_available: true,
            visible_rows_only: true,
            start_col_inclusive: true,
            end_col_exclusive: true,
        }
    }

    pub const fn is_baseline_ready(self) -> bool {
        self.live_state_query_available
            && self.visible_rows_only
            && self.start_col_inclusive
            && self.end_col_exclusive
    }
}

#[cfg(test)]
#[path = "capability_test.rs"]
mod tests;
