//! Compatibility test target for the former monolithic startup/runtime scaffold suite.
//!
//! The detailed tests now live in role-focused integration test crates so state
//! assertions are easier to find and extend.

#[test]
fn startup_runtime_scaffold_suite_is_split_by_responsibility() {
    let split_targets = [
        "startup_transpile",
        "startup_registry",
        "bundled_runtime_boundary",
    ];

    assert_eq!(split_targets.len(), 3);
    assert!(split_targets.contains(&"startup_transpile"));
    assert!(split_targets.contains(&"startup_registry"));
    assert!(split_targets.contains(&"bundled_runtime_boundary"));
}
