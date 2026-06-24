//! ADR 0006 Phase 2: モーダル入力を単一パイプラインで解決するための `Command`
//! モデルと、mapping 解決を入力パイプラインへ統合する純粋関数群。
//!
//! 背景:
//! 従来 `saya` は mapping を 2 段で解決していた。
//! - 純粋関数 `startup_keymap_action_for_snapshot_input`（決定源は host pending 単一）
//! - 結合 seam `resolve_startup_keymap_with_core_sync`（CoreBridge + host pending 結合）
//!
//! そして main.rs では mapping RHS が `StartupKeymapAction::Literal`（キー列）か
//! `StartupKeymapAction::RegisteredCommand`（プラグインコマンド）かで実行経路が
//! 二分し、後者はプラグインコマンドの「横取り」特別経路を通っていた。
//!
//! Phase 2 では mapping RHS を第一級の `Command` 型でモデル化する。
//! - `Command::BuiltinEdit(intent)`: core にキー列として dispatch する編集コマンド
//! - `Command::HostCommand { name, count }`: ホスト（TypeScript/プラグイン）コマンド
//!
//! mapping RHS は「キー列を産む」か「`Command` を産む」かのいずれかであり、
//! `MappingRhs` で表現する。pipeline 解決関数はこれらを一様に `Command` へ畳み込み、
//! count を host 側で合成する。これにより「横取り」特別経路は不要になり、main.rs は
//! 解決済み `Command` の variant だけで分岐できる。
//!
//! 設計上の注意（ADR 0006 確定方針, 案A）:
//! - 本モジュールは解決層の純粋関数に集約する。実行の越境厳密化（完成コマンドのみが
//!   backend を越える）は Phase 3 に委譲する。
//! - count×BuiltinEdit（`3gd` / `2gg`）は完全実装する。
//! - count×HostCommand は案2を採用し、`Command::HostCommand { name, count }` の型に
//!   count を載せるが、実行側への配線は Phase 2 では行わない（count はログのみ）。

use crate::app::bootstrap::{StartupKeymapAction, StartupKeymapSnapshot};

use super::{
    startup_keymap_action_for_lhs, startup_keymap_has_longer_prefix, startup_keymap_lhs_from_input,
    startup_keymap_lhs_notation_to_core_keys, startup_keymap_mode_from_core_mode,
};
use crate::input::router::KeyInput;
use vim_core_rs::CoreLightSnapshot;

/// モーダル入力パイプラインが解決する第一級コマンド。
///
/// ADR 0006 の `Command = BuiltinEdit(intent) | HostCommand(name, args)` に対応する。
/// Rust では type エイリアス規約（TS 専用）の対象外なので enum で表現する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// core（vim-core-rs）にキー列として dispatch する編集コマンド。
    /// 例: `"gg"`, `"3gd"`, `"dd"`。count は文字列 prefix として合成済み。
    BuiltinEdit(String),
    /// ホスト（TypeScript/プラグイン）側で実行する登録コマンド。
    /// `count` は ADR 0006 案2 に従い型に載せるが、Phase 2 では実行未配線。
    HostCommand { name: String, count: Option<usize> },
}

/// mapping RHS の正規化表現。
///
/// mapping の右辺は「キー列を産む」か「`Command` を産む」かのいずれか。
/// `StartupKeymapAction` からの変換を担い、pipeline はここから `Command` を畳み込む。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MappingRhs {
    /// キー列（例 `Literal("G")`）。count を合成して `BuiltinEdit` になる。
    Keys(String),
    /// 既に確定したコマンド（主に `HostCommand`）。
    Command(Command),
}

impl MappingRhs {
    /// bootstrap 層の `StartupKeymapAction` を pipeline の `MappingRhs` へ正規化する。
    ///
    /// - `Literal(keys)` -> `MappingRhs::Keys(keys)`
    /// - `RegisteredCommand(name)` -> `MappingRhs::Command(HostCommand { name, count: None })`
    pub fn from_startup_keymap_action(action: StartupKeymapAction) -> Self {
        match action {
            StartupKeymapAction::Literal(keys) => MappingRhs::Keys(keys),
            StartupKeymapAction::RegisteredCommand(name) => {
                MappingRhs::Command(Command::HostCommand { name, count: None })
            }
        }
    }
}

/// 入力パイプライン 1 ステップの解決結果。
///
/// 単一の typeahead 上で mapping と modal 文法を合成した結果を表す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineResolution {
    /// `Command` が確定した。呼び出し側は variant で分岐して実行する。
    Command(Command),
    /// より長い prefix が存在し pending が継続する。キーは consume 済み。
    PendingPrefix,
    /// digit を host count に取り込んだ。キーは consume 済み（core へは流さない）。
    CountAccumulated,
    /// mapping に該当せず、このキー列を core（EditKey）へ素通しする。
    Passthrough(String),
    /// このキーは pipeline では扱えない（lhs 表現不能）。呼び出し側の従来処理へ。
    Unhandled,
}

/// count digit かどうかを判定する。
///
/// vim の count は先頭 `0` を count として扱わない（`0` は行頭移動コマンド）。
/// よって count が空のときの `0` は count digit ではない。
fn digit_value_for_count(key: &KeyInput, has_pending_count: bool) -> Option<usize> {
    match key {
        KeyInput::Char(ch) if ch.is_ascii_digit() => {
            let value = (*ch as u8 - b'0') as usize;
            if value == 0 && !has_pending_count {
                // 先頭の `0` は count ではなくコマンド。
                None
            } else {
                Some(value)
            }
        }
        _ => None,
    }
}

/// 蓄積中の count に新しい桁を合成する。
fn accumulate_count(current: Option<usize>, digit: usize) -> usize {
    current
        .unwrap_or(0)
        .saturating_mul(10)
        .saturating_add(digit)
}

/// 解決した `MappingRhs` に host count を合成して最終 `Command` を作る。
///
/// - `Keys(keys)` + count=Some(n) -> `BuiltinEdit("{n}{keys}")`（例 `3` + `gd` -> `3gd`）
/// - `Keys(keys)` + count=None    -> `BuiltinEdit(keys)`
/// - `Command(HostCommand { name, .. })` + count -> `HostCommand { name, count }`
///   （案2: 型に count を載せる。実行配線は Phase 3+）
/// - `Command(BuiltinEdit(..))` は通常生成されないが、防御的に count を前置する。
pub fn compose_command_with_count(rhs: MappingRhs, count: Option<usize>) -> Command {
    match rhs {
        MappingRhs::Keys(keys) => {
            let composed = match count {
                Some(n) => format!("{n}{keys}"),
                None => keys,
            };
            Command::BuiltinEdit(composed)
        }
        MappingRhs::Command(Command::HostCommand {
            name,
            count: rhs_count,
        }) => {
            let effective = count.or(rhs_count);
            log::debug!(
                "[main][pipeline] host command resolved with count (case 2: count carried in type, execution unwired in phase 2): name={name}, host_count={count:?}, rhs_count={rhs_count:?}, effective={effective:?}"
            );
            Command::HostCommand {
                name,
                count: effective,
            }
        }
        MappingRhs::Command(Command::BuiltinEdit(keys)) => {
            let composed = match count {
                Some(n) => format!("{n}{keys}"),
                None => keys,
            };
            Command::BuiltinEdit(composed)
        }
    }
}

// ADR 0006 Phase 3: `command_from_startup_keymap_action`（Phase 2 で main.rs が
// mapping action を `Command` に昇格するために使った legacy seam）は撤去した。
// 入力ルーティングは `resolve_pipeline_command_buffered` へ一本化され、mapping 解決と
// count 合成・完成判定をパイプライン内で完結させる。

/// ADR 0006 Phase 2: mapping 解決を入力パイプラインへ統合した純粋解決関数。
///
/// 単一の host pending（`host_pending`）と host count（`host_count`）の上で mapping と
/// modal 文法を合成し、`BuiltinEdit` / `HostCommand` を一様に `PipelineResolution` で返す。
///
/// 振る舞い:
/// 1. count digit は host count に蓄積し `CountAccumulated` を返す（core へ流さない）。
///    ただし pending prefix 保持中は count 蓄積より prefix 合成を優先する。
/// 2. host pending が prefix を保持していれば、それと今回キーを結合して mapping 解決。
///    - 完成 -> count を合成して `Command`。
///    - より長い prefix あり -> `PendingPrefix`。
///    - 不成立 -> prefix を消費して単打判定へフォールスルー。
/// 3. 単打で mapping 完成 -> count 合成して `Command`。
/// 4. より長い prefix あり -> host pending にセットして `PendingPrefix`。
/// 5. いずれにも該当しない -> count を prefix として合成しつつ `Passthrough`。
pub fn resolve_pipeline_command(
    keymaps: &[StartupKeymapSnapshot],
    snapshot: &CoreLightSnapshot,
    host_pending: &mut Option<String>,
    host_count: &mut Option<usize>,
    key: &KeyInput,
) -> PipelineResolution {
    let Some(mode) = startup_keymap_mode_from_core_mode(snapshot.mode) else {
        log::debug!(
            "[main][pipeline] mode not mappable, unhandled: core_mode={:?}",
            snapshot.mode
        );
        return PipelineResolution::Unhandled;
    };
    let Some(key_lhs) = startup_keymap_lhs_from_input(key) else {
        log::debug!("[main][pipeline] key has no lhs representation, unhandled: key={key:?}");
        return PipelineResolution::Unhandled;
    };

    // (1) count digit の蓄積。pending prefix 保持中は prefix 合成を優先するため除外。
    if host_pending.is_none() {
        if let Some(digit) = digit_value_for_count(key, host_count.is_some()) {
            let next = accumulate_count(*host_count, digit);
            log::debug!(
                "[main][pipeline] count accumulated: digit={digit}, prev={host_count:?}, next={next}"
            );
            *host_count = Some(next);
            return PipelineResolution::CountAccumulated;
        }
        log::trace!(
            "[main][pipeline] key is not a count digit (no pending): key={key:?}, host_count={host_count:?}"
        );
    }

    // (2) host pending prefix を保持している場合の結合解決。
    if let Some(prefix) = host_pending.take() {
        let lhs = format!("{prefix}{key_lhs}");
        log::debug!(
            "[main][pipeline] resolving with host pending prefix: prefix={prefix:?}, key_lhs={key_lhs:?}, lhs={lhs:?}, host_count={host_count:?}"
        );
        if let Some(action) = startup_keymap_action_for_lhs(keymaps, mode, &lhs) {
            let rhs = MappingRhs::from_startup_keymap_action(action);
            let count = host_count.take();
            let command = compose_command_with_count(rhs, count);
            log::debug!(
                "[main][pipeline] host pending prefix completed a mapping: lhs={lhs:?}, command={command:?}"
            );
            return PipelineResolution::Command(command);
        }
        if startup_keymap_has_longer_prefix(keymaps, mode, &lhs) {
            log::debug!("[main][pipeline] host pending prefix extended: lhs={lhs:?}");
            *host_pending = Some(lhs);
            return PipelineResolution::PendingPrefix;
        }
        // 結合 lhs が mapping を完成せず、より長い mapping prefix も無い。
        // この場合、握っていた prefix と今回キーの結合（例 `g`+`g`=`gg`）は builtin
        // grammar 側のコマンド（例 `gg` で先頭行ジャンプ）である可能性が高い。
        // 旧実装は prefix を捨てて最新キーのみで再判定していたが、それでは
        // `gg` の 1 打目 `g` が失われ count も落ちる（ADR 0006 が指摘する gg 欠陥）。
        // ここでは結合 lhs 全体を count を合成して core へ flush する。
        let count = host_count.take();
        // ADR 0006 回帰修正・A-1: lhs は keymap 照合用の Vim notation（例 `<C-f>`）であり、
        // core へ素通しする実キー文字列ではない。notation を実キー（制御コード等）へ
        // 復元してから flush する。Char のみで構成される combined lhs（例 `gg`）は
        // 変換しても不変なので従来挙動を壊さない。
        let core_keys = startup_keymap_lhs_notation_to_core_keys(&lhs);
        let flushed = match count {
            Some(n) => format!("{n}{core_keys}"),
            None => core_keys.clone(),
        };
        log::debug!(
            "[main][pipeline] host pending prefix did not complete a mapping; flush combined lhs to core as builtin: combined_lhs={lhs:?}, core_keys={core_keys:?}, composed_count={count:?}, flushed={flushed:?}"
        );
        return PipelineResolution::Passthrough(flushed);
    }

    // (3) 単打で mapping 完成。
    if let Some(action) = startup_keymap_action_for_lhs(keymaps, mode, &key_lhs) {
        let rhs = MappingRhs::from_startup_keymap_action(action);
        let count = host_count.take();
        let command = compose_command_with_count(rhs, count);
        log::debug!(
            "[main][pipeline] single-key resolved a mapping: key_lhs={key_lhs:?}, command={command:?}"
        );
        return PipelineResolution::Command(command);
    }

    // (4) より長い prefix があれば pending を延長。
    if startup_keymap_has_longer_prefix(keymaps, mode, &key_lhs) {
        log::debug!("[main][pipeline] single-key set new host pending prefix: key_lhs={key_lhs:?}");
        *host_pending = Some(key_lhs);
        return PipelineResolution::PendingPrefix;
    }

    // (5) 素通し。count を prefix として合成して core へ流す。
    // ADR 0006 回帰修正・A-1: key_lhs は keymap 照合用 notation（例 Ctrl-f なら `<C-f>`）。
    // core は実キー文字列（Ctrl-f なら ASCII 制御コード `\u{6}`）を解釈するため、notation を
    // 実キーへ復元してから渡す。これを怠ると core が `<C-f>` を Ctrl-f と認識できず no-op に
    // なる（Ctrl-f/Ctrl-b/Ctrl-d/Ctrl-u のページ送り回帰）。Char キーは変換不変。
    let core_keys = startup_keymap_lhs_notation_to_core_keys(&key_lhs);
    let count = host_count.take();
    let passthrough = match count {
        Some(n) => format!("{n}{core_keys}"),
        None => core_keys,
    };
    log::debug!(
        "[main][pipeline] passthrough to core: key_lhs={key_lhs:?}, passthrough={passthrough:?}, composed_count={count:?}"
    );
    PipelineResolution::Passthrough(passthrough)
}

/// ADR 0006 Phase 2: timeoutlen 発火時の解決を注入可能 seam として表現する純粋関数。
///
/// 曖昧 prefix（例: `g` で `gd` mapping が存在し、かつ `g` 自体が builtin grammar の
/// prefix でもある）を保持したまま timeout が発火したとき、pending を builtin として
/// 確定（core へ flush）する。これにより `timeoutlen` 経過後に `g` 単体が builtin
/// grammar 側へ確定する挙動を決定的にテストできる。
///
/// 実時間タイマの main.rs 配線は Phase 3 に委譲する（本関数は seam のみ）。
///
/// 戻り値:
/// - 保持していた host pending を builtin として `Passthrough` で返す（core flush）。
/// - pending が無ければ `Unhandled`。
pub fn resolve_pipeline_on_timeout(
    _keymaps: &[StartupKeymapSnapshot],
    snapshot: &CoreLightSnapshot,
    host_pending: &mut Option<String>,
    host_count: &mut Option<usize>,
) -> PipelineResolution {
    let Some(prefix) = host_pending.take() else {
        log::debug!("[main][pipeline] timeout fired with no host pending, unhandled");
        return PipelineResolution::Unhandled;
    };
    let count = host_count.take();
    let flushed = match count {
        Some(n) => format!("{n}{prefix}"),
        None => prefix.clone(),
    };
    log::debug!(
        "[main][pipeline] timeout flush: pending prefix resolved as builtin: prefix={prefix:?}, composed_count={count:?}, flushed={flushed:?}, core_mode={:?}",
        snapshot.mode
    );
    PipelineResolution::Passthrough(flushed)
}

/// ADR 0006 Phase 3: 完成コマンドのみが backend を越えるよう、host 側で部分入力を
/// バッファリングした上での 1 ステップ解決結果。
///
/// `resolve_pipeline_command` の `Passthrough(keys)`（core へ素通し予定のキー列）に対し、
/// core の非破壊予測器（完成度判定）を適用して「完成なら 1 回 dispatch、pending なら
/// host バッファに留め core を呼ばない」を表現する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BufferedResolution {
    /// mapping が確定した（`BuiltinEdit` / `HostCommand`）。呼び出し側が variant で実行する。
    Command(Command),
    /// 完成キー列が確定した。この文字列を core へ **1 回だけ** dispatch する。
    /// host 側 passthrough バッファは空にクリア済み。
    DispatchComplete(String),
    /// このキー列はまだ未完成（operator の motion 待ち等）。host バッファに留め、
    /// backend は呼ばない。キーは consume 済み。
    HoldPending,
    /// digit を host count に取り込んだ。キーは consume 済み（core へは流さない）。
    CountAccumulated,
    /// pipeline で扱えないキー（lhs 表現不能）。呼び出し側の従来処理へフォールバック。
    Unhandled,
}

/// ADR 0006 Phase 3: 完成判定をパイプラインに集約し、完成コマンドのみを backend へ
/// 越境させる buffered 解決関数。
///
/// `resolve_pipeline_command` をラップし、その `Passthrough(keys)` に対して core の
/// 非破壊完成度予測（`classify`）を適用する。
///
/// `classify` は「あるキー列を現在モードで core に渡すと完成か pending か」を **状態を
/// 変えずに** 返す純粋クエリ（`CoreBridge::classify_input_completeness` 相当）。
///
/// 振る舞い:
/// - keymap の `Command` 確定 / `CountAccumulated` / `Unhandled` はそのまま転送する。
///   ただし `Command`（mapping 確定）や `Unhandled` の前には host passthrough バッファに
///   溜まっている未完成キーがあればそれを先に flush するのが本来だが、設計上 keymap 解決
///   （prefix/count）と passthrough バッファは同時に pending にならない（keymap の
///   `PendingPrefix` 中は passthrough は空）。よって単純化のため両立は考慮しない。
/// - `PendingPrefix` は keymap prefix の pending。これも backend を呼ばない `HoldPending`
///   として表現する（passthrough バッファとは別管理だが、呼び出し側から見れば同じ
///   「保留中・core 未呼び出し」）。
/// - `Passthrough(keys)`: host passthrough バッファに `keys` を追記し、結合列を `classify`。
///   - `Pending` -> バッファに留め `HoldPending`（**core を呼ばない**）。
///   - `Complete` -> バッファ全体を `DispatchComplete` で返しバッファをクリア
///     （**完成キー列を 1 回だけ dispatch**）。
pub fn resolve_pipeline_command_buffered<F>(
    keymaps: &[StartupKeymapSnapshot],
    snapshot: &CoreLightSnapshot,
    host_pending: &mut Option<String>,
    host_count: &mut Option<usize>,
    host_passthrough: &mut String,
    key: &KeyInput,
    classify: F,
) -> BufferedResolution
where
    F: Fn(&str) -> vim_core_rs::CoreInputCompleteness,
{
    let resolution = resolve_pipeline_command(keymaps, snapshot, host_pending, host_count, key);
    match resolution {
        PipelineResolution::Command(command) => {
            log::debug!(
                "[main][pipeline] buffered: mapping command resolved, forwarding: command={command:?}, residual_passthrough={host_passthrough:?}"
            );
            BufferedResolution::Command(command)
        }
        PipelineResolution::CountAccumulated => {
            log::debug!("[main][pipeline] buffered: count accumulated, backend not called");
            BufferedResolution::CountAccumulated
        }
        PipelineResolution::PendingPrefix => {
            log::debug!(
                "[main][pipeline] buffered: keymap prefix pending, backend not called (host_pending={host_pending:?})"
            );
            BufferedResolution::HoldPending
        }
        PipelineResolution::Passthrough(keys) => {
            host_passthrough.push_str(&keys);
            let completeness = classify(host_passthrough);
            log::debug!(
                "[main][pipeline] buffered: passthrough appended, completeness predicted: appended={keys:?}, buffer={host_passthrough:?}, completeness={completeness:?}"
            );
            if completeness.is_pending() {
                log::debug!(
                    "[main][pipeline] buffered: prediction=Pending, holding in host buffer, backend NOT called: buffer={host_passthrough:?}"
                );
                BufferedResolution::HoldPending
            } else {
                let complete = std::mem::take(host_passthrough);
                log::debug!(
                    "[main][pipeline] buffered: prediction=Complete, dispatching complete sequence ONCE to backend: complete={complete:?}"
                );
                BufferedResolution::DispatchComplete(complete)
            }
        }
        PipelineResolution::Unhandled => {
            log::debug!(
                "[main][pipeline] buffered: unhandled, fall back to legacy path: residual_passthrough={host_passthrough:?}"
            );
            BufferedResolution::Unhandled
        }
    }
}

/// ADR 0006 Phase 3: timeoutlen 発火時に host passthrough バッファを完成として
/// flush する。未完成のまま timeout した operator/motion 待ち等を builtin として確定し、
/// core へ 1 回 dispatch するためのキー列を返す（空なら None）。
pub fn flush_passthrough_buffer_on_timeout(host_passthrough: &mut String) -> Option<String> {
    if host_passthrough.is_empty() {
        return None;
    }
    let flushed = std::mem::take(host_passthrough);
    log::debug!(
        "[main][pipeline] buffered timeout flush: passthrough buffer flushed to core as builtin: flushed={flushed:?}"
    );
    Some(flushed)
}

#[cfg(test)]
#[path = "command_adr0006_phase2_test.rs"]
mod adr0006_phase2_tests;
#[cfg(test)]
#[path = "command_adr0006_phase3_test.rs"]
mod adr0006_phase3_tests;
