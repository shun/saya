//! startup keymap 登録コマンドの実行と、入力からのキーマップ解決。

use super::*;

pub async fn execute_startup_keymap_registered_command(
    runtime_session: Option<&mut RuntimeSessionOwner>,
    command_name: &str,
    outcome: &mut crate::app::bootstrap::BootstrapOutcome,
    session_state: &mut crate::app::session::EditorSessionState,
    floating_window_manager: &mut FloatingWindowManager,
    completion_float_manager: &mut CompletionFloatManager,
    lsp_diagnostic_store: &mut LspDiagnosticStore,
    terminal_float_manager: &mut TerminalFloatManager,
    panel_manager: &mut PanelManager,
    runtime_input_prompt: Option<&mut Option<RuntimeInputPromptUiState>>,
    transient_msg: &mut Option<String>,
    need_redraw: &mut bool,
    runtime_presentation_intents: &mut Vec<RuntimePresentationIntent>,
    lsif_bridge: Option<&LsifBridgeHandle>,
) -> Option<ShutdownReason> {
    let Some(runtime_session) = runtime_session else {
        log::info!(
            "[main][keymap] registered command skipped because runtime session is unavailable: command={}",
            command_name
        );
        *transient_msg = Some(format!("Runtime command unavailable: {}", command_name));
        *need_redraw = true;
        return None;
    };
    log::info!(
        "[main][keymap] executing startup registered command: command={}",
        command_name
    );
    let mut host_session = MainRuntimeHostSession::new_with_floating_windows(
        outcome,
        session_state,
        floating_window_manager,
        completion_float_manager,
        lsp_diagnostic_store,
        terminal_float_manager,
        panel_manager,
        lsif_bridge,
    );
    host_session.runtime_input_prompt = runtime_input_prompt;
    let dispatch_outcome = runtime_session
        .execute_command(command_name, &mut host_session)
        .await;
    apply_runtime_dispatch_outcome(
        transient_msg,
        need_redraw,
        runtime_presentation_intents,
        dispatch_outcome,
    )
}

pub fn startup_keymap_action_for_input(
    keymaps: &[crate::app::bootstrap::StartupKeymapSnapshot],
    mode: CoreMode,
    key: &KeyInput,
) -> Option<StartupKeymapAction> {
    let mode = startup_keymap_mode_from_core_mode(mode)?;
    let lhs = startup_keymap_lhs_from_input(key)?;
    startup_keymap_action_for_lhs(keymaps, mode, &lhs)
}

pub fn startup_keymap_action_for_snapshot_input(
    keymaps: &[crate::app::bootstrap::StartupKeymapSnapshot],
    snapshot: &vim_core_rs::CoreLightSnapshot,
    key: &KeyInput,
    pending_lhs: &mut Option<String>,
) -> Option<StartupKeymapAction> {
    let mode = startup_keymap_mode_from_core_mode(snapshot.mode)?;
    let key_lhs = startup_keymap_lhs_from_input(key)?;
    if let Some(prefix) = pending_lhs.take() {
        let lhs = format!("{prefix}{key_lhs}");
        if let Some(action) = startup_keymap_action_for_lhs(keymaps, mode, &lhs) {
            return Some(action);
        }
        if startup_keymap_has_longer_prefix(keymaps, mode, &lhs) {
            *pending_lhs = Some(lhs);
            return None;
        }
    }

    if snapshot.pending_input.pending_keys.is_empty() {
        if let Some(action) = startup_keymap_action_for_lhs(keymaps, mode, &key_lhs) {
            return Some(action);
        }
        if startup_keymap_has_longer_prefix(keymaps, mode, &key_lhs) {
            *pending_lhs = Some(key_lhs);
        }
        return None;
    }
    let pending_lhs = (!snapshot.pending_input.pending_keys.is_empty())
        .then(|| format!("{}{}", snapshot.pending_input.pending_keys, key_lhs));
    let direct_lhs = startup_keymap_lhs_from_input(key)?;

    [pending_lhs.as_deref(), Some(direct_lhs.as_str())]
        .into_iter()
        .flatten()
        .find_map(|lhs| {
            keymaps
                .iter()
                .rev()
                .find(|keymap| keymap.mode == mode && keymap.lhs == lhs)
                .map(|keymap| keymap.action.clone())
        })
}

pub fn startup_keymap_action_for_lhs(
    keymaps: &[crate::app::bootstrap::StartupKeymapSnapshot],
    mode: StartupKeymapMode,
    lhs: &str,
) -> Option<StartupKeymapAction> {
    keymaps
        .iter()
        .rev()
        .find(|keymap| keymap.mode == mode && keymap.lhs == lhs)
        .map(|keymap| keymap.action.clone())
}

pub fn startup_keymap_has_longer_prefix(
    keymaps: &[crate::app::bootstrap::StartupKeymapSnapshot],
    mode: StartupKeymapMode,
    lhs: &str,
) -> bool {
    keymaps
        .iter()
        .any(|keymap| keymap.mode == mode && keymap.lhs.starts_with(lhs) && keymap.lhs != lhs)
}

pub fn startup_keymap_mode_from_core_mode(mode: CoreMode) -> Option<StartupKeymapMode> {
    match mode {
        CoreMode::Insert => Some(StartupKeymapMode::Insert),
        CoreMode::Visual | CoreMode::VisualLine | CoreMode::VisualBlock => {
            Some(StartupKeymapMode::Visual)
        }
        CoreMode::Normal => Some(StartupKeymapMode::Normal),
        _ => None,
    }
}

pub fn startup_keymap_lhs_from_input(key: &KeyInput) -> Option<String> {
    match key {
        KeyInput::Char(ch) => Some(ch.to_string()),
        KeyInput::Ctrl(ch) => Some(format!("<C-{}>", ch.to_ascii_lowercase())),
        KeyInput::Tab => Some("<Tab>".to_string()),
        KeyInput::BackTab => Some("<S-Tab>".to_string()),
        KeyInput::Enter => Some("<Enter>".to_string()),
        KeyInput::Escape => Some("<Esc>".to_string()),
        KeyInput::Backspace => Some("<BS>".to_string()),
        _ => None,
    }
}
