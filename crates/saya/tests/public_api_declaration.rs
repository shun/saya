use saya::runtime::live::RUNTIME_SAYA_TYPE_DECLARATION;
use saya::runtime::startup::STARTUP_SAYA_TYPE_DECLARATION;

#[test]
fn startup_public_api_type_declaration_is_published_for_external_language_servers() {
    let declaration = std::fs::read_to_string(
        saya::support::paths::dev_ts_plugins_dir().join("saya-startup.d.ts"),
    )
    .expect("startup API declaration should be published for TypeScript language servers");

    assert_eq!(
        strip_deno_fmt_ignore_file(&declaration),
        STARTUP_SAYA_TYPE_DECLARATION.trim(),
        "published startup declaration must stay in sync with the runtime startup surface"
    );
}

#[test]
fn runtime_public_api_type_declaration_is_published_for_external_language_servers() {
    let declaration = std::fs::read_to_string(
        saya::support::paths::dev_ts_plugins_dir().join("types/runtime.d.ts"),
    )
    .expect("runtime API declaration should be published for TypeScript language servers");

    assert_eq!(
        strip_deno_fmt_ignore_file(&declaration),
        RUNTIME_SAYA_TYPE_DECLARATION.trim(),
        "published runtime declaration must stay in sync with the live runtime surface"
    );
}

fn strip_deno_fmt_ignore_file(declaration: &str) -> &str {
    declaration
        .strip_prefix("// deno-fmt-ignore-file\n\n")
        .unwrap_or(declaration)
        .trim()
}

#[test]
fn startup_public_api_type_declaration_covers_formal_configuration_surface() {
    let declaration = STARTUP_SAYA_TYPE_DECLARATION;

    assert!(declaration.contains("declare global"));
    assert!(declaration.contains("tabstop"));
    assert!(
        !declaration.contains("tabSize"),
        "startup API should use Vim-compatible tabstop naming without tabSize alias"
    );
    assert!(declaration.contains("number"));
    assert!(declaration.contains("numberwidth"));
    assert!(declaration.contains("cmdheight"));
    for removed in [
        "lineNumbers",
        "numberWidth",
        "messageHeight",
        "messageheight",
    ] {
        assert!(
            !declaration.contains(removed),
            "startup API should use Vim-compatible option naming without {removed} alias"
        );
    }
    assert!(declaration.contains("syntax"));
    assert!(declaration.contains("keymap"));
    assert!(declaration.contains("ftplugin"));
    assert!(declaration.contains("statusline"));
    assert!(declaration.contains("filetype"));
    assert!(declaration.contains("commands"));
    assert!(declaration.contains("events"));
    assert!(declaration.contains("theme"));
    assert!(declaration.contains("plugins"));
    assert!(declaration.contains("use(specs: SayaPluginUseSpec[])"));
    assert!(declaration.contains("lazy(specs: SayaPluginLazySpec[])"));
    assert!(declaration.contains("SayaStartupThemeSurface"));
    assert!(declaration.contains("inlineCode"));
    assert!(declaration.contains("SayaStartupSurface"));
}

#[test]
fn startup_public_api_type_declaration_does_not_expose_runtime_or_gui_transport_details() {
    let declaration = STARTUP_SAYA_TYPE_DECLARATION.to_ascii_lowercase();

    for forbidden in [
        "overlay",
        "graphics",
        "protocol",
        "bytes",
        "kitty",
        "sixel",
        "window.open",
        "gpu",
    ] {
        assert!(
            !declaration.contains(forbidden),
            "startup declaration should stay TUI-only and avoid transport leaks: {forbidden}"
        );
    }
}

#[test]
fn runtime_public_api_type_declaration_covers_formal_execution_surface() {
    let declaration = RUNTIME_SAYA_TYPE_DECLARATION;

    assert!(declaration.contains("declare global"));
    assert!(declaration.contains("commands"));
    assert!(declaration.contains("buffer"));
    assert!(declaration.contains("window"));
    assert!(declaration.contains("SayaRuntimeOpenFloatOptions"));
    assert!(declaration.contains("SayaReadonlyFloatSnapshot"));
    assert!(declaration.contains("type SayaPanelNode"));
    assert!(declaration.contains("{ kind: \"view\"; nodes: SayaPanelNode[] }"));
    assert!(declaration.contains("kind: \"terminal\" | \"lines\" | \"view\""));
    assert!(declaration.contains("openFloat"));
    assert!(declaration.contains("close(id: number)"));
    assert!(declaration.contains("focus(id: number)"));
    assert!(declaration.contains("floats()"));
    assert!(declaration.contains("interface SayaCompletionKeyBindings"));
    assert!(declaration.contains("keys?: SayaCompletionKeyBindings"));
    assert!(declaration.contains("close(): Promise<boolean>"));
    assert!(declaration.contains("editor"));
    assert!(declaration.contains("filer"));
    assert!(declaration.contains("lsp"));
    assert!(declaration.contains("SayaRuntimeLspSurface"));
    assert!(declaration.contains("connect(options: SayaLspConnectOptions)"));
    assert!(declaration.contains("interface SayaRuntimeLspClient"));
    assert!(declaration.contains("lsif"));
    assert!(declaration.contains("SayaRuntimeLsifSurface"));
    assert!(declaration.contains("SayaLsifRuntimeBridgeRequest"));
    assert!(declaration.contains("SayaLsifRuntimeBridgeResponse"));
    assert!(declaration.contains("positionEncoding"));
    assert!(declaration.contains("input"));
    assert!(declaration.contains("SayaRuntimeInputSurface"));
    assert!(
        declaration.contains("prompt(options: SayaInputPromptOptions): Promise<string | null>")
    );
    assert!(declaration.contains("SayaRuntimeFilerSurface"));
    assert!(declaration.contains("SayaFilerListOptions"));
    assert!(declaration.contains("filter?: string | null"));
    assert!(declaration.contains("SayaFilerEntry"));
    assert!(declaration.contains("SayaCurrentFilerEntry"));
    assert!(declaration.contains("currentEntry"));
    assert!(declaration.contains("createFile"));
    assert!(declaration.contains("createDirectory"));
    assert!(declaration.contains("copy"));
    assert!(declaration.contains("move"));
    assert!(declaration.contains("rename"));
    assert!(declaration.contains("delete"));
    assert!(declaration.contains("mark"));
    assert!(declaration.contains("unmark"));
    assert!(declaration.contains("clearMarks"));
    assert!(declaration.contains("bulkDeletePreview"));
    assert!(declaration.contains("bulkDelete"));
    assert!(declaration.contains("previewId"));
    assert!(declaration.contains("SayaFilerOperationReport"));
    assert!(declaration.contains("type SayaFilerOperationKind"));
    assert!(declaration.contains("interface SayaFilerDeleteOptions"));
    assert!(declaration.contains("interface SayaFilerBulkDeleteOptions"));
    assert!(declaration.contains("interface SayaDirectoryBufferOperationPreview"));
    assert!(declaration.contains("interface SayaDirectoryBufferOperationPrompt"));
    assert!(declaration.contains("interface SayaDirectoryBufferApplyReport"));
    assert!(declaration.contains("cursorRow"));
    assert!(declaration.contains("cursorCol"));
    assert!(declaration.contains("currentLine"));
    assert!(declaration.contains("SayaRuntimeSurface"));

    for forbidden in ["renderer", "FloatingScreenModel", "rawTerminal", "drawCell"] {
        assert!(
            !declaration.contains(forbidden),
            "runtime float API must not expose raw renderer access: {forbidden}"
        );
    }
}

#[test]
fn dired_v1_preview_contract_docs_match_guarded_public_declarations() {
    let declaration = RUNTIME_SAYA_TYPE_DECLARATION;
    let dired_contract = std::fs::read_to_string("docs/api/dired-api-v1.md")
        .expect("versioned dired API contract should be readable");
    let release_notes =
        std::fs::read_to_string("docs/release-notes.md").expect("release notes should be readable");

    for expected in [
        "saya.filer.list(path, options)",
        "saya.filer.currentEntry()",
        "saya.filer.createFile(path)",
        "saya.filer.createDirectory(path)",
        "saya.filer.copy(from, to)",
        "saya.filer.move(from, to)",
        "saya.filer.rename(from, to)",
        "saya.filer.delete(path, options)",
        "saya.filer.mark(path)",
        "saya.filer.unmark(path)",
        "saya.filer.clearMarks()",
        "saya.filer.bulkDeletePreview()",
        "saya.filer.bulkDelete(options)",
        "setupSayaDired(options)",
        "SayaFilerDeleteOptions",
        "SayaFilerBulkDeleteOptions",
        "SayaFilerOperationReport",
        "SayaDirectoryBufferOperationPreview",
        "SayaDirectoryBufferOperationPrompt",
        "SayaDirectoryBufferApplyReport",
        "Migration notes",
        "Anti-patterns",
    ] {
        assert!(
            dired_contract.contains(expected),
            "versioned dired contract should document public item: {expected}"
        );
    }

    for expected in [
        "type SayaFilerOperationKind",
        "interface SayaFilerDeleteOptions",
        "interface SayaFilerBulkDeleteOptions",
        "interface SayaFilerOperationReport",
        "interface SayaDirectoryBufferOperationPreview",
        "interface SayaDirectoryBufferOperationPrompt",
        "interface SayaDirectoryBufferApplyReport",
    ] {
        assert!(
            declaration.contains(expected),
            "runtime declaration guard should pin dired public type: {expected}"
        );
    }

    assert!(
        release_notes.contains("Dired v1 preview API")
            && release_notes.contains("remains a preview feature")
            && release_notes.contains("backend adapter"),
        "release notes should record why the preview label remains"
    );
}

#[test]
fn runtime_public_api_type_declaration_does_not_expose_transport_specific_presentation_details() {
    let declaration = RUNTIME_SAYA_TYPE_DECLARATION
        .to_ascii_lowercase()
        .replace("estimatedbytes", "");

    for forbidden in [
        "overlay", "graphics", "protocol", "bytes", "asset", "kitty", "sixel", "wgpu", "neovim",
    ] {
        assert!(
            !declaration.contains(forbidden),
            "runtime declaration should keep presentation transport private: {forbidden}"
        );
    }
}
