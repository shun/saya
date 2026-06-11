//! startup モジュールの import 指定子解決とハッシュユーティリティ。

use super::*;

pub(super) fn canonical_startup_module_id(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    bytes_to_hex(&Sha256::digest(bytes))
}

pub(super) fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

pub(super) fn collect_static_import_statement(
    lines: &[&str],
    start_index: usize,
) -> (String, usize) {
    let first_line = lines[start_index];
    let trimmed = first_line.trim_start();
    if !trimmed.starts_with("import ") && !trimmed.starts_with("export {") {
        return (first_line.to_string(), 1);
    }

    let mut statement = first_line.to_string();
    let mut consumed_lines = 1usize;
    while !statement.trim_end().ends_with(';') && start_index + consumed_lines < lines.len() {
        statement.push('\n');
        statement.push_str(lines[start_index + consumed_lines]);
        consumed_lines += 1;
    }
    (statement, consumed_lines)
}

pub(super) fn parse_static_import_specifier(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if !trimmed.starts_with("import ") {
        return None;
    }
    let after_from = trimmed
        .split_once(" from ")
        .map(|(_, specifier)| specifier.trim())
        .unwrap_or_else(|| trimmed.trim_start_matches("import").trim());
    parse_quoted_module_specifier(after_from.trim_end_matches(';').trim())
}

pub(super) fn parse_static_re_export_specifier(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if !trimmed.starts_with("export ") {
        return None;
    }
    let (_, specifier) = trimmed.split_once(" from ")?;
    parse_quoted_module_specifier(specifier.trim_end_matches(';').trim())
}

pub(super) fn parse_quoted_module_specifier(value: &str) -> Option<&str> {
    let quote = value.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rest = &value[quote.len_utf8()..];
    let end = rest.find(quote)?;
    Some(&rest[..end])
}

pub(super) fn resolve_local_startup_import(
    importer: &Path,
    specifier: &str,
) -> Result<PathBuf, String> {
    let path = if let Some(path) = specifier.strip_prefix("file://") {
        PathBuf::from(path)
    } else if specifier.starts_with("./") || specifier.starts_with("../") {
        importer
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(specifier)
    } else if let Some(path) = specifier.strip_prefix("~/") {
        resolve_home_relative_startup_import(specifier, path, std::env::var_os("HOME"))?
    } else if let Some((name, path)) = parse_env_relative_startup_import(specifier) {
        resolve_env_relative_startup_import(specifier, name, path, std::env::var_os(name))?
    } else if specifier.starts_with('/') {
        PathBuf::from(specifier)
    } else {
        return Err(format!(
            "unsupported startup import specifier: {} (only local file imports are supported)",
            specifier
        ));
    };

    Ok(path)
}

pub(super) fn resolve_home_relative_startup_import(
    specifier: &str,
    path: &str,
    home: Option<OsString>,
) -> Result<PathBuf, String> {
    let home = home.filter(|value| !value.is_empty()).ok_or_else(|| {
        format!("unsupported startup import specifier: {specifier} (HOME is not set)")
    })?;
    Ok(PathBuf::from(home).join(path))
}

pub(super) fn parse_env_relative_startup_import(specifier: &str) -> Option<(&str, &str)> {
    let rest = specifier.strip_prefix('$')?;
    if let Some(rest) = rest.strip_prefix('{') {
        let (name, path) = rest.split_once("}/")?;
        if is_valid_env_startup_import_name(name) {
            return Some((name, path));
        }
        return None;
    }

    let (name, path) = rest.split_once('/')?;
    if is_valid_env_startup_import_name(name) {
        Some((name, path))
    } else {
        None
    }
}

pub(super) fn is_valid_env_startup_import_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if first != '_' && !first.is_ascii_alphabetic() {
        return false;
    }
    chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

pub(super) fn resolve_env_relative_startup_import(
    specifier: &str,
    name: &str,
    path: &str,
    value: Option<OsString>,
) -> Result<PathBuf, String> {
    let value = value.filter(|value| !value.is_empty()).ok_or_else(|| {
        format!("unsupported startup import specifier: {specifier} ({name} is not set)")
    })?;
    Ok(PathBuf::from(value).join(path))
}

#[cfg(test)]
mod startup_import_path_tests {
    use super::*;

    #[test]
    fn home_relative_startup_import_resolves_against_home_directory() {
        let path = resolve_home_relative_startup_import(
            "~/saya-plugins/number.ts",
            "saya-plugins/number.ts",
            Some(OsString::from("/tmp/saya-home")),
        )
        .expect("home-relative import");

        assert_eq!(
            path,
            PathBuf::from("/tmp/saya-home")
                .join("saya-plugins")
                .join("number.ts")
        );
    }

    #[test]
    fn home_relative_startup_import_requires_home_directory() {
        let result = resolve_home_relative_startup_import(
            "~/saya-plugins/number.ts",
            "saya-plugins/number.ts",
            None,
        );

        assert_eq!(
            result,
            Err(
                "unsupported startup import specifier: ~/saya-plugins/number.ts (HOME is not set)"
                    .to_string()
            )
        );
    }

    #[test]
    fn env_relative_startup_import_parses_plain_environment_prefix() {
        assert_eq!(
            parse_env_relative_startup_import("$SAYA_HOME/runtime/plugins/dired/index.ts"),
            Some(("SAYA_HOME", "runtime/plugins/dired/index.ts"))
        );
    }

    #[test]
    fn env_relative_startup_import_parses_braced_environment_prefix() {
        assert_eq!(
            parse_env_relative_startup_import("${SAYA_HOME}/runtime/plugins/dired/index.ts"),
            Some(("SAYA_HOME", "runtime/plugins/dired/index.ts"))
        );
    }

    #[test]
    fn env_relative_startup_import_rejects_invalid_environment_prefix() {
        assert_eq!(parse_env_relative_startup_import("$1_BAD/plugin.ts"), None);
        assert_eq!(
            parse_env_relative_startup_import("${SAYA_HOME/plugin.ts"),
            None
        );
    }

    #[test]
    fn env_relative_startup_import_resolves_against_environment_value() {
        let path = resolve_env_relative_startup_import(
            "$SAYA_HOME/runtime/plugins/dired/index.ts",
            "SAYA_HOME",
            "runtime/plugins/dired/index.ts",
            Some(OsString::from("/tmp/saya-home")),
        )
        .expect("env-relative import");

        assert_eq!(
            path,
            PathBuf::from("/tmp/saya-home")
                .join("runtime")
                .join("plugins")
                .join("dired")
                .join("index.ts")
        );
    }

    #[test]
    fn env_relative_startup_import_requires_environment_value() {
        let result = resolve_env_relative_startup_import(
            "$SAYA_HOME/runtime/plugins/dired/index.ts",
            "SAYA_HOME",
            "runtime/plugins/dired/index.ts",
            None,
        );

        assert_eq!(
            result,
            Err(
                "unsupported startup import specifier: $SAYA_HOME/runtime/plugins/dired/index.ts (SAYA_HOME is not set)"
                    .to_string()
            )
        );
    }
}
