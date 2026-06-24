//! CLI 出力整形ヘルパー。
//!
//! help/version テキストの描画と、起動時に発生する各種エラーを
//! ユーザー向けメッセージへ整形する純粋関数を集約する。バイナリ
//! `sy` のエントリ（`main.rs`）から利用される。

use crate::app::bootstrap::BootstrapError;
use crate::app::cli::CliParseError;
use crate::app::startup::{LaunchStartError, TuiStartupContextError};

pub fn format_cli_error(error: CliParseError) -> String {
    match error {
        CliParseError::MissingConfigPath => "設定ファイルのパスが指定されていません".to_string(),
        CliParseError::MissingLineNumber => "開始行番号が指定されていません".to_string(),
        CliParseError::MissingPluginCommand => {
            "plugin サブコマンドが指定されていません".to_string()
        }
        CliParseError::InvalidLineNumber(value) => {
            format!("開始行番号が不正です: {}", value.to_string_lossy())
        }
        CliParseError::MultipleTargetPaths => "対象ファイルは 1 つだけ指定できます".to_string(),
        CliParseError::UnknownPluginCommand(command) => {
            format!(
                "未対応の plugin サブコマンドです: {}",
                command.to_string_lossy()
            )
        }
        CliParseError::UnknownFlag(flag) => {
            format!("未対応のオプションです: {}", flag.to_string_lossy())
        }
    }
}

pub fn format_bootstrap_error(error: BootstrapError) -> String {
    match error {
        BootstrapError::SessionAlreadyInitialized => {
            "エディタのセッションはすでに初期化されています".to_string()
        }
        BootstrapError::StdinReadFailed { message } => {
            format!("標準入力を読み込めませんでした: {}", message)
        }
        BootstrapError::TargetReadFailed { path, message } => {
            format!(
                "対象ファイルを読み込めませんでした ({}): {}",
                path.display(),
                message
            )
        }
    }
}

pub fn format_launch_start_error(error: LaunchStartError) -> String {
    match error {
        LaunchStartError::Bootstrap(error) => format_bootstrap_error(error),
        LaunchStartError::Terminal(error) => {
            format!("terminal lifecycle の初期化に失敗しました: {:?}", error)
        }
        LaunchStartError::Policy(error) => {
            format!("TUI-only policy に違反する起動要求です: {error}")
        }
    }
}

pub fn format_tui_startup_context_error(error: TuiStartupContextError) -> String {
    match error {
        TuiStartupContextError::Launch(error) => format_launch_start_error(error),
        TuiStartupContextError::CapabilityProbe(error) => {
            format!("terminal capability probe failed during startup composition: {error}")
        }
    }
}

pub fn render_help_text() -> String {
    [
        "Usage: sy [arguments] [file]",
        "",
        "Arguments:",
        "  --               Only file names after this",
        "  -                Read text from stdin",
        "  -u <init.ts>     Use <init.ts> as startup config",
        "  --config <path>  Use <path> as startup config",
        "                    Default: $XDG_CONFIG_HOME/saya/init.ts",
        "                    Fallback: $HOME/.config/saya/init.ts",
        "  +                Start at end of file",
        "  +<lnum>          Start at line <lnum>",
        "  -R               Read-only mode",
        "  -h, --help       Print help and exit",
        "  --version        Print version information and exit",
        "",
        "Plugin commands:",
        "  plugin sync      Generate plugin cache artifacts via the manager",
        "  plugin update    Update plugin cache artifacts via the manager",
        "  plugin list      List cached plugins",
        "  plugin clean     Remove generated startup and lazy cache artifacts",
        "  plugin doctor    Check plugin cache health",
    ]
    .join("\n")
}

pub fn render_version_text() -> String {
    format!("sy {}", env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
#[path = "cli_output_test.rs"]
mod tests;
