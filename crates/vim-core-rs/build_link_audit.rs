use regex::Regex;
use super::build_compile_plan::CompilePlan;
use std::collections::BTreeSet;
use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const NATIVE_SOURCE_AUDIT_REPORT_NAME: &str = "native-source-audit-report.txt";
const ARCHIVE_MEMBER_AUDIT_REPORT_NAME: &str = "archive-member-audit-report.txt";
const NORMAL_DELEGATION_PROOF_REPORT_NAME: &str = "normal-delegation-proof.txt";
const EX_DELEGATION_PROOF_REPORT_NAME: &str = "ex-delegation-proof.txt";

const BRIDGE_PROHIBITED_SYMBOLS: &[&str] = &[
    "exec_normal_cmd",
    "normal_cmd",
    "do_cmdline_cmd",
    "do_cmdline",
    "ml_get",
    "ml_replace",
    "ml_delete",
    "ml_append",
    "check_cursor",
    "coladvance",
    "inc_cursor",
    "curbuf->",
    "curwin->",
    "u_save",
    "u_clearall",
    "mch_early_init",
    "common_init",
    "screenalloc",
    "open_buffer",
    "restart_edit",
    "VIsual_active",
    "finish_op",
];

const BRIDGE_CUSTOM_EX_PARSER_PATTERNS: &[&str] = &[
    "strcmp(",
    "strncmp(",
    "strcasecmp(",
    "strncasecmp(",
    "matches_keyword(",
    "\":write\"",
    "\":quit\"",
    "\":redraw\"",
    "\":input\"",
    "\":bell\"",
];

const AUDIT_ALLOW_PREFIX: &str = "AUDIT-ALLOW:";

const NORMAL_DELETE_REQUIRED_SYMBOLS: &[&str] = &[
    "exec_normal_cmd",
    "ins_typebuf",
    "ml_delete",
    "changed_bytes",
    "changed_lines",
    "check_cursor",
];

const NORMAL_INSERT_REQUIRED_SYMBOLS: &[&str] = &[
    "ins_typebuf",
    "coladvance",
    "inc_cursor",
    "gchar_cursor",
    "open_line",
    "restart_edit",
    "u_save",
];

const EX_COMMANDLINE_REQUIRED_SYMBOLS: &[&str] = &[
    "vim_bridge_execute_ex_command",
    "upstream_runtime_execute_ex_command",
    "do_cmdline_cmd",
    "do_cmdline",
];

const EX_HOST_ACTION_REQUIRED_SYMBOLS: &[&str] = &[
    "vim_bridge_take_pending_host_action",
    "upstream_runtime_take_pending_host_action",
    "buf_write",
    "buf_write_all",
    "ex_exitval",
];

const EX_REDRAW_REQUIRED_SYMBOLS: &[&str] = &["redraw_cmd"];

pub fn run_link_audit(
    archive_path: &Path,
    out_dir: &Path,
    compile_plan: &CompilePlan,
    generated_sources: &[PathBuf],
) -> Result<(), String> {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR")
        .map_err(|error| format!("link audit requires CARGO_MANIFEST_DIR: {error}"))?;
    let native_dir = Path::new(&manifest_dir).join("native");
    let native_report = audit_native_sources(&native_dir)?;
    let native_report_path = write_native_source_audit_report(archive_path, out_dir, &native_report)?;
    let archive_report = audit_archive_members(archive_path, compile_plan, generated_sources)?;
    let archive_report_path =
        write_archive_member_audit_report(archive_path, out_dir, &archive_report)?;
    let normal_delegation_proof = verify_normal_delegation_symbols(archive_path)?;
    let normal_delegation_proof_path =
        write_normal_delegation_proof_report(archive_path, out_dir, &normal_delegation_proof)?;
    let ex_delegation_proof = verify_ex_delegation_symbols(archive_path)?;
    let ex_delegation_proof_path =
        write_ex_delegation_proof_report(archive_path, out_dir, &ex_delegation_proof)?;

    if native_report.passed
        && archive_report.passed
        && normal_delegation_proof.passed
        && ex_delegation_proof.passed
    {
        return Ok(());
    }

    let mut sections = Vec::new();
    if !native_report.passed {
        sections.push(format!(
            "native source audit failed. See {} for details.\n{}",
            native_report_path.display(),
            native_report
        ));
    }
    if !archive_report.passed {
        sections.push(format!(
            "archive member audit failed. See {} for details.\n{}",
            archive_report_path.display(),
            archive_report
        ));
    }
    if !normal_delegation_proof.passed {
        sections.push(format!(
            "normal delegation proof failed. See {} for details.\n{}",
            normal_delegation_proof_path.display(),
            normal_delegation_proof
        ));
    }
    if !ex_delegation_proof.passed {
        sections.push(format!(
            "ex delegation proof failed. See {} for details.\n{}",
            ex_delegation_proof_path.display(),
            ex_delegation_proof
        ));
    }

    Err(format!(
        "link audit failed for {}.\n{}",
        archive_path.display(),
        sections.join("\n")
    ))
}

pub fn audit_native_sources(native_dir: &Path) -> Result<NativeSourceAuditReport, String> {
    let mut violations = Vec::new();
    let string_compare_re = Regex::new(r#"(?i)(STRCMP|STRNCMP|strcmp|strncmp|strcasecmp|strncasecmp|matches_keyword)\s*\(.*,\s*".*"(\s*,\s*\d+)?\)"#).unwrap();

    for file_name in &["vim_bridge.c", "upstream_runtime.c"] {
        let file_path = native_dir.join(file_name);
        if file_path.exists() {
            let content = fs::read_to_string(&file_path).map_err(|error| {
                format!(
                    "native ソース監査: {} の読み取りに失敗: {error}",
                    file_path.display()
                )
            })?;
            scan_file_for_violations(&file_path, &content, &string_compare_re, &mut violations);
        }
    }

    violations.sort_by(|left, right| {
        left.file()
            .cmp(right.file())
            .then(left.line_number().cmp(&right.line_number()))
            .then(left.matched_pattern().cmp(right.matched_pattern()))
    });

    let passed = violations.is_empty();
    Ok(NativeSourceAuditReport {
        passed,
        violations,
    })
}

pub fn write_native_source_audit_report(
    archive_path: &Path,
    out_dir: &Path,
    report: &NativeSourceAuditReport,
) -> Result<PathBuf, String> {
    let report_path = out_dir.join(NATIVE_SOURCE_AUDIT_REPORT_NAME);
    let mut content = String::new();
    content.push_str("native source audit report\n");
    content.push_str(&format!("archive: {}\n", archive_path.display()));
    content.push_str(&format!(
        "status: {}\n\n",
        if report.passed { "passed" } else { "failed" }
    ));
    content.push_str(&report.to_string());
    content.push('\n');

    fs::write(&report_path, content).map_err(|error| {
        format!(
            "failed to write native source audit report {}: {error}",
            report_path.display()
        )
    })?;

    Ok(report_path)
}

pub fn audit_archive_members(
    archive_path: &Path,
    compile_plan: &CompilePlan,
    generated_sources: &[PathBuf],
) -> Result<ArchiveMemberAuditReport, String> {
    let expected_translation_units =
        expected_translation_units(compile_plan, generated_sources)?;
    let actual_archive_members = list_archive_members(archive_path)?;

    let mut actual_translation_units = BTreeSet::new();
    let mut violations = Vec::new();

    for archive_member in &actual_archive_members {
        if let Some(translation_unit) = normalize_archive_member_name(archive_member) {
            actual_translation_units.insert(translation_unit.clone());

            if !expected_translation_units.contains(&translation_unit) {
                violations.push(ArchiveAuditViolation::UnexpectedArchiveMember {
                    archive_member: translation_unit.clone(),
                    archive_entry: archive_member.clone(),
                });
            }

            if let Some(reason) = prohibited_translation_unit_reason(&translation_unit) {
                violations.push(ArchiveAuditViolation::ProhibitedTranslationUnit {
                    translation_unit,
                    reason: reason.to_string(),
                    archive_entry: archive_member.clone(),
                });
            }
        }
    }

    for translation_unit in &expected_translation_units {
        if !actual_translation_units.contains(translation_unit) {
            violations.push(ArchiveAuditViolation::MissingExpectedTranslationUnit {
                translation_unit: translation_unit.clone(),
            });
        }
    }

    violations.sort_by(|left, right| {
        left.translation_unit()
            .cmp(right.translation_unit())
            .then(left.kind().cmp(right.kind()))
    });

    let passed = violations.is_empty();
    Ok(ArchiveMemberAuditReport {
        passed,
        expected_translation_units: expected_translation_units.into_iter().collect(),
        actual_translation_units: actual_translation_units.into_iter().collect(),
        violations,
    })
}

pub fn write_archive_member_audit_report(
    archive_path: &Path,
    out_dir: &Path,
    report: &ArchiveMemberAuditReport,
) -> Result<PathBuf, String> {
    let report_path = out_dir.join(ARCHIVE_MEMBER_AUDIT_REPORT_NAME);
    let mut content = String::new();
    content.push_str("archive member audit report\n");
    content.push_str(&format!("archive: {}\n", archive_path.display()));
    content.push_str(&format!(
        "status: {}\n\n",
        if report.passed { "passed" } else { "failed" }
    ));
    content.push_str(&report.to_string());
    content.push('\n');

    fs::write(&report_path, content).map_err(|error| {
        format!(
            "failed to write archive member audit report {}: {error}",
            report_path.display()
        )
    })?;

    Ok(report_path)
}

fn expected_translation_units(
    compile_plan: &CompilePlan,
    generated_sources: &[PathBuf],
) -> Result<BTreeSet<String>, String> {
    compile_plan
        .native_sources
        .iter()
        .chain(compile_plan.vendor_sources.iter())
        .chain(generated_sources.iter())
        .map(|path| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .map(ToOwned::to_owned)
                .ok_or_else(|| {
                    format!(
                        "failed to derive translation unit name from {}",
                        path.display()
                    )
                })
        })
        .collect()
}

fn list_archive_members(archive_path: &Path) -> Result<Vec<String>, String> {
    let ar = env::var("AR").unwrap_or_else(|_| "ar".to_string());
    let output = Command::new(&ar)
        .arg("t")
        .arg(archive_path)
        .output()
        .map_err(|error| format!("failed to list archive members for {}: {error}", archive_path.display()))?;

    if !output.status.success() {
        return Err(format!(
            "failed to list archive members for {}: status {:?}\nstdout:\n{}\nstderr:\n{}",
            archive_path.display(),
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

fn list_archive_symbols(archive_path: &Path) -> Result<BTreeSet<String>, String> {
    let nm = env::var("NM").unwrap_or_else(|_| "nm".to_string());
    let output = Command::new(&nm)
        .arg("-g")
        .arg(archive_path)
        .output()
        .map_err(|error| {
            format!(
                "failed to list archive symbols for {}: {error}",
                archive_path.display()
            )
        })?;

    if !output.status.success() {
        return Err(format!(
            "failed to list archive symbols for {}: status {:?}\nstdout:\n{}\nstderr:\n{}",
            archive_path.display(),
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(parse_nm_symbol_line)
        .collect())
}

fn parse_nm_symbol_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.ends_with(':') {
        return None;
    }

    let mut fields = trimmed.split_whitespace();
    let first = fields.next()?;
    let second = fields.next()?;
    let symbol = fields.next()?;

    if first.len() == 1 {
        return normalize_symbol_name(second);
    }

    if second.len() == 1 {
        return normalize_symbol_name(symbol);
    }

    None
}

fn normalize_symbol_name(symbol: &str) -> Option<String> {
    let normalized = symbol.strip_prefix('_').unwrap_or(symbol);
    if normalized.is_empty() {
        None
    } else {
        Some(normalized.to_string())
    }
}

fn normalize_archive_member_name(archive_member: &str) -> Option<String> {
    if archive_member.starts_with("__.SYMDEF") {
        return None;
    }

    let file_name = Path::new(archive_member).file_name()?.to_str()?;
    let stem = file_name.strip_suffix(".o")?;

    if let Some((prefix, rest)) = stem.split_once('-')
        && !rest.is_empty()
        && prefix.chars().all(|ch| ch.is_ascii_hexdigit())
    {
        return Some(rest.to_string());
    }

    Some(stem.to_string())
}

fn prohibited_translation_unit_reason(translation_unit: &str) -> Option<&'static str> {
    if translation_unit == "gui" || translation_unit.starts_with("gui_") {
        return Some("GUI translation units must not be linked into the headless archive");
    }
    if translation_unit == "os_win32"
        || translation_unit == "os_mswin"
        || translation_unit == "os_vms"
        || translation_unit == "os_qnx"
        || translation_unit.starts_with("os_mac")
    {
        return Some("non-Unix platform translation units must not be linked into the headless archive");
    }
    if translation_unit.starts_with("if_") && translation_unit != "if_cscope" {
        return Some("integration-specific bridge translation units must not be linked into the headless archive");
    }
    if translation_unit.ends_with("_test") {
        return Some("test helper translation units must not be linked into the release archive");
    }

    None
}

fn scan_file_for_violations(
    file_path: &Path,
    content: &str,
    string_compare_re: &Regex,
    violations: &mut Vec<NativeSourceViolation>,
) {
    let file_name = file_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown");

    for (line_index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if should_skip_line(trimmed) {
            continue;
        }

        // Bridge prohibited symbols are only for vim_bridge.c (which should be pure wrapper)
        if file_name == "vim_bridge.c" {
            for &symbol in BRIDGE_PROHIBITED_SYMBOLS {
                if line.contains(symbol) {
                    violations.push(NativeSourceViolation::BridgeContainsVimInternalCall {
                        file: file_name.to_string(),
                        line_number: line_index + 1,
                        matched_symbol: symbol.to_string(),
                        line_content: trimmed.to_string(),
                    });
                }
            }
        }

        // Check for comment-based explicit bypass
        if trimmed.contains(AUDIT_ALLOW_PREFIX) {
            continue;
        }

        // Regex-based check for string comparison with literals
        if let Some(captures) = string_compare_re.captures(line) {
            let matched_pattern = captures.get(0).unwrap().as_str().to_string();
            violations.push(NativeSourceViolation::BridgeContainsCustomExParser {
                file: file_name.to_string(),
                line_number: line_index + 1,
                matched_pattern,
                line_content: trimmed.to_string(),
            });
            continue;
        }

        // Legacy keyword-based check for safety
        for &pattern in BRIDGE_CUSTOM_EX_PARSER_PATTERNS {
            if line.contains(pattern) {
                violations.push(NativeSourceViolation::BridgeContainsCustomExParser {
                    file: file_name.to_string(),
                    line_number: line_index + 1,
                    matched_pattern: pattern.to_string(),
                    line_content: trimmed.to_string(),
                });
            }
        }
    }
}

fn should_skip_line(trimmed: &str) -> bool {
    trimmed.is_empty()
        || trimmed.starts_with("//")
        || trimmed.starts_with('*')
        || trimmed.starts_with("/*")
        || trimmed.starts_with("#include")
}

#[derive(Debug)]
pub struct NativeSourceAuditReport {
    pub passed: bool,
    pub violations: Vec<NativeSourceViolation>,
}

impl fmt::Display for NativeSourceAuditReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.passed {
            write!(f, "native ソース監査: 合格（違反なし）")
        } else {
            writeln!(
                f,
                "native ソース監査: 不合格（{} 件の違反）",
                self.violations.len()
            )?;
            for violation in &self.violations {
                writeln!(f, "  {violation}")?;
            }
            Ok(())
        }
    }
}

#[derive(Debug)]
pub enum NativeSourceViolation {
    BridgeContainsVimInternalCall {
        file: String,
        line_number: usize,
        matched_symbol: String,
        line_content: String,
    },
    BridgeContainsCustomExParser {
        file: String,
        line_number: usize,
        matched_pattern: String,
        line_content: String,
    },
}

impl NativeSourceViolation {
    fn file(&self) -> &str {
        match self {
            Self::BridgeContainsVimInternalCall { file, .. }
            | Self::BridgeContainsCustomExParser { file, .. } => file,
        }
    }

    fn line_number(&self) -> usize {
        match self {
            Self::BridgeContainsVimInternalCall { line_number, .. }
            | Self::BridgeContainsCustomExParser { line_number, .. } => *line_number,
        }
    }

    fn matched_pattern(&self) -> &str {
        match self {
            Self::BridgeContainsVimInternalCall { matched_symbol, .. } => matched_symbol,
            Self::BridgeContainsCustomExParser { matched_pattern, .. } => matched_pattern,
        }
    }
}

impl fmt::Display for NativeSourceViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BridgeContainsVimInternalCall {
                file,
                line_number,
                matched_symbol,
                line_content,
            } => write!(
                f,
                "{}:{} bridge must not call Vim internal symbol `{}`: {}",
                file, line_number, matched_symbol, line_content
            ),
            Self::BridgeContainsCustomExParser {
                file,
                line_number,
                matched_pattern,
                line_content,
            } => write!(
                f,
                "{}:{} bridge must not implement custom Ex parsing via `{}`: {}",
                file, line_number, matched_pattern, line_content
            ),
        }
    }
}

#[derive(Debug)]
pub struct ArchiveMemberAuditReport {
    pub passed: bool,
    pub expected_translation_units: Vec<String>,
    pub actual_translation_units: Vec<String>,
    pub violations: Vec<ArchiveAuditViolation>,
}

impl fmt::Display for ArchiveMemberAuditReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "archive member audit: {}",
            if self.passed {
                "passed"
            } else {
                "failed"
            }
        )?;
        writeln!(
            f,
            "  expected translation units: {}",
            self.expected_translation_units.join(", ")
        )?;
        writeln!(
            f,
            "  actual translation units: {}",
            self.actual_translation_units.join(", ")
        )?;

        if self.violations.is_empty() {
            write!(f, "  violations: none")
        } else {
            writeln!(f, "  violations:")?;
            for violation in &self.violations {
                writeln!(f, "    {violation}")?;
            }
            Ok(())
        }
    }
}

#[derive(Debug)]
pub enum ArchiveAuditViolation {
    MissingExpectedTranslationUnit {
        translation_unit: String,
    },
    UnexpectedArchiveMember {
        archive_member: String,
        archive_entry: String,
    },
    ProhibitedTranslationUnit {
        translation_unit: String,
        reason: String,
        archive_entry: String,
    },
}

impl ArchiveAuditViolation {
    fn translation_unit(&self) -> &str {
        match self {
            Self::MissingExpectedTranslationUnit { translation_unit }
            | Self::ProhibitedTranslationUnit {
                translation_unit, ..
            } => translation_unit,
            Self::UnexpectedArchiveMember { archive_member, .. } => archive_member,
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::MissingExpectedTranslationUnit { .. } => "missing",
            Self::UnexpectedArchiveMember { .. } => "unexpected",
            Self::ProhibitedTranslationUnit { .. } => "prohibited",
        }
    }
}

impl fmt::Display for ArchiveAuditViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingExpectedTranslationUnit { translation_unit } => write!(
                f,
                "missing translation unit `{translation_unit}` from libvimcore.a"
            ),
            Self::UnexpectedArchiveMember {
                archive_member,
                archive_entry,
            } => write!(
                f,
                "unexpected archive member `{archive_entry}` (normalized as `{archive_member}`) is not declared in the compile plan"
            ),
            Self::ProhibitedTranslationUnit {
                translation_unit,
                reason,
                archive_entry,
            } => write!(
                f,
                "prohibited translation unit `{translation_unit}` found in archive member `{archive_entry}`: {reason}"
            ),
        }
    }
}

pub fn verify_normal_delegation_symbols(
    archive_path: &Path,
) -> Result<NormalDelegationProofReport, String> {
    let archive_symbols = list_archive_symbols(archive_path)?;
    Ok(build_normal_delegation_proof_from_symbols(archive_symbols))
}

pub fn verify_ex_delegation_symbols(archive_path: &Path) -> Result<ExDelegationProofReport, String> {
    let archive_symbols = list_archive_symbols(archive_path)?;
    Ok(build_ex_delegation_proof_from_symbols(archive_symbols))
}

pub fn build_normal_delegation_proof_from_symbols(
    archive_symbols: BTreeSet<String>,
) -> NormalDelegationProofReport {
    let scenarios = vec![
        DelegationScenarioProof::from_required_symbols(
            "normal-delete",
            "Normal 削除系コマンドが upstream の通常実行・buffer mutation・cursor 更新へ委譲している証跡",
            NORMAL_DELETE_REQUIRED_SYMBOLS,
            &archive_symbols,
        ),
        DelegationScenarioProof::from_required_symbols(
            "normal-insert-enter",
            "Insert/Replace 開始系コマンドが upstream の mode 遷移・cursor 補正・open line へ委譲している証跡",
            NORMAL_INSERT_REQUIRED_SYMBOLS,
            &archive_symbols,
        ),
    ];
    let passed = scenarios.iter().all(|scenario| scenario.missing_symbols.is_empty());

    NormalDelegationProofReport {
        passed,
        archive_symbols: archive_symbols.into_iter().collect(),
        scenarios,
    }
}

pub fn write_normal_delegation_proof_report(
    archive_path: &Path,
    out_dir: &Path,
    report: &NormalDelegationProofReport,
) -> Result<PathBuf, String> {
    let report_path = out_dir.join(NORMAL_DELEGATION_PROOF_REPORT_NAME);
    let mut content = String::new();
    content.push_str("normal delegation proof report\n");
    content.push_str(&format!("archive: {}\n", archive_path.display()));
    content.push_str(&format!(
        "status: {}\n\n",
        if report.passed { "passed" } else { "failed" }
    ));
    content.push_str(&report.to_string());
    content.push('\n');

    fs::write(&report_path, content).map_err(|error| {
        format!(
            "failed to write normal delegation proof report {}: {error}",
            report_path.display()
        )
    })?;

    Ok(report_path)
}

pub fn build_ex_delegation_proof_from_symbols(
    archive_symbols: BTreeSet<String>,
) -> ExDelegationProofReport {
    let scenarios = vec![
        DelegationScenarioProof::from_required_symbols(
            "ex-commandline",
            "Ex 実行が bridge/runtime から upstream の command line 実行経路へ接続されている証跡",
            EX_COMMANDLINE_REQUIRED_SYMBOLS,
            &archive_symbols,
        ),
        DelegationScenarioProof::from_required_symbols(
            "ex-host-actions",
            "write/quit host action 回収が bridge/runtime の queue と upstream の write/quit 実装存在に支えられている証跡",
            EX_HOST_ACTION_REQUIRED_SYMBOLS,
            &archive_symbols,
        ),
        DelegationScenarioProof::from_required_symbols(
            "ex-redraw",
            "redraw 系 Ex コマンドが upstream redraw 実装へ委譲できる証跡",
            EX_REDRAW_REQUIRED_SYMBOLS,
            &archive_symbols,
        ),
    ];
    let passed = scenarios.iter().all(|scenario| scenario.missing_symbols.is_empty());

    ExDelegationProofReport {
        passed,
        archive_symbols: archive_symbols.into_iter().collect(),
        scenarios,
    }
}

pub fn write_ex_delegation_proof_report(
    archive_path: &Path,
    out_dir: &Path,
    report: &ExDelegationProofReport,
) -> Result<PathBuf, String> {
    let report_path = out_dir.join(EX_DELEGATION_PROOF_REPORT_NAME);
    let mut content = String::new();
    content.push_str("ex delegation proof report\n");
    content.push_str(&format!("archive: {}\n", archive_path.display()));
    content.push_str(&format!(
        "status: {}\n\n",
        if report.passed { "passed" } else { "failed" }
    ));
    content.push_str(&report.to_string());
    content.push('\n');

    fs::write(&report_path, content).map_err(|error| {
        format!(
            "failed to write ex delegation proof report {}: {error}",
            report_path.display()
        )
    })?;

    Ok(report_path)
}

#[derive(Debug)]
pub struct NormalDelegationProofReport {
    pub passed: bool,
    pub archive_symbols: Vec<String>,
    pub scenarios: Vec<DelegationScenarioProof>,
}

impl fmt::Display for NormalDelegationProofReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "normal delegation proof: {}",
            if self.passed { "passed" } else { "failed" }
        )?;
        writeln!(f, "  archive symbols discovered: {}", self.archive_symbols.len())?;
        for scenario in &self.scenarios {
            writeln!(f, "  {scenario}")?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct ExDelegationProofReport {
    pub passed: bool,
    pub archive_symbols: Vec<String>,
    pub scenarios: Vec<DelegationScenarioProof>,
}

impl fmt::Display for ExDelegationProofReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "ex delegation proof: {}",
            if self.passed { "passed" } else { "failed" }
        )?;
        writeln!(f, "  archive symbols discovered: {}", self.archive_symbols.len())?;
        for scenario in &self.scenarios {
            writeln!(f, "  {scenario}")?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct DelegationScenarioProof {
    pub name: String,
    pub description: String,
    pub required_symbols: Vec<String>,
    pub missing_symbols: Vec<String>,
}

impl DelegationScenarioProof {
    fn from_required_symbols(
        name: &str,
        description: &str,
        required_symbols: &[&str],
        archive_symbols: &BTreeSet<String>,
    ) -> Self {
        let required_symbols = required_symbols
            .iter()
            .map(|symbol| (*symbol).to_string())
            .collect::<Vec<_>>();
        let missing_symbols = required_symbols
            .iter()
            .filter(|symbol| !archive_symbols.contains(*symbol))
            .cloned()
            .collect::<Vec<_>>();

        Self {
            name: name.to_string(),
            description: description.to_string(),
            required_symbols,
            missing_symbols,
        }
    }
}

impl fmt::Display for DelegationScenarioProof {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "scenario `{}`: {} | required: {}",
            self.name,
            self.description,
            self.required_symbols.join(", ")
        )?;
        if self.missing_symbols.is_empty() {
            write!(f, " | status: passed")
        } else {
            write!(
                f,
                " | status: failed | missing: {}",
                self.missing_symbols.join(", ")
            )
        }
    }
}
