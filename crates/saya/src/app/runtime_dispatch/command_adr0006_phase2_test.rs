use super::*;
use crate::app::bootstrap::{StartupKeymapAction, StartupKeymapMode, StartupKeymapSnapshot};
use crate::app::test_support::launch_serial_lock as session_test_lock;
use crate::core::bridge::CoreBridge;

fn normal_keymap(lhs: &str, action: StartupKeymapAction) -> StartupKeymapSnapshot {
    StartupKeymapSnapshot {
        mode: StartupKeymapMode::Normal,
        lhs: lhs.to_string(),
        action,
    }
}

/// pipeline をキー列で駆動し、確定した `Command` を順に集める最小ドライバ。
/// `BuiltinEdit` は core に dispatch、`HostCommand` は実行せず記録のみ。
/// `Passthrough` は core に dispatch する（count prefix 合成込み）。
///
/// 位置づけ: これは本番対話ループ（main.rs）の match 腕の「ミラー」であり、
/// `resolve_pipeline_command` / `resolve_pipeline_command_buffered` の純粋契約
/// （どのキー列がどの `Command` / 解決へ落ちるか）のみを検証する。本番ループへの
/// 実配線（イベント取得→classify→resolve→core 越境の継ぎ目）は検証しない。
/// ミラーが本番 match と一致している保証はこのテスト自身にはなく、実配線の担保は
/// 実バイナリ E2E マトリクス `tests/integration_input_pipeline_e2e.rs`
/// （Ctrl-f/b・dd・`:`・`/`・`:w` 等）に委ねる。
fn drive(
    keymaps: &[StartupKeymapSnapshot],
    bridge: &mut CoreBridge,
    keys: &[KeyInput],
    host_pending: &mut Option<String>,
    host_count: &mut Option<usize>,
) -> Vec<Command> {
    let mut commands = Vec::new();
    for key in keys {
        let snapshot = bridge.light_snapshot();
        let resolution =
            resolve_pipeline_command(keymaps, &snapshot, host_pending, host_count, key);
        match resolution {
            PipelineResolution::Command(command) => {
                match &command {
                    Command::BuiltinEdit(keys) => {
                        let _ = bridge.dispatch_key(keys);
                    }
                    Command::HostCommand { .. } => {
                        // 実行はテストでは記録のみ。
                    }
                }
                commands.push(command);
            }
            PipelineResolution::Passthrough(keys) => {
                let _ = bridge.dispatch_key(&keys);
            }
            PipelineResolution::PendingPrefix
            | PipelineResolution::CountAccumulated
            | PipelineResolution::Unhandled => {}
        }
    }
    commands
}

/// count×BuiltinEdit: `3gd`（`gd` -> Literal("j"）で 3 行下へ移動相当を確認。
/// host count=3 が mapping RHS のキー列 `j` に合成され `BuiltinEdit("3j")` になる。
#[test]
fn count_composes_with_builtin_mapping_3gd() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // gd -> Literal("j")（1 行下移動）。count=3 で 3 行下へ。
    let keymaps = vec![normal_keymap(
        "gd",
        StartupKeymapAction::Literal("j".to_string()),
    )];
    let mut bridge = CoreBridge::new("l0\nl1\nl2\nl3\nl4\nl5\n").expect("core bridge init");
    assert_eq!(bridge.snapshot().cursor_row, 0, "前提: カーソルは先頭行");

    let mut host_pending: Option<String> = None;
    let mut host_count: Option<usize> = None;
    let commands = drive(
        &keymaps,
        &mut bridge,
        &[
            KeyInput::Char('3'),
            KeyInput::Char('g'),
            KeyInput::Char('d'),
        ],
        &mut host_pending,
        &mut host_count,
    );

    assert_eq!(
        commands,
        vec![Command::BuiltinEdit("3j".to_string())],
        "count=3 が mapping RHS に合成され BuiltinEdit(\"3j\") になること"
    );
    assert_eq!(
        bridge.snapshot().cursor_row,
        3,
        "3gd で 3 行下（row 3）へ移動すること"
    );
    assert!(
        host_count.is_none(),
        "確定後は host count がクリアされること"
    );
    assert!(
        host_pending.is_none(),
        "確定後は host pending がクリアされること"
    );
}

/// count×builtin grammar（mapping 不在）: `2gg` は mapping に該当しないが、
/// `g` が grammar prefix なので pending -> `gg` で `BuiltinEdit("2gg")` 相当に
/// なり先頭行へジャンプする（count は passthrough に合成される）。
#[test]
fn count_composes_with_builtin_grammar_2gg() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // g 始まり mapping (gd) を登録しておく（g 単体は grammar prefix）。
    let keymaps = vec![normal_keymap(
        "gd",
        StartupKeymapAction::Literal("$".to_string()),
    )];
    let mut bridge = CoreBridge::new("l0\nl1\nl2\nl3\nl4\n").expect("core bridge init");
    for _ in 0..4 {
        let _ = bridge.dispatch_key("j");
    }
    assert_eq!(bridge.snapshot().cursor_row, 4, "前提: カーソルは最終行");

    let mut host_pending: Option<String> = None;
    let mut host_count: Option<usize> = None;
    let _ = drive(
        &keymaps,
        &mut bridge,
        &[
            KeyInput::Char('2'),
            KeyInput::Char('g'),
            KeyInput::Char('g'),
        ],
        &mut host_pending,
        &mut host_count,
    );

    // 2gg は 2 行目 (row 1) へジャンプする。
    assert_eq!(
        bridge.snapshot().cursor_row,
        1,
        "2gg で 2 行目（row 1）へジャンプすること"
    );
}

/// 曖昧解決の単一規則: `g`(grammar prefix) vs `gd`(mapping)。
/// `g` -> pending、`d` で `gd` mapping が確定する（builtin より mapping 優先）。
#[test]
fn ambiguity_gd_mapping_wins_over_g_grammar_when_completed() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let keymaps = vec![normal_keymap(
        "gd",
        StartupKeymapAction::Literal("G".to_string()),
    )];
    let mut bridge = CoreBridge::new("l0\nl1\nl2\nl3\n").expect("core bridge init");

    let mut host_pending: Option<String> = None;
    let mut host_count: Option<usize> = None;
    let commands = drive(
        &keymaps,
        &mut bridge,
        &[KeyInput::Char('g'), KeyInput::Char('d')],
        &mut host_pending,
        &mut host_count,
    );
    assert_eq!(
        commands,
        vec![Command::BuiltinEdit("G".to_string())],
        "gd mapping が確定して BuiltinEdit(\"G\") になること"
    );
}

/// RHS Command モデル化（HostCommand）: registered command が `Command::HostCommand`
/// として一様に解決されること（横取り特別経路を通らない）。
#[test]
fn registered_command_resolves_as_host_command() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let keymaps = vec![normal_keymap(
        "gd",
        StartupKeymapAction::RegisteredCommand("lsp.definition".to_string()),
    )];
    let mut bridge = CoreBridge::new("l0\nl1\n").expect("core bridge init");

    let mut host_pending: Option<String> = None;
    let mut host_count: Option<usize> = None;
    let commands = drive(
        &keymaps,
        &mut bridge,
        &[KeyInput::Char('g'), KeyInput::Char('d')],
        &mut host_pending,
        &mut host_count,
    );
    assert_eq!(
        commands,
        vec![Command::HostCommand {
            name: "lsp.definition".to_string(),
            count: None,
        }],
        "registered command が Command::HostCommand として一様に解決されること"
    );
}

/// count×HostCommand（案2）: `3<plugin>` で `Command::HostCommand{count:Some(3)}`
/// が型に載ること（実行未配線でよい）。
#[test]
fn count_is_carried_in_host_command_type() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // single-key mapping `q` -> registered command。
    let keymaps = vec![normal_keymap(
        "q",
        StartupKeymapAction::RegisteredCommand("plugin.run".to_string()),
    )];
    let mut bridge = CoreBridge::new("l0\nl1\n").expect("core bridge init");

    let mut host_pending: Option<String> = None;
    let mut host_count: Option<usize> = None;
    let commands = drive(
        &keymaps,
        &mut bridge,
        &[KeyInput::Char('3'), KeyInput::Char('q')],
        &mut host_pending,
        &mut host_count,
    );
    assert_eq!(
        commands,
        vec![Command::HostCommand {
            name: "plugin.run".to_string(),
            count: Some(3),
        }],
        "host count=3 が HostCommand の count に載ること（案2）"
    );
}

/// timeoutlen seam: 曖昧 prefix `g` を保持したまま timeout 発火で builtin 確定。
/// `g` mapping prefix(gd) 保持中に timeout -> core へ `g` を flush（passthrough）。
#[test]
fn timeout_flushes_ambiguous_prefix_as_builtin() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let keymaps = vec![normal_keymap(
        "gd",
        StartupKeymapAction::Literal("G".to_string()),
    )];
    let bridge = CoreBridge::new("l0\nl1\nl2\nl3\n").expect("core bridge init");

    let mut host_pending: Option<String> = None;
    let mut host_count: Option<usize> = None;

    // `g` 単打で pending prefix にする。
    let snapshot = bridge.light_snapshot();
    let r = resolve_pipeline_command(
        &keymaps,
        &snapshot,
        &mut host_pending,
        &mut host_count,
        &KeyInput::Char('g'),
    );
    assert_eq!(r, PipelineResolution::PendingPrefix, "g 単打は pending");
    assert_eq!(host_pending.as_deref(), Some("g"), "pending は g");

    // timeout 発火: g を builtin として core へ flush。
    let snapshot = bridge.light_snapshot();
    let timeout =
        resolve_pipeline_on_timeout(&keymaps, &snapshot, &mut host_pending, &mut host_count);
    assert_eq!(
        timeout,
        PipelineResolution::Passthrough("g".to_string()),
        "timeout 発火で pending prefix が builtin として flush されること"
    );
    assert!(
        host_pending.is_none(),
        "timeout 後は host pending がクリアされること"
    );
}

/// MappingRhs 正規化: StartupKeymapAction からの変換を担保。
#[test]
fn mapping_rhs_normalizes_from_startup_keymap_action() {
    assert_eq!(
        MappingRhs::from_startup_keymap_action(StartupKeymapAction::Literal("dd".to_string())),
        MappingRhs::Keys("dd".to_string()),
    );
    assert_eq!(
        MappingRhs::from_startup_keymap_action(StartupKeymapAction::RegisteredCommand(
            "x.y".to_string()
        )),
        MappingRhs::Command(Command::HostCommand {
            name: "x.y".to_string(),
            count: None,
        }),
    );
}
