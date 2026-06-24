use super::SearchCapabilityContract;

#[test]
fn baseline_contract_exposes_live_state_query_and_column_semantics() {
    let contract = SearchCapabilityContract::baseline_ready_contract();

    assert!(contract.is_baseline_ready());
    assert!(contract.live_state_query_available);
    assert!(contract.visible_rows_only);
    assert!(contract.start_col_inclusive);
    assert!(contract.end_col_exclusive);
}
