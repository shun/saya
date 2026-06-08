use std::env;
use std::fs;
use std::path::Path;

#[test]
fn native_source_audit_report_is_traceable() {
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR should be set during cargo test");
    let report_path = Path::new(&out_dir).join("native-source-audit-report.txt");

    assert!(
        report_path.exists(),
        "native-source-audit-report.txt should exist"
    );
    let content = fs::read_to_string(&report_path).expect("report should be readable");
    assert!(content.contains("native source audit report"));
    assert!(
        content.contains("status: passed"),
        "actual native/ should pass audit"
    );
}

#[test]
fn archive_member_audit_report_is_traceable() {
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR should be set during cargo test");
    let report_path = Path::new(&out_dir).join("archive-member-audit-report.txt");

    assert!(
        report_path.exists(),
        "archive-member-audit-report.txt should exist"
    );
    let content = fs::read_to_string(&report_path).expect("report should be readable");
    assert!(content.contains("archive member audit report"));
    assert!(
        content.contains("status: passed"),
        "actual archive should pass member audit"
    );
}

#[test]
fn normal_delegation_proof_is_traceable() {
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR should be set during cargo test");
    let report_path = Path::new(&out_dir).join("normal-delegation-proof.txt");

    assert!(
        report_path.exists(),
        "normal-delegation-proof.txt should exist"
    );
    let content = fs::read_to_string(&report_path).expect("report should be readable");
    assert!(content.contains("normal delegation proof report"));
    assert!(
        content.contains("status: passed"),
        "actual archive should pass normal delegation proof"
    );
}

#[test]
fn ex_delegation_proof_is_traceable() {
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR should be set during cargo test");
    let report_path = Path::new(&out_dir).join("ex-delegation-proof.txt");

    assert!(report_path.exists(), "ex-delegation-proof.txt should exist");
    let content = fs::read_to_string(&report_path).expect("report should be readable");
    assert!(content.contains("ex delegation proof report"));
    assert!(
        content.contains("status: passed"),
        "actual archive should pass ex delegation proof"
    );
}

#[test]
fn compile_proof_is_traceable() {
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR should be set during cargo test");
    let proof_path = Path::new(&out_dir).join("upstream_build_fingerprint.json");

    assert!(
        proof_path.exists(),
        "upstream_build_fingerprint.json should exist"
    );
    let content = fs::read_to_string(&proof_path).expect("proof should be readable");

    let proof: serde_json::Value =
        serde_json::from_str(&content).expect("proof should be valid JSON");
    assert!(proof.get("tag").is_some());
    assert!(proof.get("commit").is_some());
    assert!(proof.get("native_sources").is_some());
    assert!(proof.get("vendor_sources").is_some());
}

#[test]
fn vim_license_text_is_shipped_in_repository_root() {
    let license_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("LICENSE-vim");
    assert!(
        license_path.exists(),
        "LICENSE-vim should exist in the repository root"
    );

    let content = fs::read_to_string(&license_path).expect("LICENSE-vim should be readable");
    assert!(
        content.contains("VIM LICENSE"),
        "LICENSE-vim should include the Vim license text"
    );
}

#[test]
fn root_license_describes_apache_and_vim_split() {
    let license_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("LICENSE");
    assert!(
        license_path.exists(),
        "LICENSE should exist in the repository root"
    );

    let content = fs::read_to_string(&license_path).expect("LICENSE should be readable");
    assert!(
        content.contains("Apache License, Version 2.0"),
        "LICENSE should include the Apache 2.0 text for original vim-core-rs code"
    );
    assert!(
        content.contains("LICENSE-vim"),
        "LICENSE should point readers to the bundled Vim license text"
    );
    assert!(
        content.contains("portions of upstream Vim under the Vim"),
        "LICENSE should disclose that vendored Vim code remains under the Vim License"
    );
}

#[test]
fn third_party_notice_describes_modified_vim_distribution() {
    let notice_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("THIRD_PARTY_NOTICES.md");
    assert!(
        notice_path.exists(),
        "THIRD_PARTY_NOTICES.md should exist in the repository root"
    );

    let content =
        fs::read_to_string(&notice_path).expect("THIRD_PARTY_NOTICES.md should be readable");
    assert!(
        content.contains("vendors and modifies portions of Vim")
            && content.contains("modified Vim distribution"),
        "third-party notice should disclose that this repository distributes a modified Vim"
    );
    assert!(
        content.contains("LICENSE-vim"),
        "third-party notice should point readers to the bundled Vim license text"
    );
    assert!(
        content.contains("https://github.com/shun/vim-core-rs/issues"),
        "third-party notice should include contact information for the modified Vim distribution"
    );
}

#[test]
fn cargo_manifest_uses_license_file_for_mixed_licensing() {
    let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let content = fs::read_to_string(&manifest_path).expect("Cargo.toml should be readable");

    assert!(
        content.contains("license-file = \"LICENSE\""),
        "Cargo.toml should point to the repository license file for mixed licensing"
    );
    assert!(
        !content.contains("\nlicense = "),
        "Cargo.toml should avoid a single SPDX license field for this mixed-license package"
    );
}

#[test]
fn rendering_state_family_boundary_is_documented_and_classified() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let scope = fs::read_to_string(repo_root.join("docs/SCOPE.md"))
        .expect("docs/SCOPE.md should be readable");
    let api_contracts = fs::read_to_string(repo_root.join("docs/api-contracts.md"))
        .expect("docs/api-contracts.md should be readable");
    let classification_doc =
        fs::read_to_string(repo_root.join("docs/upstream-test-classification.md"))
            .expect("docs/upstream-test-classification.md should be readable");
    let manifest = fs::read_to_string(repo_root.join("upstream-test-classification.json"))
        .expect("upstream-test-classification.json should be readable");

    assert!(
        scope.contains("Rendering State Family")
            && scope.contains("authoritative source")
            && scope.contains("Vim-owned read-only extraction boundary")
            && scope.contains("current member")
            && scope.contains("deferred placeholder")
            && scope.contains("exclusion")
            && !scope.contains("issue #14")
            && scope.contains("overlay composition")
            && scope.contains("popup window")
            && scope.contains("resolved highlight attribute tables"),
        "scope should fix the final authority vocabulary and phase boundary"
    );
    assert!(
        api_contracts.contains("Rendering State Family")
            && api_contracts.contains("authoritative source")
            && api_contracts.contains("Vim-owned read-only extraction boundary")
            && api_contracts.contains("current member")
            && api_contracts.contains("deferred placeholder")
            && api_contracts.contains("exclusion")
            && !api_contracts.contains("issue #14")
            && api_contracts.contains("popup placement")
            && api_contracts.contains("overlay layout")
            && api_contracts.contains("resolved highlight attribute tables")
            && (api_contracts.contains("no new family descriptor")
                || api_contracts.contains("does not add a new family descriptor")
                || api_contracts.contains("new family descriptor is not added")),
        "api contracts should map the final family boundary to existing extraction APIs without a new descriptor"
    );
    assert!(
        classification_doc.contains("`popupwin` rendering stays host-owned")
            && classification_doc.contains("`textprop` remains Vim-owned annotation state")
            && classification_doc.contains("Vim-owned read-only extraction boundary")
            && classification_doc.contains("current member")
            && classification_doc.contains("deferred placeholder")
            && classification_doc.contains("exclusion")
            && !classification_doc.contains("issue #14")
            && classification_doc.contains("`:highlight` definition tables")
            && classification_doc.contains("resolved attribute tables"),
        "classification doc should explain the popupwin exclusion, textprop deferment, and highlight-table exclusion with final authority wording"
    );
    assert!(
        manifest.contains("\"id\": \"test_textprop\"")
            && manifest.contains("\"id\": \"test_popupwin\"")
            && manifest.contains("\"id\": \"test_popupwin_textprop\"")
            && manifest.contains("\"id\": \"test_highlight\"")
            && manifest.contains("deferred annotation-state extraction")
            && manifest.contains("host-owned presentation")
            && manifest.contains("popup placement")
            && manifest.contains("resolved attribute tables"),
        "classification manifest should preserve the family boundary rationales for popupwin, textprop, and highlight tables"
    );
}

#[test]
fn incsearch_search_family_contract_is_documented_in_classification_metadata() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let classification_doc =
        fs::read_to_string(repo_root.join("docs/upstream-test-classification.md"))
            .expect("docs/upstream-test-classification.md should be readable");
    let manifest = fs::read_to_string(repo_root.join("upstream-test-classification.json"))
        .expect("upstream-test-classification.json should be readable");

    assert!(
        classification_doc.contains("Search family")
            && classification_doc.contains("incsearch")
            && classification_doc.contains("inactive window")
            && classification_doc.contains("byte columns"),
        "classification doc should describe incsearch as part of the Search family contract boundary"
    );
    assert!(
        manifest.contains("\"name\": \"test_search.vim\"")
            && manifest.contains("\"name\": \"test_search_stat.vim\"")
            && manifest.contains("\"name\": \"test_searchpos.vim\"")
            && manifest.contains("incsearch_contract.rs")
            && manifest.contains("Search family")
            && manifest.contains("inactive window")
            && manifest.contains("byte columns"),
        "classification manifest should map search cases to incsearch contract coverage and Search family boundary terms"
    );
}

#[test]
fn filesystem_environment_promotion_boundary_is_documented_in_traceability_outputs() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let classification_doc =
        fs::read_to_string(repo_root.join("docs/upstream-test-classification.md"))
            .expect("docs/upstream-test-classification.md should be readable");
    let manifest = fs::read_to_string(repo_root.join("upstream-test-classification.json"))
        .expect("upstream-test-classification.json should be readable");

    assert!(
        classification_doc.contains("Filesystem and environment promotion")
            && classification_doc.contains("test_file_perm.vim")
            && classification_doc.contains("test_file_size.vim")
            && classification_doc.contains("test_filecopy.vim")
            && classification_doc.contains("vfs_contract.rs")
            && classification_doc.contains("test_xdg.vim")
            && classification_doc.contains("runtime_path_contract.rs")
            && classification_doc.contains("integration_contract.rs")
            && classification_doc.contains("broad gate")
            && classification_doc.contains("primary authority")
            && classification_doc.contains("test_filechanged.vim")
            && classification_doc.contains("test_menu.vim")
            && classification_doc.contains("test_shortpathname.vim")
            && classification_doc.contains("test_windows_home.vim")
            && classification_doc.contains("host-owned")
            && classification_doc.contains("out of scope")
            && classification_doc.contains("environment/platform dependent"),
        "classification doc should explain dedicated filesystem/environment promotion and state that integration_contract.rs is only a broad gate, not the primary authority"
    );
    assert!(
        manifest.contains("\"id\": \"test_file_perm\"")
            && manifest.contains("\"contract_suite\": \"vfs_contract.rs\"")
            && manifest.contains("\"id\": \"test_file_size\"")
            && manifest.contains("\"id\": \"test_filecopy\"")
            && manifest.contains("\"id\": \"test_xdg\"")
            && manifest.contains("\"contract_suite\": \"runtime_path_contract.rs\"")
            && manifest.contains("\"id\": \"test_filechanged\"")
            && manifest.contains("\"id\": \"test_menu\"")
            && manifest.contains("\"id\": \"test_shortpathname\"")
            && manifest.contains("\"id\": \"test_windows_home\"")
            && manifest.contains("host-owned coordination")
            && manifest.contains("out-of-scope")
            && manifest.contains("platform-dependent")
            && manifest.contains("environment-dependent"),
        "classification manifest should keep dedicated evidence for promoted cases and shared reclassification vocabulary for deferred cases"
    );
}
