//! ADR 0006 Phase 3: 完成コマンドのみが backend を越えること（部分入力は host
//! パイプラインに留め、core を呼ばない）を担保する。
//!
//! 完成度は core の非破壊予測器 `CoreBridge::classify_input_completeness` に委ねる。
//! backend 呼び出し回数は `CoreBridge::dispatch_key_count()` で観測する。
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

/// Phase 3 の host 入力パイプラインを最小再現するドライバ。
/// 各キーで `resolve_pipeline_command_buffered` を呼び、`DispatchComplete` のときだけ
/// core に dispatch する。`HoldPending` / `CountAccumulated` は backend を呼ばない。
///
/// 位置づけ: これは本番対話ループ（main.rs）の match 腕の「ミラー」であり、
/// `resolve_pipeline_command_buffered` の純粋契約（完成度判定により完成 Command
/// だけが core を越え、部分入力は host 側に留まること）のみを検証する。本番ループ
/// への実配線は検証しない。実配線の担保は実バイナリ E2E マトリクス
/// `tests/integration_input_pipeline_e2e.rs`（Ctrl-f/b・dd・`:`・`/`・`:w` 等）に委ねる。
fn drive_buffered(
    keymaps: &[StartupKeymapSnapshot],
    bridge: &mut CoreBridge,
    keys: &[KeyInput],
    host_pending: &mut Option<String>,
    host_count: &mut Option<usize>,
    host_passthrough: &mut String,
) -> Vec<Command> {
    let mut commands = Vec::new();
    for key in keys {
        let snapshot = bridge.light_snapshot();
        // classify は core への参照を借りないよう、その時点のモードでクロージャを作る。
        let mode = snapshot.mode;
        let classify = |seq: &str| vim_core_rs::predict_input_completeness(seq, mode);
        let resolution = resolve_pipeline_command_buffered(
            keymaps,
            &snapshot,
            host_pending,
            host_count,
            host_passthrough,
            key,
            classify,
        );
        match resolution {
            BufferedResolution::Command(command) => {
                if let Command::BuiltinEdit(keys) = &command {
                    let _ = bridge.dispatch_key(keys);
                }
                commands.push(command);
            }
            BufferedResolution::DispatchComplete(complete) => {
                let _ = bridge.dispatch_key(&complete);
            }
            BufferedResolution::HoldPending
            | BufferedResolution::CountAccumulated
            | BufferedResolution::Unhandled => {}
        }
    }
    commands
}

/// 部分入力 `g` 単体では backend を一切呼ばない（pending として host に留まる）。
/// 2 打目 `g` で初めて完成 `gg` が 1 回だけ dispatch され先頭行へジャンプする。
#[test]
fn partial_g_does_not_reach_backend_until_complete_gg() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // g 始まり mapping を登録（g 単体は grammar prefix でもある）。
    let keymaps = vec![normal_keymap(
        "gd",
        StartupKeymapAction::RegisteredCommand("lsp.definition".to_string()),
    )];
    let mut bridge = CoreBridge::new("l0\nl1\nl2\nl3\n").expect("core bridge init");
    for _ in 0..3 {
        let _ = bridge.dispatch_key("j");
    }
    assert_eq!(bridge.snapshot().cursor_row, 3, "前提: 最終行");
    let dispatch_before = bridge.dispatch_key_count();

    let mut host_pending: Option<String> = None;
    let mut host_count: Option<usize> = None;
    let mut host_passthrough = String::new();

    // 1 打目 `g`: keymap prefix pending なので backend は呼ばれない。
    let snapshot = bridge.light_snapshot();
    let mode = snapshot.mode;
    let r = resolve_pipeline_command_buffered(
        &keymaps,
        &snapshot,
        &mut host_pending,
        &mut host_count,
        &mut host_passthrough,
        &KeyInput::Char('g'),
        |seq: &str| vim_core_rs::predict_input_completeness(seq, mode),
    );
    assert_eq!(r, BufferedResolution::HoldPending, "g 単体は pending");
    assert_eq!(
        bridge.dispatch_key_count(),
        dispatch_before,
        "g 単体で backend (dispatch_key) を呼んではならない"
    );

    // 2 打目 `g`: `gd` mapping は完成せず `gg` を完成として core へ 1 回 flush。
    let snapshot = bridge.light_snapshot();
    let mode = snapshot.mode;
    let r = resolve_pipeline_command_buffered(
        &keymaps,
        &snapshot,
        &mut host_pending,
        &mut host_count,
        &mut host_passthrough,
        &KeyInput::Char('g'),
        |seq: &str| vim_core_rs::predict_input_completeness(seq, mode),
    );
    assert_eq!(
        r,
        BufferedResolution::DispatchComplete("gg".to_string()),
        "2 打目 g で完成 gg を 1 回 dispatch"
    );
    let _ = bridge.dispatch_key("gg");
    assert_eq!(
        bridge.dispatch_key_count(),
        dispatch_before + 1,
        "完成時に backend へ 1 回だけ dispatch する"
    );
    assert_eq!(bridge.snapshot().cursor_row, 0, "gg で先頭行へジャンプ");
}

/// operator 部分入力 `d`（motion 待ち）は backend を呼ばず host に留まり、
/// `d`,`w` 完成で `dw` を 1 回だけ dispatch する。
#[test]
fn partial_operator_d_holds_until_motion_completes_dw() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let keymaps: Vec<StartupKeymapSnapshot> = vec![];
    let mut bridge = CoreBridge::new("hello world\n").expect("core bridge init");
    let dispatch_before = bridge.dispatch_key_count();

    let mut host_pending: Option<String> = None;
    let mut host_count: Option<usize> = None;
    let mut host_passthrough = String::new();

    // `d`: operator motion 待ち。予測 Pending、backend 未呼び出し。
    let snapshot = bridge.light_snapshot();
    let mode = snapshot.mode;
    let r = resolve_pipeline_command_buffered(
        &keymaps,
        &snapshot,
        &mut host_pending,
        &mut host_count,
        &mut host_passthrough,
        &KeyInput::Char('d'),
        |seq: &str| vim_core_rs::predict_input_completeness(seq, mode),
    );
    assert_eq!(
        r,
        BufferedResolution::HoldPending,
        "d は motion 待ちで pending"
    );
    assert_eq!(
        bridge.dispatch_key_count(),
        dispatch_before,
        "d 単体で backend を呼んではならない"
    );
    assert_eq!(host_passthrough, "d", "d は host バッファに留まる");

    // `w`: dw 完成。1 回だけ dispatch。
    let snapshot = bridge.light_snapshot();
    let mode = snapshot.mode;
    let r = resolve_pipeline_command_buffered(
        &keymaps,
        &snapshot,
        &mut host_pending,
        &mut host_count,
        &mut host_passthrough,
        &KeyInput::Char('w'),
        |seq: &str| vim_core_rs::predict_input_completeness(seq, mode),
    );
    assert_eq!(
        r,
        BufferedResolution::DispatchComplete("dw".to_string()),
        "dw が完成して 1 回 dispatch"
    );
    let _ = bridge.dispatch_key("dw");
    assert_eq!(
        bridge.dispatch_key_count(),
        dispatch_before + 1,
        "完成時に backend へ 1 回だけ dispatch"
    );
    assert_eq!(
        bridge.snapshot().text.trim_end(),
        "world",
        "dw で先頭ワードが削除される"
    );
}

/// keymap prefix + count の部分入力（`3`,`g`,`d`）は backend を呼ばず、
/// 完成時に `BuiltinEdit("3...")` として 1 度だけ解決される。
#[test]
fn count_prefix_keymap_partial_does_not_reach_backend() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let keymaps = vec![normal_keymap(
        "gd",
        StartupKeymapAction::Literal("j".to_string()),
    )];
    let mut bridge = CoreBridge::new("l0\nl1\nl2\nl3\nl4\nl5\n").expect("core bridge init");
    let dispatch_before = bridge.dispatch_key_count();

    let mut host_pending: Option<String> = None;
    let mut host_count: Option<usize> = None;
    let mut host_passthrough = String::new();

    // `3`: count 蓄積、backend 未呼び出し。
    let snapshot = bridge.light_snapshot();
    let mode = snapshot.mode;
    let r3 = resolve_pipeline_command_buffered(
        &keymaps,
        &snapshot,
        &mut host_pending,
        &mut host_count,
        &mut host_passthrough,
        &KeyInput::Char('3'),
        |seq: &str| vim_core_rs::predict_input_completeness(seq, mode),
    );
    assert_eq!(r3, BufferedResolution::CountAccumulated, "3 は count 蓄積");
    assert_eq!(
        bridge.dispatch_key_count(),
        dispatch_before,
        "count digit で backend を呼んではならない"
    );

    // `g`: keymap prefix pending、backend 未呼び出し。
    let snapshot = bridge.light_snapshot();
    let mode = snapshot.mode;
    let rg = resolve_pipeline_command_buffered(
        &keymaps,
        &snapshot,
        &mut host_pending,
        &mut host_count,
        &mut host_passthrough,
        &KeyInput::Char('g'),
        |seq: &str| vim_core_rs::predict_input_completeness(seq, mode),
    );
    assert_eq!(
        rg,
        BufferedResolution::HoldPending,
        "g は keymap prefix pending"
    );
    assert_eq!(
        bridge.dispatch_key_count(),
        dispatch_before,
        "keymap prefix pending で backend を呼んではならない"
    );

    // `d`: gd mapping 完成 + count=3 -> BuiltinEdit("3j")。
    let commands = drive_buffered(
        &keymaps,
        &mut bridge,
        &[KeyInput::Char('d')],
        &mut host_pending,
        &mut host_count,
        &mut host_passthrough,
    );
    assert_eq!(
        commands,
        vec![Command::BuiltinEdit("3j".to_string())],
        "3gd が BuiltinEdit(\"3j\") に解決される"
    );
    assert_eq!(bridge.snapshot().cursor_row, 3, "3gd で row 3 へ移動");
}

/// 退行なし: `g`,`g` -> row0 / `g`,`d` -> mapping 発火 / count 合成（buffered 経由）。
#[test]
fn regression_gg_and_gd_and_count_via_buffered() {
    let _lock = session_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    // g,g -> row0
    {
        let keymaps = vec![normal_keymap(
            "gd",
            StartupKeymapAction::RegisteredCommand("lsp.definition".to_string()),
        )];
        let mut bridge = CoreBridge::new("l0\nl1\nl2\nl3\n").expect("init");
        for _ in 0..3 {
            let _ = bridge.dispatch_key("j");
        }
        let mut hp = None;
        let mut hc = None;
        let mut pb = String::new();
        let _ = drive_buffered(
            &keymaps,
            &mut bridge,
            &[KeyInput::Char('g'), KeyInput::Char('g')],
            &mut hp,
            &mut hc,
            &mut pb,
        );
        assert_eq!(bridge.snapshot().cursor_row, 0, "g,g -> row0");
    }

    // g,d -> mapping 発火 (Literal("G") で最終行へ)
    {
        let keymaps = vec![normal_keymap(
            "gd",
            StartupKeymapAction::Literal("G".to_string()),
        )];
        let mut bridge = CoreBridge::new("l0\nl1\nl2\nl3\n").expect("init");
        let mut hp = None;
        let mut hc = None;
        let mut pb = String::new();
        let commands = drive_buffered(
            &keymaps,
            &mut bridge,
            &[KeyInput::Char('g'), KeyInput::Char('d')],
            &mut hp,
            &mut hc,
            &mut pb,
        );
        assert_eq!(
            commands,
            vec![Command::BuiltinEdit("G".to_string())],
            "g,d -> mapping 発火"
        );
        assert_eq!(bridge.snapshot().cursor_row, 3, "G で最終行へ");
    }

    // 2,g,g -> row1 (count 合成 + grammar)
    {
        let keymaps = vec![normal_keymap(
            "gd",
            StartupKeymapAction::Literal("$".to_string()),
        )];
        let mut bridge = CoreBridge::new("l0\nl1\nl2\nl3\nl4\n").expect("init");
        for _ in 0..4 {
            let _ = bridge.dispatch_key("j");
        }
        let mut hp = None;
        let mut hc = None;
        let mut pb = String::new();
        let _ = drive_buffered(
            &keymaps,
            &mut bridge,
            &[
                KeyInput::Char('2'),
                KeyInput::Char('g'),
                KeyInput::Char('g'),
            ],
            &mut hp,
            &mut hc,
            &mut pb,
        );
        assert_eq!(bridge.snapshot().cursor_row, 1, "2gg -> row1");
    }
}
