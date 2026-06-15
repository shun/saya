//! 起動オプションの正規化とセッション・コアへの適用を担当する。

use super::*;

pub(super) fn normalize_tab_size(tab_size: i64) -> u16 {
    u16::try_from(tab_size).unwrap_or(8).max(1)
}

pub(super) fn normalize_number_width(number_width: i64) -> u16 {
    u16::try_from(number_width).unwrap_or(4).max(1)
}

pub(super) fn normalize_message_height(message_height: i64) -> u16 {
    u16::try_from(message_height).unwrap_or(5).max(1)
}

pub(super) fn normalize_percent(value: i64) -> u16 {
    u16::try_from(value.clamp(1, 100)).unwrap_or(100)
}

pub(super) fn normalize_u16(value: i64) -> u16 {
    u16::try_from(value.max(0)).unwrap_or(u16::MAX)
}

pub(super) fn normalize_i16(value: i64) -> i16 {
    i16::try_from(value).unwrap_or(0)
}

pub(super) fn normalize_u8(value: i64) -> u8 {
    u8::try_from(value.max(0)).unwrap_or(u8::MAX)
}

pub(super) fn apply_startup_presentation_to_session_state(
    state: &mut EditorSessionState,
    options: &StartupOptionsSnapshot,
) {
    let presentation_options = [
        (
            SayaOptionName::RelativeNumber,
            SayaOptionValue::Boolean(options.relative_number),
        ),
        (
            SayaOptionName::CursorLine,
            SayaOptionValue::Boolean(options.cursorline),
        ),
        (
            SayaOptionName::ScrollOff,
            SayaOptionValue::Number(i64::from(options.scrolloff)),
        ),
        (
            SayaOptionName::SidescrollOff,
            SayaOptionValue::Number(i64::from(options.sidescrolloff)),
        ),
        (SayaOptionName::Wrap, SayaOptionValue::Boolean(options.wrap)),
        (
            SayaOptionName::LastStatus,
            SayaOptionValue::Number(i64::from(options.laststatus)),
        ),
        (
            SayaOptionName::MessageHeight,
            SayaOptionValue::Number(i64::from(options.message_height)),
        ),
        (SayaOptionName::List, SayaOptionValue::Boolean(options.list)),
        (
            SayaOptionName::ListChars,
            SayaOptionValue::String(options.listchars.clone()),
        ),
        (
            SayaOptionName::MermaidPreview,
            SayaOptionValue::Boolean(options.mermaid_preview_auto),
        ),
        (
            SayaOptionName::MermaidPreviewBackground,
            SayaOptionValue::String(options.mermaid_preview_background.clone()),
        ),
        (
            SayaOptionName::MermaidPreviewWidth,
            SayaOptionValue::Number(i64::from(options.mermaid_preview_width_percent)),
        ),
        (
            SayaOptionName::MermaidPreviewHeight,
            SayaOptionValue::Number(i64::from(options.mermaid_preview_height_percent)),
        ),
        (
            SayaOptionName::FoldMethod,
            SayaOptionValue::String(options.foldmethod.clone()),
        ),
        (
            SayaOptionName::FoldLevel,
            SayaOptionValue::Number(i64::from(options.foldlevel)),
        ),
    ];
    for (name, value) in presentation_options {
        let _ = state.apply_presentation_option(name, value);
    }
}

pub(super) fn apply_startup_core_options(
    core_bridge: &mut CoreBridge,
    options: &StartupOptionsSnapshot,
) {
    let core_options = [
        (
            SayaOptionName::TabSize,
            SayaOptionValue::Number(i64::from(options.tab_size)),
        ),
        (
            SayaOptionName::ExpandTab,
            SayaOptionValue::Boolean(options.expandtab),
        ),
        (
            SayaOptionName::ShiftWidth,
            SayaOptionValue::Number(i64::from(options.shiftwidth)),
        ),
        (
            SayaOptionName::SoftTabStop,
            SayaOptionValue::Number(i64::from(options.softtabstop)),
        ),
        (
            SayaOptionName::AutoIndent,
            SayaOptionValue::Boolean(options.autoindent),
        ),
        (
            SayaOptionName::SmartIndent,
            SayaOptionValue::Boolean(options.smartindent),
        ),
        (
            SayaOptionName::IgnoreCase,
            SayaOptionValue::Boolean(options.ignorecase),
        ),
        (
            SayaOptionName::SmartCase,
            SayaOptionValue::Boolean(options.smartcase),
        ),
    ];
    for (name, value) in core_options {
        if let Err(error) = core_bridge.set_core_option(name, value) {
            log::debug!(
                "[bootstrap] startup core option application failed and was ignored: name={}, error={:?}",
                name,
                error
            );
        }
    }

    let syntax_command = if options.syntax {
        "syntax on"
    } else {
        "syntax off"
    };
    log::debug!(
        "[bootstrap] applying startup syntax option through Vim core ex command: syntax={}, command={:?}",
        options.syntax,
        syntax_command
    );
    if let Err(error) = core_bridge.apply_ex_command(syntax_command) {
        log::debug!(
            "[bootstrap] startup syntax command application failed and was ignored: command={:?}, error={:?}",
            syntax_command,
            error
        );
    }

    let hlsearch_command = if options.hlsearch {
        "set hlsearch"
    } else {
        "set nohlsearch"
    };
    log::debug!(
        "[bootstrap] applying startup hlsearch option through Vim core ex command: hlsearch={}, command={:?}",
        options.hlsearch,
        hlsearch_command
    );
    if let Err(error) = core_bridge.apply_ex_command(hlsearch_command) {
        log::debug!(
            "[bootstrap] startup hlsearch command application failed and was ignored: command={:?}, error={:?}",
            hlsearch_command,
            error
        );
    }
}

pub(super) fn apply_startup_ftplugin_options(
    core_bridge: &mut CoreBridge,
    target_path: Option<&Path>,
    config: &FtPluginConfig,
) {
    let Some(ftplugin) = resolve_ftplugin_for_path(target_path, config) else {
        log::debug!(
            "[bootstrap][ftplugin] no startup ftplugin matched: target_path={:?}",
            target_path
        );
        return;
    };

    log::debug!(
        "[bootstrap][ftplugin] applying startup ftplugin: filetype={}, option_count={}, target_path={:?}",
        ftplugin.filetype,
        ftplugin.options.len(),
        target_path
    );
    for option in ftplugin.options {
        if let Err(error) = core_bridge.set_core_option(option.name, option.value) {
            log::debug!(
                "[bootstrap][ftplugin] ftplugin core option application failed and was ignored: filetype={}, name={}, error={:?}",
                ftplugin.filetype,
                option.name,
                error
            );
        }
    }
}

pub(super) fn map_session_guard_error(error: SessionGuardError) -> BootstrapError {
    match error {
        SessionGuardError::AlreadyInitialized => BootstrapError::SessionAlreadyInitialized,
    }
}
