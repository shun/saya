use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use crate::runtime::plugin::{PluginCacheRoot, PluginCommand};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchRequest {
    pub input_source: InputSource,
    pub config_source: ConfigSource,
    pub initial_cursor: InitialCursorPosition,
    pub read_only: bool,
    pub startup_action: StartupAction,
    /// 起動時に参照するプラグインキャッシュの基点を明示注入するための seam。
    ///
    /// 本番起動では `None` を維持し、`PluginHost::default_from_env()`（環境変数
    /// 解決）に委ねる。テストではここへ一時ディレクトリを注入することで、環境変数を
    /// 一切変更せずにプラグインキャッシュを密閉できる。
    pub plugin_cache_root: Option<PluginCacheRoot>,
    /// `ConfigSource::Default` を解決するときの設定ディレクトリ基点を明示注入する seam。
    ///
    /// 本番起動では `None` を維持し、`default_init_ts_path()`（`XDG_CONFIG_HOME` /
    /// `HOME` 解決、すなわち実ユーザーのホーム）に委ねる。テストではここへ一時ディレクトリ
    /// を注入することで、実ホームを読まずに `ConfigSource::Default` の挙動を検証できる。
    pub default_config_dir: Option<PathBuf>,
}

impl Default for LaunchRequest {
    fn default() -> Self {
        Self {
            input_source: InputSource::Empty,
            config_source: ConfigSource::Default,
            initial_cursor: InitialCursorPosition::None,
            read_only: false,
            startup_action: StartupAction::Edit,
            plugin_cache_root: None,
            default_config_dir: None,
        }
    }
}

impl LaunchRequest {
    pub fn target_path(&self) -> Option<&PathBuf> {
        match &self.input_source {
            InputSource::File(path) => Some(path),
            InputSource::Empty | InputSource::Stdin => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputSource {
    Empty,
    File(PathBuf),
    Stdin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigSource {
    Default,
    File(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitialCursorPosition {
    None,
    End,
    Line(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupAction {
    Edit,
    PrintHelp,
    PrintVersion,
    Plugin(PluginCommand),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliParseError {
    MissingConfigPath,
    MissingLineNumber,
    MissingPluginCommand,
    InvalidLineNumber(OsString),
    MultipleTargetPaths,
    UnknownPluginCommand(OsString),
    UnknownFlag(OsString),
}

pub fn parse_launch_request<I, S>(args: I) -> Result<LaunchRequest, CliParseError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut request = LaunchRequest::default();
    let mut args = args.into_iter();
    let mut treat_all_as_files = false;

    while let Some(arg) = args.next() {
        let arg = arg.as_ref();

        if treat_all_as_files {
            set_input_source(
                &mut request.input_source,
                InputSource::File(PathBuf::from(arg)),
            )?;
            continue;
        }

        if arg == "--" {
            treat_all_as_files = true;
            continue;
        }

        if is_config_flag(arg) {
            let Some(config_path) = args.next() else {
                return Err(CliParseError::MissingConfigPath);
            };
            request.config_source = ConfigSource::File(PathBuf::from(config_path.as_ref()));
            continue;
        }

        if arg == "-" {
            set_input_source(&mut request.input_source, InputSource::Stdin)?;
            continue;
        }

        if let Some(initial_cursor) = parse_initial_cursor(arg)? {
            request.initial_cursor = initial_cursor;
            continue;
        }

        if matches!(arg.to_str(), Some("-R")) {
            request.read_only = true;
            continue;
        }

        if matches!(arg.to_str(), Some("-h" | "--help")) {
            request.startup_action = StartupAction::PrintHelp;
            continue;
        }

        if matches!(arg.to_str(), Some("--version")) {
            request.startup_action = StartupAction::PrintVersion;
            continue;
        }

        if matches!(arg.to_str(), Some("plugin")) {
            let Some(command) = args.next() else {
                return Err(CliParseError::MissingPluginCommand);
            };
            let Some(command_text) = command.as_ref().to_str() else {
                return Err(CliParseError::UnknownPluginCommand(
                    command.as_ref().to_os_string(),
                ));
            };
            request.startup_action =
                StartupAction::Plugin(PluginCommand::parse(command_text).ok_or_else(|| {
                    CliParseError::UnknownPluginCommand(command.as_ref().to_os_string())
                })?);
            continue;
        }

        if is_flag(arg) {
            return Err(CliParseError::UnknownFlag(arg.to_os_string()));
        }

        set_input_source(
            &mut request.input_source,
            InputSource::File(PathBuf::from(arg)),
        )?;
    }

    Ok(request)
}

fn set_input_source(current: &mut InputSource, next: InputSource) -> Result<(), CliParseError> {
    if matches!(current, InputSource::Empty) {
        *current = next;
        return Ok(());
    }

    Err(CliParseError::MultipleTargetPaths)
}

fn parse_initial_cursor(arg: &OsStr) -> Result<Option<InitialCursorPosition>, CliParseError> {
    let Some(value) = arg.to_str() else {
        return Ok(None);
    };

    if value == "+" {
        return Ok(Some(InitialCursorPosition::End));
    }

    let Some(line_number_text) = value.strip_prefix('+') else {
        return Ok(None);
    };

    if line_number_text.is_empty() {
        return Err(CliParseError::MissingLineNumber);
    }

    let line_number = line_number_text
        .parse::<usize>()
        .ok()
        .filter(|line_number| *line_number > 0)
        .ok_or_else(|| CliParseError::InvalidLineNumber(arg.to_os_string()))?;
    Ok(Some(InitialCursorPosition::Line(line_number)))
}

fn is_flag(arg: &OsStr) -> bool {
    arg.to_str()
        .map(|value| value.starts_with('-') && value != "-")
        .unwrap_or(false)
}

fn is_config_flag(arg: &OsStr) -> bool {
    matches!(arg.to_str(), Some("--config" | "-u"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_target_path_and_config_path_from_args() {
        let request = parse_launch_request(["notes.txt", "--config", "init.ts"]).unwrap();

        assert_eq!(
            request,
            LaunchRequest {
                input_source: InputSource::File(PathBuf::from("notes.txt")),
                config_source: ConfigSource::File(PathBuf::from("init.ts")),
                ..LaunchRequest::default()
            }
        );
    }

    #[test]
    fn parses_vim_style_u_option_as_config_path() {
        let request = parse_launch_request(["notes.txt", "-u", "init.ts"]).unwrap();

        assert_eq!(
            request,
            LaunchRequest {
                input_source: InputSource::File(PathBuf::from("notes.txt")),
                config_source: ConfigSource::File(PathBuf::from("init.ts")),
                ..LaunchRequest::default()
            }
        );
    }

    #[test]
    fn allows_starting_without_target_path_with_vim_style_u_option() {
        let request = parse_launch_request(["-u", "init.ts"]).unwrap();

        assert_eq!(
            request,
            LaunchRequest {
                config_source: ConfigSource::File(PathBuf::from("init.ts")),
                ..LaunchRequest::default()
            }
        );
    }

    #[test]
    fn defaults_config_source_when_config_not_provided() {
        let request = parse_launch_request(["notes.txt"]).unwrap();

        assert_eq!(
            request,
            LaunchRequest {
                input_source: InputSource::File(PathBuf::from("notes.txt")),
                ..LaunchRequest::default()
            }
        );
    }

    #[test]
    fn allows_starting_without_target_path() {
        let request = parse_launch_request(["--config", "init.ts"]).unwrap();

        assert_eq!(
            request,
            LaunchRequest {
                config_source: ConfigSource::File(PathBuf::from("init.ts")),
                ..LaunchRequest::default()
            }
        );
    }

    #[test]
    fn parses_dash_dash_then_treats_following_value_as_file_name() {
        let request = parse_launch_request(["--", "-leading-name.txt"]).unwrap();

        assert_eq!(
            request,
            LaunchRequest {
                input_source: InputSource::File(PathBuf::from("-leading-name.txt")),
                ..LaunchRequest::default()
            }
        );
    }

    #[test]
    fn parses_stdin_input_source() {
        let request = parse_launch_request(["-"]).unwrap();

        assert_eq!(
            request,
            LaunchRequest {
                input_source: InputSource::Stdin,
                ..LaunchRequest::default()
            }
        );
    }

    #[test]
    fn parses_plus_as_end_of_file_cursor() {
        let request = parse_launch_request(["+"]).unwrap();

        assert_eq!(
            request,
            LaunchRequest {
                initial_cursor: InitialCursorPosition::End,
                ..LaunchRequest::default()
            }
        );
    }

    #[test]
    fn parses_plus_line_number_as_initial_cursor() {
        let request = parse_launch_request(["+42"]).unwrap();

        assert_eq!(
            request,
            LaunchRequest {
                initial_cursor: InitialCursorPosition::Line(42),
                ..LaunchRequest::default()
            }
        );
    }

    #[test]
    fn parses_read_only_mode() {
        let request = parse_launch_request(["-R", "notes.txt"]).unwrap();

        assert_eq!(
            request,
            LaunchRequest {
                input_source: InputSource::File(PathBuf::from("notes.txt")),
                read_only: true,
                ..LaunchRequest::default()
            }
        );
    }

    #[test]
    fn parses_help_startup_action() {
        let request = parse_launch_request(["--help"]).unwrap();

        assert_eq!(
            request,
            LaunchRequest {
                startup_action: StartupAction::PrintHelp,
                ..LaunchRequest::default()
            }
        );
    }

    #[test]
    fn parses_short_help_startup_action() {
        let request = parse_launch_request(["-h"]).unwrap();

        assert_eq!(
            request,
            LaunchRequest {
                startup_action: StartupAction::PrintHelp,
                ..LaunchRequest::default()
            }
        );
    }

    #[test]
    fn parses_version_startup_action() {
        let request = parse_launch_request(["--version"]).unwrap();

        assert_eq!(
            request,
            LaunchRequest {
                startup_action: StartupAction::PrintVersion,
                ..LaunchRequest::default()
            }
        );
    }

    #[test]
    fn parses_plugin_sync_action() {
        let request = parse_launch_request(["plugin", "sync"]).unwrap();

        assert_eq!(
            request,
            LaunchRequest {
                startup_action: StartupAction::Plugin(PluginCommand::Sync),
                ..LaunchRequest::default()
            }
        );
    }

    #[test]
    fn rejects_missing_plugin_command() {
        let err = parse_launch_request(["plugin"]).unwrap_err();

        assert_eq!(err, CliParseError::MissingPluginCommand);
    }

    #[test]
    fn rejects_unknown_plugin_command() {
        let err = parse_launch_request(["plugin", "packadd"]).unwrap_err();

        assert_eq!(
            err,
            CliParseError::UnknownPluginCommand(OsString::from("packadd"))
        );
    }

    #[test]
    fn rejects_multiple_positional_targets() {
        let err = parse_launch_request(["notes.txt", "other.txt"]).unwrap_err();

        assert_eq!(err, CliParseError::MultipleTargetPaths);
    }

    #[test]
    fn rejects_multiple_targets_when_stdin_and_file_are_combined() {
        let err = parse_launch_request(["-", "notes.txt"]).unwrap_err();

        assert_eq!(err, CliParseError::MultipleTargetPaths);
    }

    #[test]
    fn rejects_config_flag_without_value() {
        let err = parse_launch_request(["--config"]).unwrap_err();

        assert_eq!(err, CliParseError::MissingConfigPath);
    }

    #[test]
    fn rejects_vim_style_u_option_without_value() {
        let err = parse_launch_request(["-u"]).unwrap_err();

        assert_eq!(err, CliParseError::MissingConfigPath);
    }

    #[test]
    fn rejects_invalid_initial_line_number_text() {
        let err = parse_launch_request(["+abc"]).unwrap_err();

        assert_eq!(
            err,
            CliParseError::InvalidLineNumber(OsString::from("+abc"))
        );
    }

    #[test]
    fn rejects_zero_as_initial_line_number() {
        let err = parse_launch_request(["+0"]).unwrap_err();

        assert_eq!(err, CliParseError::InvalidLineNumber(OsString::from("+0")));
    }

    #[test]
    fn rejects_unknown_flags() {
        let err = parse_launch_request(["--unexpected"]).unwrap_err();

        assert_eq!(
            err,
            CliParseError::UnknownFlag(OsString::from("--unexpected"))
        );
    }
}
