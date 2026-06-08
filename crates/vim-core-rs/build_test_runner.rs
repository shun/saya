use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const UPSTREAM_TESTDIR: &str = "vendor/vim_src/src/testdir";
const UPSTREAM_SKIPLIST: &str = "upstream-test-skiplist.txt";
const UPSTREAM_CLASSIFICATION_MANIFEST: &str = "upstream-test-classification.json";
const GENERATED_TESTS_RS: &str = "upstream_vim_tests.rs";
const GENERATED_MANIFEST_JSON: &str = "upstream_test_manifest.json";
const COMPATIBILITY_ADR: &str = "docs/adr/0002-define-compatibility-boundaries.md";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpstreamTestClassification {
    PreserveDirectly,
    PreserveThroughAdaptation,
    OutOfScope,
    TemporarilyExcluded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeneratedSelectionStatus {
    Included,
    ExcludedByPolicy,
    TemporarilyExcluded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageStatus {
    Covered,
    Uncovered,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoverageEvidence {
    pub contract_suite: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratedAdaptedBehavior {
    pub id: String,
    pub upstream_case_name: String,
    pub relative_path: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub upstream_test_cases: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bucket: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related_contract_suites: Vec<String>,
    pub coverage_status: CoverageStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage_evidence: Option<CoverageEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratedUpstreamTestCase {
    pub name: String,
    pub relative_path: String,
    pub classification: UpstreamTestClassification,
    pub selection_status: GeneratedSelectionStatus,
    pub selected_for_generated_runner: bool,
    pub exclusion_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompatibilityBaselineSummary {
    pub boundary: String,
    pub total: usize,
    pub in_scope: usize,
    pub direct: usize,
    pub adapted: usize,
    pub out_of_scope: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdaptationCoverageSummary {
    pub tracking_unit: String,
    pub total_units: usize,
    pub covered_units: usize,
    pub uncovered_units: usize,
    pub runtime_path_total_units: usize,
    pub runtime_path_covered_units: usize,
    pub runtime_path_uncovered_units: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratedUpstreamTestManifest {
    pub compatibility_baseline: CompatibilityBaselineSummary,
    pub adaptation_coverage: AdaptationCoverageSummary,
    pub cases: Vec<GeneratedUpstreamTestCase>,
    pub adapted_behaviors: Vec<GeneratedAdaptedBehavior>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ClassificationManifest {
    counts: ClassificationCounts,
    cases: Vec<ClassificationCase>,
    #[serde(default)]
    adapted_behaviors: Vec<ClassificationAdaptedBehavior>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ClassificationCounts {
    total_cases: usize,
    preserve_directly: usize,
    preserve_through_adaptation: usize,
    out_of_scope: usize,
    temporarily_excluded: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ClassificationCase {
    name: String,
    relative_path: String,
    classification: UpstreamTestClassification,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ClassificationAdaptedBehavior {
    id: String,
    upstream_case_name: String,
    relative_path: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    upstream_test_cases: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bucket: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rationale: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    related_contract_suites: Vec<String>,
    coverage_status: CoverageStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    coverage_evidence: Option<CoverageEvidence>,
}

pub fn generate_upstream_tests(out_dir: &Path) -> Result<(), String> {
    let repo_root = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR")
            .map_err(|error| format!("missing CARGO_MANIFEST_DIR: {error}"))?,
    );
    generate_upstream_tests_from(&repo_root, out_dir)
}

pub fn generate_upstream_tests_from(repo_root: &Path, out_dir: &Path) -> Result<(), String> {
    let test_dir = repo_root.join(UPSTREAM_TESTDIR);
    if !test_dir.exists() {
        let manifest = GeneratedUpstreamTestManifest {
            compatibility_baseline: empty_compatibility_baseline_summary(),
            adaptation_coverage: empty_adaptation_coverage_summary(),
            cases: Vec::new(),
            adapted_behaviors: Vec::new(),
        };
        write_manifest(out_dir, &manifest)?;
        write_generated_runner(out_dir, &manifest)?;
        return Ok(());
    }

    let skiplist_path = repo_root.join(UPSTREAM_SKIPLIST);
    let classification_path = repo_root.join(UPSTREAM_CLASSIFICATION_MANIFEST);
    let skiplist = parse_skiplist(&skiplist_path)?;
    let classification_manifest = parse_classification_manifest(&classification_path)?;
    let compatibility_baseline = build_compatibility_baseline_summary(&classification_manifest)?;
    let adaptation_coverage = build_adaptation_coverage_summary(&classification_manifest)?;
    let manifest = GeneratedUpstreamTestManifest {
        compatibility_baseline,
        adaptation_coverage,
        cases: collect_cases(repo_root, &test_dir, &skiplist, &classification_manifest)?,
        adapted_behaviors: collect_adapted_behaviors(&classification_manifest),
    };

    write_manifest(out_dir, &manifest)?;
    write_generated_runner(out_dir, &manifest)?;
    Ok(())
}

pub fn parse_skiplist(skiplist_path: &Path) -> Result<BTreeMap<String, String>, String> {
    if !skiplist_path.exists() {
        return Ok(BTreeMap::new());
    }

    let content = fs::read_to_string(skiplist_path).map_err(|error| {
        format!(
            "failed to read upstream test skiplist {}: {error}",
            skiplist_path.display()
        )
    })?;

    let mut skiplist = BTreeMap::new();
    for (line_no, raw_line) in content.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let Some((case_name, reason)) = line.split_once('|') else {
            return Err(format!(
                "invalid upstream test skiplist line {} in {}: expected `test_name.vim | reason`",
                line_no + 1,
                skiplist_path.display()
            ));
        };

        let case_name = case_name.trim();
        let reason = reason.trim();
        if case_name.is_empty() || reason.is_empty() {
            return Err(format!(
                "invalid upstream test skiplist line {} in {}: case name and reason must be non-empty",
                line_no + 1,
                skiplist_path.display()
            ));
        }

        skiplist.insert(case_name.to_string(), reason.to_string());
    }

    Ok(skiplist)
}

fn parse_classification_manifest(
    classification_path: &Path,
) -> Result<ClassificationManifest, String> {
    let content = fs::read_to_string(classification_path).map_err(|error| {
        format!(
            "failed to read upstream test classification manifest {}: {error}",
            classification_path.display()
        )
    })?;
    let manifest: ClassificationManifest = serde_json::from_str(&content).map_err(|error| {
        format!(
            "failed to parse upstream test classification manifest {}: {error}",
            classification_path.display()
        )
    })?;

    validate_classification_counts(&manifest, classification_path)?;
    Ok(manifest)
}

fn validate_classification_counts(
    manifest: &ClassificationManifest,
    classification_path: &Path,
) -> Result<(), String> {
    let mut preserve_directly = 0usize;
    let mut preserve_through_adaptation = 0usize;
    let mut out_of_scope = 0usize;
    let mut temporarily_excluded = 0usize;

    for case in &manifest.cases {
        match case.classification {
            UpstreamTestClassification::PreserveDirectly => preserve_directly += 1,
            UpstreamTestClassification::PreserveThroughAdaptation => {
                preserve_through_adaptation += 1;
            }
            UpstreamTestClassification::OutOfScope => out_of_scope += 1,
            UpstreamTestClassification::TemporarilyExcluded => temporarily_excluded += 1,
        }
    }

    let actual = ClassificationCounts {
        total_cases: manifest.cases.len(),
        preserve_directly,
        preserve_through_adaptation,
        out_of_scope,
        temporarily_excluded,
    };

    if manifest.counts != actual {
        return Err(format!(
            "upstream test classification counts in {} do not match case list: expected {:?}, actual {:?}",
            classification_path.display(),
            manifest.counts,
            actual
        ));
    }

    validate_adapted_behaviors(manifest, classification_path)?;

    Ok(())
}

fn validate_adapted_behaviors(
    manifest: &ClassificationManifest,
    classification_path: &Path,
) -> Result<(), String> {
    let mut adapted_files = BTreeMap::new();
    for case in &manifest.cases {
        if case.classification == UpstreamTestClassification::PreserveThroughAdaptation {
            adapted_files.insert(case.name.as_str(), case.relative_path.as_str());
        }
    }

    let mut behavior_ids = BTreeSet::new();
    let mut files_with_behaviors = BTreeSet::new();
    for behavior in &manifest.adapted_behaviors {
        if !behavior_ids.insert(behavior.id.clone()) {
            return Err(format!(
                "adapted behavior manifest in {} contains duplicate id: {}",
                classification_path.display(),
                behavior.id
            ));
        }

        let Some(expected_relative_path) = adapted_files.get(behavior.upstream_case_name.as_str())
        else {
            return Err(format!(
                "adapted behavior {} in {} references non-adapted upstream case {}",
                behavior.id,
                classification_path.display(),
                behavior.upstream_case_name
            ));
        };
        if expected_relative_path != &behavior.relative_path.as_str() {
            return Err(format!(
                "adapted behavior {} in {} has mismatched path for {}: expected {}, got {}",
                behavior.id,
                classification_path.display(),
                behavior.upstream_case_name,
                expected_relative_path,
                behavior.relative_path
            ));
        }

        validate_adapted_behavior(behavior, classification_path)?;
        files_with_behaviors.insert(behavior.upstream_case_name.as_str());
    }

    for case in &manifest.cases {
        if case.classification == UpstreamTestClassification::PreserveThroughAdaptation
            && !files_with_behaviors.contains(case.name.as_str())
        {
            return Err(format!(
                "preserve_through_adaptation case {} in {} must declare at least one adapted behavior",
                case.name,
                classification_path.display()
            ));
        }
    }

    Ok(())
}

fn validate_adapted_behavior(
    behavior: &ClassificationAdaptedBehavior,
    classification_path: &Path,
) -> Result<(), String> {
    if behavior.id.trim().is_empty() {
        return Err(format!(
            "adapted behavior in {} must declare a non-empty id",
            classification_path.display()
        ));
    }

    for test_case in &behavior.upstream_test_cases {
        if test_case.trim().is_empty() {
            return Err(format!(
                "adapted behavior {} in {} must not declare an empty upstream test case name",
                behavior.id,
                classification_path.display()
            ));
        }
    }

    if behavior.related_contract_suites.is_empty() {
        return Err(format!(
            "adapted behavior {} in {} must declare at least one related_contract_suites entry",
            behavior.id,
            classification_path.display()
        ));
    }

    if behavior.coverage_status == CoverageStatus::Covered && behavior.coverage_evidence.is_none() {
        return Err(format!(
            "covered adapted behavior {} in {} must declare coverage_evidence",
            behavior.id,
            classification_path.display()
        ));
    }

    if let Some(evidence) = &behavior.coverage_evidence {
        validate_coverage_evidence(behavior, evidence, classification_path)?;
    }

    Ok(())
}

fn validate_coverage_evidence(
    behavior: &ClassificationAdaptedBehavior,
    evidence: &CoverageEvidence,
    classification_path: &Path,
) -> Result<(), String> {
    let subject_name = &behavior.id;

    if evidence.contract_suite.trim().is_empty() {
        return Err(format!(
            "coverage_evidence for {} in {} must declare a non-empty contract_suite",
            subject_name,
            classification_path.display()
        ));
    }

    let has_locator = evidence
        .test_name
        .as_deref()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
        || evidence
            .evidence_ref
            .as_deref()
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false);

    if !has_locator {
        return Err(format!(
            "coverage_evidence for {} in {} must declare test_name or evidence_ref",
            subject_name,
            classification_path.display()
        ));
    }

    if !behavior
        .related_contract_suites
        .iter()
        .any(|suite| suite == &evidence.contract_suite)
    {
        return Err(format!(
            "coverage_evidence for {} in {} must reference a suite declared in related_contract_suites",
            subject_name,
            classification_path.display()
        ));
    }

    let repo_root = classification_path.parent().ok_or_else(|| {
        format!(
            "failed to resolve repository root from classification manifest path {}",
            classification_path.display()
        )
    })?;
    let suite_path = repo_root.join("tests").join(&evidence.contract_suite);
    if !suite_path.exists() {
        return Err(format!(
            "coverage_evidence for {} in {} references missing contract suite {}",
            subject_name,
            classification_path.display(),
            suite_path.display()
        ));
    }

    if let Some(test_name) = evidence.test_name.as_deref() {
        validate_contract_test_locator(subject_name, test_name, &suite_path, classification_path)?;
    }

    Ok(())
}

fn validate_contract_test_locator(
    subject_name: &str,
    test_name: &str,
    suite_path: &Path,
    classification_path: &Path,
) -> Result<(), String> {
    let suite_source = fs::read_to_string(suite_path).map_err(|error| {
        format!(
            "failed to read contract suite {} while validating coverage_evidence in {}: {error}",
            suite_path.display(),
            classification_path.display()
        )
    })?;

    let needle = format!("fn {test_name}(");
    if !suite_source.lines().any(|line| line.contains(&needle)) {
        return Err(format!(
            "coverage_evidence for {} in {} references missing test {} in {}",
            subject_name,
            classification_path.display(),
            test_name,
            suite_path.display()
        ));
    }

    Ok(())
}

fn build_compatibility_baseline_summary(
    classification_manifest: &ClassificationManifest,
) -> Result<CompatibilityBaselineSummary, String> {
    let mut direct = 0usize;
    let mut adapted = 0usize;
    let mut out_of_scope = 0usize;

    for case in &classification_manifest.cases {
        match case.classification {
            UpstreamTestClassification::PreserveDirectly => direct += 1,
            UpstreamTestClassification::PreserveThroughAdaptation => adapted += 1,
            UpstreamTestClassification::OutOfScope => out_of_scope += 1,
            UpstreamTestClassification::TemporarilyExcluded => {}
        }
    }

    let total = classification_manifest.counts.total_cases;
    let in_scope = direct + adapted;

    if classification_manifest.counts.preserve_directly != direct
        || classification_manifest.counts.preserve_through_adaptation != adapted
        || classification_manifest.counts.out_of_scope != out_of_scope
    {
        return Err(
            "classification manifest counts do not match derived compatibility baseline summary"
                .to_string(),
        );
    }

    Ok(CompatibilityBaselineSummary {
        boundary: "upstream-derived in-scope embedded Vim core coverage; repo-owned contracts are tracked separately".to_string(),
        total,
        in_scope,
        direct,
        adapted,
        out_of_scope,
    })
}

fn build_adaptation_coverage_summary(
    classification_manifest: &ClassificationManifest,
) -> Result<AdaptationCoverageSummary, String> {
    let mut total_units = 0usize;
    let mut covered_units = 0usize;
    let mut uncovered_units = 0usize;
    let mut runtime_path_total_units = 0usize;
    let mut runtime_path_covered_units = 0usize;
    let mut runtime_path_uncovered_units = 0usize;

    for behavior in &classification_manifest.adapted_behaviors {
        total_units += 1;
        match behavior.coverage_status {
            CoverageStatus::Covered => covered_units += 1,
            CoverageStatus::Uncovered => uncovered_units += 1,
        }

        if behavior.bucket.as_deref() == Some("runtime_path") {
            runtime_path_total_units += 1;
            match behavior.coverage_status {
                CoverageStatus::Covered => runtime_path_covered_units += 1,
                CoverageStatus::Uncovered => runtime_path_uncovered_units += 1,
            }
        }
    }

    if total_units != covered_units + uncovered_units {
        return Err("adaptation coverage summary became inconsistent".to_string());
    }

    Ok(AdaptationCoverageSummary {
        tracking_unit: "repo-owned adapted behavior".to_string(),
        total_units,
        covered_units,
        uncovered_units,
        runtime_path_total_units,
        runtime_path_covered_units,
        runtime_path_uncovered_units,
    })
}

fn empty_compatibility_baseline_summary() -> CompatibilityBaselineSummary {
    CompatibilityBaselineSummary {
        boundary: "upstream-derived in-scope embedded Vim core coverage; repo-owned contracts are tracked separately".to_string(),
        total: 0,
        in_scope: 0,
        direct: 0,
        adapted: 0,
        out_of_scope: 0,
    }
}

fn empty_adaptation_coverage_summary() -> AdaptationCoverageSummary {
    AdaptationCoverageSummary {
        tracking_unit: "repo-owned adapted behavior".to_string(),
        total_units: 0,
        covered_units: 0,
        uncovered_units: 0,
        runtime_path_total_units: 0,
        runtime_path_covered_units: 0,
        runtime_path_uncovered_units: 0,
    }
}

fn collect_cases(
    repo_root: &Path,
    test_dir: &Path,
    skiplist: &BTreeMap<String, String>,
    classification_manifest: &ClassificationManifest,
) -> Result<Vec<GeneratedUpstreamTestCase>, String> {
    let vendored_cases = collect_vendored_cases(repo_root, test_dir)?;
    validate_manifest_alignment(&vendored_cases, classification_manifest, skiplist)?;

    let mut cases = Vec::new();
    for case in &classification_manifest.cases {
        let selection = determine_selection(case, skiplist)?;
        cases.push(GeneratedUpstreamTestCase {
            name: case.name.clone(),
            relative_path: case.relative_path.clone(),
            classification: case.classification.clone(),
            selection_status: selection.status,
            selected_for_generated_runner: selection.selected,
            exclusion_reason: selection.reason,
        });
    }

    cases.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(cases)
}

fn collect_adapted_behaviors(
    classification_manifest: &ClassificationManifest,
) -> Vec<GeneratedAdaptedBehavior> {
    let mut behaviors = classification_manifest
        .adapted_behaviors
        .iter()
        .map(|behavior| GeneratedAdaptedBehavior {
            id: behavior.id.clone(),
            upstream_case_name: behavior.upstream_case_name.clone(),
            relative_path: behavior.relative_path.clone(),
            upstream_test_cases: behavior.upstream_test_cases.clone(),
            bucket: behavior.bucket.clone(),
            rationale: behavior.rationale.clone(),
            related_contract_suites: behavior.related_contract_suites.clone(),
            coverage_status: behavior.coverage_status,
            coverage_evidence: behavior.coverage_evidence.clone(),
        })
        .collect::<Vec<_>>();
    behaviors.sort_by(|left, right| left.id.cmp(&right.id));
    behaviors
}

fn collect_vendored_cases(
    repo_root: &Path,
    test_dir: &Path,
) -> Result<BTreeMap<String, String>, String> {
    let mut cases = BTreeMap::new();
    for entry in fs::read_dir(test_dir).map_err(|error| {
        format!(
            "failed to read upstream test directory {}: {error}",
            test_dir.display()
        )
    })? {
        let entry = entry.map_err(|error| {
            format!(
                "failed to enumerate upstream test directory {}: {error}",
                test_dir.display()
            )
        })?;
        let path = entry.path();
        if !entry
            .file_type()
            .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?
            .is_file()
        {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !file_name.starts_with("test_") || !file_name.ends_with(".vim") {
            continue;
        }

        let relative_path = path
            .strip_prefix(repo_root)
            .map_err(|error| {
                format!(
                    "failed to compute relative upstream test path for {}: {error}",
                    path.display()
                )
            })?
            .to_string_lossy()
            .replace('\\', "/");
        cases.insert(file_name.to_string(), relative_path);
    }

    Ok(cases)
}

fn validate_manifest_alignment(
    vendored_cases: &BTreeMap<String, String>,
    classification_manifest: &ClassificationManifest,
    skiplist: &BTreeMap<String, String>,
) -> Result<(), String> {
    let mut manifest_names = BTreeSet::new();
    for case in &classification_manifest.cases {
        if !manifest_names.insert(case.name.clone()) {
            return Err(format!(
                "upstream test classification manifest contains duplicate case entry: {}",
                case.name
            ));
        }

        let Some(vendored_relative_path) = vendored_cases.get(&case.name) else {
            return Err(format!(
                "upstream test classification manifest references non-vendored case: {}",
                case.name
            ));
        };

        if vendored_relative_path != &case.relative_path {
            return Err(format!(
                "upstream test classification manifest has mismatched path for {}: expected {}, got {}",
                case.name, vendored_relative_path, case.relative_path
            ));
        }
    }

    for vendored_name in vendored_cases.keys() {
        if !manifest_names.contains(vendored_name) {
            return Err(format!(
                "vendored upstream test is missing from classification manifest: {}",
                vendored_name
            ));
        }
    }

    for skipped_name in skiplist.keys() {
        let Some(case) = classification_manifest
            .cases
            .iter()
            .find(|case| case.name == *skipped_name)
        else {
            return Err(format!(
                "upstream test skiplist references unknown case: {}",
                skipped_name
            ));
        };

        if case.classification != UpstreamTestClassification::TemporarilyExcluded {
            return Err(format!(
                "upstream test skiplist entry {} must be classified as temporarily_excluded",
                skipped_name
            ));
        }
    }

    Ok(())
}

struct CaseSelection {
    selected: bool,
    status: GeneratedSelectionStatus,
    reason: Option<String>,
}

fn determine_selection(
    case: &ClassificationCase,
    skiplist: &BTreeMap<String, String>,
) -> Result<CaseSelection, String> {
    match case.classification {
        UpstreamTestClassification::PreserveDirectly => Ok(CaseSelection {
            selected: true,
            status: GeneratedSelectionStatus::Included,
            reason: None,
        }),
        UpstreamTestClassification::PreserveThroughAdaptation => Ok(CaseSelection {
            selected: false,
            status: GeneratedSelectionStatus::ExcludedByPolicy,
            reason: Some(
                "Covered by repository contract tests for adapted host-boundary behavior."
                    .to_string(),
            ),
        }),
        UpstreamTestClassification::OutOfScope => Ok(CaseSelection {
            selected: false,
            status: GeneratedSelectionStatus::ExcludedByPolicy,
            reason: Some(format!("Out of scope per {}.", COMPATIBILITY_ADR)),
        }),
        UpstreamTestClassification::TemporarilyExcluded => {
            let reason = skiplist.get(&case.name).cloned().ok_or_else(|| {
                format!(
                    "temporarily_excluded case {} is missing a skiplist reason",
                    case.name
                )
            })?;
            Ok(CaseSelection {
                selected: false,
                status: GeneratedSelectionStatus::TemporarilyExcluded,
                reason: Some(reason),
            })
        }
    }
}

fn write_manifest(out_dir: &Path, manifest: &GeneratedUpstreamTestManifest) -> Result<(), String> {
    let manifest_json = serde_json::to_string_pretty(manifest)
        .map_err(|error| format!("failed to serialize upstream test manifest: {error}"))?;
    fs::write(
        out_dir.join(GENERATED_MANIFEST_JSON),
        format!("{manifest_json}\n"),
    )
    .map_err(|error| {
        format!(
            "failed to write upstream test manifest {}: {error}",
            out_dir.join(GENERATED_MANIFEST_JSON).display()
        )
    })
}

fn write_generated_runner(
    out_dir: &Path,
    manifest: &GeneratedUpstreamTestManifest,
) -> Result<(), String> {
    let mut generated = String::from(
        "// @generated by build.rs\n\
         // Each upstream Vim case is isolated in its own subprocess to avoid\n\
         // violating the single-session-per-process contract.\n\n",
    );

    let selected_cases = manifest
        .cases
        .iter()
        .filter(|case| case.selected_for_generated_runner)
        .collect::<Vec<_>>();

    if selected_cases.is_empty() {
        generated
            .push_str("// No upstream Vim test cases were selected for the generated runner.\n");
    } else {
        for case in selected_cases {
            generated.push_str("#[test]\n");
            generated.push_str(&format!("fn {}() {{\n", rust_test_name(&case.name)));
            generated.push_str(&format!(
                "    run_case_in_subprocess({:?});\n",
                case.relative_path
            ));
            generated.push_str("}\n\n");
        }
    }

    fs::write(out_dir.join(GENERATED_TESTS_RS), generated).map_err(|error| {
        format!(
            "failed to write generated upstream test runner {}: {error}",
            out_dir.join(GENERATED_TESTS_RS).display()
        )
    })
}

fn rust_test_name(case_name: &str) -> String {
    let mut name = String::from("upstream_");
    for ch in case_name.chars() {
        if ch.is_ascii_alphanumeric() {
            name.push(ch.to_ascii_lowercase());
        } else {
            name.push('_');
        }
    }
    name
}
