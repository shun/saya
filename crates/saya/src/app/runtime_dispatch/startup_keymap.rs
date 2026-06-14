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

// ADR 0006 Phase 3: Phase 0 の止血（`resolve_startup_keymap_with_core_sync` /
// `StartupKeymapResolution`）は撤去した。入力ルーティングは完成判定を集約した
// 単一パイプライン（`resolve_pipeline_command_buffered`）へ一本化され、prefix を
// core へ先行送出して同期させる必要も、mapping 確定時に core 側 pending を ESC で
// 巻き戻す必要もなくなった。完成判定は core の非破壊予測器
// (`predict_input_completeness`) を予測関数として host から呼ぶ形で実現する。

pub fn startup_keymap_action_for_input(
    keymaps: &[crate::app::bootstrap::StartupKeymapSnapshot],
    mode: CoreMode,
    key: &KeyInput,
) -> Option<StartupKeymapAction> {
    let mode = startup_keymap_mode_from_core_mode(mode)?;
    let lhs = startup_keymap_lhs_from_input(key)?;
    startup_keymap_action_for_lhs(keymaps, mode, &lhs)
}

// ADR 0006 Phase 2/3 整理: legacy wrapper `startup_keymap_action_for_snapshot_input`
// （snapshot + host pending を受け、2 キー prefix を結合解決していた）は削除した。
// 本番の入力解決は完成判定を集約した単一パイプライン
// （`resolve_pipeline_command` / `resolve_pipeline_command_buffered`, command.rs）へ
// 一本化済みであり、唯一残っていた wrapper の本番呼び出し元（起動スモーク
// `run_binary_completion_smoke`）は単キー `<C-x>` を 1 回解決するだけだったため、
// 下位純粋関数 `startup_keymap_action_for_input` 直接利用へ移管した。
// 2 キー prefix 解決の本番回帰は `tests/integration_input_pipeline.rs` が担保する。
//
// 下位関数 `startup_keymap_action_for_lhs` / `startup_keymap_has_longer_prefix` /
// `startup_keymap_lhs_from_input` / `startup_keymap_mode_from_core_mode` は、現役の
// パイプライン本体 `resolve_pipeline_command`（command.rs）が依存しているため残す。

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

// ADR 0006 Phase 2/3 整理: 旧テスト module `adr0006_phase1_tests`
// （`keymap_resolution_ignores_core_pending_when_host_pending_is_empty`）は、削除した
// wrapper `startup_keymap_action_for_snapshot_input` 専用のため撤去した。
// その「core pending（`snapshot.pending_input.pending_keys`）を keymap 解決の決定源に
// しない」性質は、現役パイプライン `resolve_pipeline_command`（command.rs）が
// snapshot から `mode` のみを参照し core pending を一切読まない構造そのもので保証され、
// `tests/integration_input_pipeline.rs` の
// `pipeline_ignores_core_pending_when_host_pending_is_empty` が本番経路で明示検証する。

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
