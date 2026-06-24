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
