use super::*;

#[test]
fn store_reports_materialization_failure_from_registered_source() {
    let mut store = OverlayAssetStore::default();
    let key = OverlayContentKey::RuntimeRegistered {
        id: "runtime.preview".to_string(),
    };
    store.register_asset(
        key.clone(),
        OverlayAssetSource::Failure {
            message: "encoder crashed".to_string(),
        },
    );

    let error = store
        .materialize(&key)
        .expect_err("failing source should return a materialization error");

    assert!(error.to_string().contains("encoder crashed"));
}
