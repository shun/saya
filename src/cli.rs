use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchRequest {
    pub target_path: Option<PathBuf>,
    pub config_source: ConfigSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigSource {
    Default,
    File(PathBuf),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliParseError {
    MissingConfigPath,
    MultipleTargetPaths,
    UnknownFlag(OsString),
}

pub fn parse_launch_request<I, S>(args: I) -> Result<LaunchRequest, CliParseError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut target_path: Option<PathBuf> = None;
    let mut config_source = ConfigSource::Default;
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        let arg = arg.as_ref();

        if arg == "--config" {
            let Some(config_path) = args.next() else {
                return Err(CliParseError::MissingConfigPath);
            };
            config_source = ConfigSource::File(PathBuf::from(config_path.as_ref()));
            continue;
        }

        if is_flag(arg) {
            return Err(CliParseError::UnknownFlag(arg.to_os_string()));
        }

        let path = PathBuf::from(arg);
        if target_path.replace(path).is_some() {
            return Err(CliParseError::MultipleTargetPaths);
        }
    }

    Ok(LaunchRequest {
        target_path,
        config_source,
    })
}

fn is_flag(arg: &OsStr) -> bool {
    arg.to_str()
        .map(|value| value.starts_with('-') && value != "-")
        .unwrap_or(false)
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
                target_path: Some(PathBuf::from("notes.txt")),
                config_source: ConfigSource::File(PathBuf::from("init.ts")),
            }
        );
    }

    #[test]
    fn defaults_config_source_when_config_not_provided() {
        let request = parse_launch_request(["notes.txt"]).unwrap();

        assert_eq!(
            request,
            LaunchRequest {
                target_path: Some(PathBuf::from("notes.txt")),
                config_source: ConfigSource::Default,
            }
        );
    }

    #[test]
    fn allows_starting_without_target_path() {
        let request = parse_launch_request(["--config", "init.ts"]).unwrap();

        assert_eq!(
            request,
            LaunchRequest {
                target_path: None,
                config_source: ConfigSource::File(PathBuf::from("init.ts")),
            }
        );
    }

    #[test]
    fn rejects_multiple_positional_targets() {
        let err = parse_launch_request(["notes.txt", "other.txt"]).unwrap_err();

        assert_eq!(err, CliParseError::MultipleTargetPaths);
    }

    #[test]
    fn rejects_config_flag_without_value() {
        let err = parse_launch_request(["--config"]).unwrap_err();

        assert_eq!(err, CliParseError::MissingConfigPath);
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
