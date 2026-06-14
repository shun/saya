//! 統合テスト: ADR 0006 入力単一パイプラインの「本番経路」回帰。
//!
//! 背景:
//! ADR 0006 Phase 3 で入力解決は `resolve_pipeline_command_buffered`
//! （`crates/saya/src/app/runtime_dispatch/command.rs`）へ一本化された。
//! main.rs:708 の本番対話ループはこの関数を「呼ぶだけ」の薄い層であり、
//! - 完成判定の classify は `core_bridge.light_snapshot().mode` から作った
//!   `vim_core_rs::predict_input_completeness` クロージャ、
//! - backend（core）への越境は `BufferedResolution::DispatchComplete` /
//!   `Command::BuiltinEdit`（完成キー列）の場合のみ `core_bridge.dispatch_key`、
//! - `Command::HostCommand` は registered command として確定し core を越えない、
//! という規約で動く。
//!
//! このテストは、その本番経路と「同一の呼び出し形・同一の classify・同一の越境規約」
//! を再現するハーネス（`drive_production_pipeline`）を通してキーストロークを流し、
//! observable な信号（cursor row / HostCommand 確定 / backend へ完成キーのみ越境）で
//! - `g`,`g` -> cursor row=0（ADR の本丸 `gg` 不具合の本番経路回帰）
//! - `g`,`d` -> registered command（`HostCommand`）が完成 Command として確定して発火
//! を検証する。
//!
//! 旧 `startup_keymap_action_for_snapshot_input` の 2 キー解決は本番未使用
//! （単キー `<C-x>` スモークのみ）であり、`g` 始まり keymap の 2 キー解決の
//! 本番保証はこのパイプラインテストが担う。
//!
//! hermetic: 実 `~/.config/saya/init.ts` を一切読まない。keymaps はテスト内で
//! 明示構築し、`CoreBridge` はインメモリのテキストで初期化する。

use std::sync::MutexGuard;

use saya::app::bootstrap::{
    StartupKeymapAction, StartupKeymapMode, StartupKeymapSnapshot, launch_test_lock,
};
use saya::app::runtime_dispatch::{
    BufferedResolution, Command, resolve_pipeline_command_buffered,
};
use saya::core::bridge::CoreBridge;
use saya::input::router::KeyInput;

/// `CoreBridge::new`（core セッション確保）はプロセス内で排他が必要なため、
/// 既存 integration テスト（core_host_actions_contract.rs）と同じ `launch_test_lock`
/// で直列化する。
fn test_lock() -> MutexGuard<'static, ()> {
    launch_test_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Normal モードの keymap スナップショットを作る最小ヘルパー。
fn normal_keymap(lhs: &str, action: StartupKeymapAction) -> StartupKeymapSnapshot {
    StartupKeymapSnapshot {
        mode: StartupKeymapMode::Normal,
        lhs: lhs.to_string(),
        action,
    }
}

/// 本番対話ループ（main.rs:705-898）と同一の呼び出し形・classify・越境規約を
/// 再現してキー列を流すハーネス。
///
/// 各キーで:
/// 1. `core_bridge.light_snapshot()` を取り、その `mode` で classify を作る
///    （本番 main.rs:706-717 と同一）。
/// 2. `resolve_pipeline_command_buffered` を本番と同じ引数で呼ぶ。
/// 3. `BufferedResolution` を本番 match 各腕と同じ規約で処理する:
///    - `Command(BuiltinEdit(rhs))` / `DispatchComplete(keys)` -> core へ 1 回 dispatch
///      （本番の `dispatch_complete_keys_to_core` 相当の越境）。
///    - `Command(HostCommand { name, count })` -> registered command として確定し
///      記録のみ（core を越えない。本番の `execute_startup_keymap_registered_command`
///      に対応する確定点）。
///    - `HoldPending` / `CountAccumulated` -> backend を呼ばない。
///
/// 戻り値: 確定した `HostCommand` 名と、core への完成 dispatch 列。
struct ProductionPipelineTrace {
    host_commands: Vec<(String, Option<usize>)>,
    dispatched_to_core: Vec<String>,
}

fn drive_production_pipeline(
    keymaps: &[StartupKeymapSnapshot],
    bridge: &mut CoreBridge,
    keys: &[KeyInput],
) -> ProductionPipelineTrace {
    // 本番ループの持ち回り状態（main.rs の startup_keymap_pending_lhs / host_count /
    // host_passthrough）に対応するローカル状態。
    let mut host_pending: Option<String> = None;
    let mut host_count: Option<usize> = None;
    let mut host_passthrough = String::new();

    let mut host_commands = Vec::new();
    let mut dispatched_to_core = Vec::new();

    for key in keys {
        // 本番 main.rs:705-717 と同一: light_snapshot の mode で classify を作る。
        let snapshot = bridge.light_snapshot();
        let predict_mode = snapshot.mode;
        let resolution = resolve_pipeline_command_buffered(
            keymaps,
            &snapshot,
            &mut host_pending,
            &mut host_count,
            &mut host_passthrough,
            key,
            |seq: &str| vim_core_rs::predict_input_completeness(seq, predict_mode),
        );

        match resolution {
            BufferedResolution::Command(Command::BuiltinEdit(rhs)) => {
                // 本番: dispatch_complete_keys_to_core で core へ越境。
                let _ = bridge.dispatch_key(&rhs);
                dispatched_to_core.push(rhs);
            }
            BufferedResolution::Command(Command::HostCommand { name, count }) => {
                // 本番: execute_startup_keymap_registered_command。core を越えない。
                host_commands.push((name, count));
            }
            BufferedResolution::DispatchComplete(complete_keys) => {
                // 本番: dispatch_complete_keys_to_core で完成キー列を core へ 1 回越境。
                let _ = bridge.dispatch_key(&complete_keys);
                dispatched_to_core.push(complete_keys);
            }
            BufferedResolution::HoldPending | BufferedResolution::CountAccumulated => {
                // 本番: 部分入力は host に留め backend を呼ばない。
            }
            BufferedResolution::Unhandled => {
                // 本番: 従来経路へフォールバック。このテストの keymap/キーでは到達しない。
                panic!("unexpected Unhandled in production pipeline regression: key={key:?}");
            }
        }
    }

    ProductionPipelineTrace {
        host_commands,
        dispatched_to_core,
    }
}

/// ADR 本丸の `gg` 不具合の本番経路回帰:
/// `g` 始まり keymap（`gd` -> registered command）登録下で `g`,`g` を本番経路に
/// 流すと、`g` 単打では backend を呼ばず（pending）、2 打目で完成 `gg` が core へ
/// 1 回だけ越境し cursor row=0（先頭行）へジャンプする。
#[test]
fn production_pipeline_gg_jumps_to_row0_with_g_prefixed_keymap() {
    let _lock = test_lock();
    let keymaps = vec![normal_keymap(
        "gd",
        StartupKeymapAction::RegisteredCommand("lsp.definition".to_string()),
    )];
    let mut bridge = CoreBridge::new("l0\nl1\nl2\nl3\n").expect("core bridge init");
    // 末尾行へ移動しておく（gg で row0 へ戻ることを観測可能にする）。
    for _ in 0..3 {
        let _ = bridge.dispatch_key("j");
    }
    assert_eq!(bridge.snapshot().cursor_row, 3, "前提: カーソルは最終行 row3");
    let dispatch_before = bridge.dispatch_key_count();

    let trace = drive_production_pipeline(
        &keymaps,
        &mut bridge,
        &[KeyInput::Char('g'), KeyInput::Char('g')],
    );

    // observable: cursor が先頭行へ。
    assert_eq!(
        bridge.snapshot().cursor_row,
        0,
        "g,g で先頭行 row0 へジャンプすること（ADR gg 不具合の本番経路回帰）"
    );
    // observable: backend へ越境したのは完成 `gg` の 1 回だけ。
    assert_eq!(
        trace.dispatched_to_core,
        vec!["gg".to_string()],
        "完成キー列 gg のみが backend へ越境すること"
    );
    assert_eq!(
        bridge.dispatch_key_count(),
        dispatch_before + 1,
        "g,g で backend dispatch は 1 回だけ（g 単打では越境しない）"
    );
    // registered command は発火していない。
    assert!(
        trace.host_commands.is_empty(),
        "g,g では HostCommand を発火しないこと"
    );
}

/// `g`,`d` で registered command（`HostCommand`）が完成 Command として確定して発火し、
/// backend へは越境しない（registered command は core を越えない本番規約）。
#[test]
fn production_pipeline_gd_resolves_registered_host_command() {
    let _lock = test_lock();
    let keymaps = vec![normal_keymap(
        "gd",
        StartupKeymapAction::RegisteredCommand("lsp.definition".to_string()),
    )];
    let mut bridge = CoreBridge::new("l0\nl1\nl2\nl3\n").expect("core bridge init");
    let dispatch_before = bridge.dispatch_key_count();

    let trace = drive_production_pipeline(
        &keymaps,
        &mut bridge,
        &[KeyInput::Char('g'), KeyInput::Char('d')],
    );

    // observable: HostCommand が確定して発火する。
    assert_eq!(
        trace.host_commands,
        vec![("lsp.definition".to_string(), None)],
        "g,d で registered command が HostCommand として確定して発火すること"
    );
    // observable: registered command は backend を越えない。
    assert!(
        trace.dispatched_to_core.is_empty(),
        "registered command 確定時に backend へ完成キーを越境させないこと"
    );
    assert_eq!(
        bridge.dispatch_key_count(),
        dispatch_before,
        "g,d では backend dispatch を一切行わないこと"
    );
}

/// ADR 0006 Phase 2/3: 削除した wrapper `startup_keymap_action_for_snapshot_input` 専用の
/// 旧ユニットテスト `keymap_resolution_ignores_core_pending_when_host_pending_is_empty`
/// と等価な性質を、本番経路（`resolve_pipeline_command_buffered`）で担保する移植版。
///
/// keymap 解決の決定源は host pending（`host_pending`）単一であり、core 側の予測 pending
/// （`snapshot.pending_input.pending_keys`）は一切参照しない。これを観測するため:
/// 1. `gd` -> registered command を登録する。
/// 2. core だけを直接 `g` で pending 状態にする（host pending は空のまま）。
/// 3. host pending 空のまま 2 打目 `d` をパイプラインへ流す。
///
/// もし pipeline が core pending を参照すれば `gd` が完成して HostCommand が発火するが、
/// host 単一決定源であればそれは起きない（HostCommand 無し）。
#[test]
fn pipeline_ignores_core_pending_when_host_pending_is_empty() {
    let _lock = test_lock();
    let keymaps = vec![normal_keymap(
        "gd",
        StartupKeymapAction::RegisteredCommand("lsp.definition".to_string()),
    )];
    let mut bridge = CoreBridge::new("l0\nl1\nl2\nl3\n").expect("core bridge init");

    // core だけを pending 状態にする（host pending は touch しない）。
    let _ = bridge.dispatch_key("g");
    assert_eq!(
        bridge.light_snapshot().pending_input.pending_keys,
        "g",
        "前提: core 側は g で pending 状態になっていること"
    );

    // host pending は空のまま 2 打目 d だけを本番経路へ流す。
    let trace = drive_production_pipeline(&keymaps, &mut bridge, &[KeyInput::Char('d')]);

    assert!(
        trace.host_commands.is_empty(),
        "host pending が空なら core pending を参照して gd mapping を発火させてはならない \
         (決定源は host 単一)"
    );
}
