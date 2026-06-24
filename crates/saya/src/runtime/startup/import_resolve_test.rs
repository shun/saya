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
