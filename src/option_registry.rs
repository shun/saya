use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SayaOptionType {
    Boolean,
    Number,
    String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SayaOptionValue {
    Boolean(bool),
    Number(i64),
    String(String),
}

impl SayaOptionValue {
    pub fn option_type(&self) -> SayaOptionType {
        match self {
            Self::Boolean(_) => SayaOptionType::Boolean,
            Self::Number(_) => SayaOptionType::Number,
            Self::String(_) => SayaOptionType::String,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SayaOptionOwner {
    CoreOwned,
    PresentationOwned,
    HostOwned,
    UnsupportedPlanned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SayaOptionName {
    AutoIndent,
    Backup,
    Clipboard,
    CursorLine,
    ExpandTab,
    FileEncoding,
    FileFormat,
    FoldLevel,
    FoldMethod,
    IgnoreCase,
    LastStatus,
    LineNumbers,
    List,
    ListChars,
    NumberWidth,
    RelativeNumber,
    ScrollOff,
    ShiftWidth,
    SidescrollOff,
    SmartCase,
    SmartIndent,
    SoftTabStop,
    TabSize,
    Undofile,
    Wrap,
    WriteBackup,
}

impl SayaOptionName {
    pub fn canonical(self) -> &'static str {
        match self {
            Self::AutoIndent => "autoindent",
            Self::Backup => "backup",
            Self::Clipboard => "clipboard",
            Self::CursorLine => "cursorline",
            Self::ExpandTab => "expandtab",
            Self::FileEncoding => "fileencoding",
            Self::FileFormat => "fileformat",
            Self::FoldLevel => "foldlevel",
            Self::FoldMethod => "foldmethod",
            Self::IgnoreCase => "ignorecase",
            Self::LastStatus => "laststatus",
            Self::LineNumbers => "number",
            Self::List => "list",
            Self::ListChars => "listchars",
            Self::NumberWidth => "numberwidth",
            Self::RelativeNumber => "relativenumber",
            Self::ScrollOff => "scrolloff",
            Self::ShiftWidth => "shiftwidth",
            Self::SidescrollOff => "sidescrolloff",
            Self::SmartCase => "smartcase",
            Self::SmartIndent => "smartindent",
            Self::SoftTabStop => "softtabstop",
            Self::TabSize => "tabstop",
            Self::Undofile => "undofile",
            Self::Wrap => "wrap",
            Self::WriteBackup => "writebackup",
        }
    }
}

impl fmt::Display for SayaOptionName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.canonical())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SayaOptionDefinition {
    pub name: SayaOptionName,
    pub value_type: SayaOptionType,
    pub owner: SayaOptionOwner,
    pub startup_public: bool,
    pub aliases: &'static [&'static str],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SayaSetOperation {
    Assign(SayaOptionValue),
    Toggle,
    Invert,
    Query,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSayaSet {
    pub definition: &'static SayaOptionDefinition,
    pub operation: SayaSetOperation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SayaSetParseError {
    NotSetCommand,
    EmptySet,
    UnknownOption(String),
    InvalidValue {
        option: SayaOptionName,
        value: String,
        expected: SayaOptionType,
    },
    InvalidOperation {
        option: SayaOptionName,
        operation: String,
    },
}

pub struct SayaOptionRegistry;

impl SayaOptionRegistry {
    pub fn definitions() -> &'static [SayaOptionDefinition] {
        OPTION_DEFINITIONS
    }

    pub fn resolve(name: &str) -> Option<&'static SayaOptionDefinition> {
        let normalized = normalize_name(name);
        OPTION_DEFINITIONS.iter().find(|definition| {
            definition.name.canonical() == normalized
                || definition
                    .aliases
                    .iter()
                    .any(|alias| alias.to_ascii_lowercase() == normalized)
        })
    }

    pub fn startup_public_definitions() -> impl Iterator<Item = &'static SayaOptionDefinition> {
        OPTION_DEFINITIONS
            .iter()
            .filter(|definition| definition.startup_public)
    }

    pub fn parse_set_command(command: &str) -> Result<ParsedSayaSet, SayaSetParseError> {
        let normalized = normalize_command(command).ok_or(SayaSetParseError::NotSetCommand)?;
        let Some(rest) = normalized.strip_prefix("set ") else {
            return Err(SayaSetParseError::NotSetCommand);
        };
        let token = rest
            .split_whitespace()
            .next()
            .ok_or(SayaSetParseError::EmptySet)?;
        parse_set_token(token)
    }
}

fn parse_set_token(token: &str) -> Result<ParsedSayaSet, SayaSetParseError> {
    let token = token.trim();
    if token.is_empty() {
        return Err(SayaSetParseError::EmptySet);
    }

    if let Some(name) = token.strip_suffix('?') {
        let definition = SayaOptionRegistry::resolve(name)
            .ok_or_else(|| SayaSetParseError::UnknownOption(name.to_string()))?;
        return Ok(ParsedSayaSet {
            definition,
            operation: SayaSetOperation::Query,
        });
    }

    if let Some((name, value)) = token.split_once('=') {
        let definition = SayaOptionRegistry::resolve(name)
            .ok_or_else(|| SayaSetParseError::UnknownOption(name.to_string()))?;
        let value = parse_value(definition, value)?;
        return Ok(ParsedSayaSet {
            definition,
            operation: SayaSetOperation::Assign(value),
        });
    }

    if let Some(name) = token.strip_suffix('!') {
        let definition = SayaOptionRegistry::resolve(name)
            .ok_or_else(|| SayaSetParseError::UnknownOption(name.to_string()))?;
        if definition.value_type != SayaOptionType::Boolean {
            return Err(SayaSetParseError::InvalidOperation {
                option: definition.name,
                operation: "!".to_string(),
            });
        }
        return Ok(ParsedSayaSet {
            definition,
            operation: SayaSetOperation::Toggle,
        });
    }

    if let Some(name) = token.strip_prefix("inv") {
        let definition = SayaOptionRegistry::resolve(name)
            .ok_or_else(|| SayaSetParseError::UnknownOption(name.to_string()))?;
        if definition.value_type != SayaOptionType::Boolean {
            return Err(SayaSetParseError::InvalidOperation {
                option: definition.name,
                operation: "inv".to_string(),
            });
        }
        return Ok(ParsedSayaSet {
            definition,
            operation: SayaSetOperation::Toggle,
        });
    }

    if let Some(name) = token.strip_prefix("no") {
        if let Some(definition) = SayaOptionRegistry::resolve(name)
            && definition.value_type == SayaOptionType::Boolean
        {
            return Ok(ParsedSayaSet {
                definition,
                operation: SayaSetOperation::Assign(SayaOptionValue::Boolean(false)),
            });
        }
    }

    let definition = SayaOptionRegistry::resolve(token)
        .ok_or_else(|| SayaSetParseError::UnknownOption(token.to_string()))?;
    if definition.value_type != SayaOptionType::Boolean {
        return Err(SayaSetParseError::InvalidOperation {
            option: definition.name,
            operation: "bare".to_string(),
        });
    }
    Ok(ParsedSayaSet {
        definition,
        operation: SayaSetOperation::Assign(SayaOptionValue::Boolean(true)),
    })
}

fn parse_value(
    definition: &SayaOptionDefinition,
    value: &str,
) -> Result<SayaOptionValue, SayaSetParseError> {
    match definition.value_type {
        SayaOptionType::Boolean => match value {
            "true" | "1" => Ok(SayaOptionValue::Boolean(true)),
            "false" | "0" => Ok(SayaOptionValue::Boolean(false)),
            _ => Err(SayaSetParseError::InvalidValue {
                option: definition.name,
                value: value.to_string(),
                expected: definition.value_type,
            }),
        },
        SayaOptionType::Number => value
            .parse::<i64>()
            .map(SayaOptionValue::Number)
            .map_err(|_| SayaSetParseError::InvalidValue {
                option: definition.name,
                value: value.to_string(),
                expected: definition.value_type,
            }),
        SayaOptionType::String => Ok(SayaOptionValue::String(value.to_string())),
    }
}

fn normalize_command(command: &str) -> Option<String> {
    let trimmed = command.trim();
    let trimmed = trimmed.strip_prefix(':').unwrap_or(trimmed).trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.split_whitespace().collect::<Vec<_>>().join(" "))
}

fn normalize_name(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

const OPTION_DEFINITIONS: &[SayaOptionDefinition] = &[
    SayaOptionDefinition {
        name: SayaOptionName::AutoIndent,
        value_type: SayaOptionType::Boolean,
        owner: SayaOptionOwner::CoreOwned,
        startup_public: true,
        aliases: &["ai"],
    },
    SayaOptionDefinition {
        name: SayaOptionName::Backup,
        value_type: SayaOptionType::Boolean,
        owner: SayaOptionOwner::UnsupportedPlanned,
        startup_public: false,
        aliases: &["bk"],
    },
    SayaOptionDefinition {
        name: SayaOptionName::Clipboard,
        value_type: SayaOptionType::String,
        owner: SayaOptionOwner::HostOwned,
        startup_public: false,
        aliases: &["cb"],
    },
    SayaOptionDefinition {
        name: SayaOptionName::CursorLine,
        value_type: SayaOptionType::Boolean,
        owner: SayaOptionOwner::PresentationOwned,
        startup_public: true,
        aliases: &["cul"],
    },
    SayaOptionDefinition {
        name: SayaOptionName::ExpandTab,
        value_type: SayaOptionType::Boolean,
        owner: SayaOptionOwner::CoreOwned,
        startup_public: true,
        aliases: &["et"],
    },
    SayaOptionDefinition {
        name: SayaOptionName::FileEncoding,
        value_type: SayaOptionType::String,
        owner: SayaOptionOwner::UnsupportedPlanned,
        startup_public: false,
        aliases: &["fenc"],
    },
    SayaOptionDefinition {
        name: SayaOptionName::FileFormat,
        value_type: SayaOptionType::String,
        owner: SayaOptionOwner::UnsupportedPlanned,
        startup_public: false,
        aliases: &["ff"],
    },
    SayaOptionDefinition {
        name: SayaOptionName::FoldLevel,
        value_type: SayaOptionType::Number,
        owner: SayaOptionOwner::PresentationOwned,
        startup_public: true,
        aliases: &["fdl"],
    },
    SayaOptionDefinition {
        name: SayaOptionName::FoldMethod,
        value_type: SayaOptionType::String,
        owner: SayaOptionOwner::PresentationOwned,
        startup_public: true,
        aliases: &["fdm"],
    },
    SayaOptionDefinition {
        name: SayaOptionName::IgnoreCase,
        value_type: SayaOptionType::Boolean,
        owner: SayaOptionOwner::CoreOwned,
        startup_public: true,
        aliases: &["ic"],
    },
    SayaOptionDefinition {
        name: SayaOptionName::LastStatus,
        value_type: SayaOptionType::Number,
        owner: SayaOptionOwner::PresentationOwned,
        startup_public: true,
        aliases: &["ls"],
    },
    SayaOptionDefinition {
        name: SayaOptionName::LineNumbers,
        value_type: SayaOptionType::Boolean,
        owner: SayaOptionOwner::PresentationOwned,
        startup_public: true,
        aliases: &["nu", "lineNumbers"],
    },
    SayaOptionDefinition {
        name: SayaOptionName::List,
        value_type: SayaOptionType::Boolean,
        owner: SayaOptionOwner::PresentationOwned,
        startup_public: true,
        aliases: &[],
    },
    SayaOptionDefinition {
        name: SayaOptionName::ListChars,
        value_type: SayaOptionType::String,
        owner: SayaOptionOwner::PresentationOwned,
        startup_public: true,
        aliases: &["lcs"],
    },
    SayaOptionDefinition {
        name: SayaOptionName::NumberWidth,
        value_type: SayaOptionType::Number,
        owner: SayaOptionOwner::PresentationOwned,
        startup_public: true,
        aliases: &["nuw", "numberWidth"],
    },
    SayaOptionDefinition {
        name: SayaOptionName::RelativeNumber,
        value_type: SayaOptionType::Boolean,
        owner: SayaOptionOwner::PresentationOwned,
        startup_public: true,
        aliases: &["rnu", "relativenumber"],
    },
    SayaOptionDefinition {
        name: SayaOptionName::ScrollOff,
        value_type: SayaOptionType::Number,
        owner: SayaOptionOwner::PresentationOwned,
        startup_public: true,
        aliases: &["so"],
    },
    SayaOptionDefinition {
        name: SayaOptionName::ShiftWidth,
        value_type: SayaOptionType::Number,
        owner: SayaOptionOwner::CoreOwned,
        startup_public: true,
        aliases: &["sw"],
    },
    SayaOptionDefinition {
        name: SayaOptionName::SidescrollOff,
        value_type: SayaOptionType::Number,
        owner: SayaOptionOwner::PresentationOwned,
        startup_public: true,
        aliases: &["siso"],
    },
    SayaOptionDefinition {
        name: SayaOptionName::SmartCase,
        value_type: SayaOptionType::Boolean,
        owner: SayaOptionOwner::CoreOwned,
        startup_public: true,
        aliases: &["scs"],
    },
    SayaOptionDefinition {
        name: SayaOptionName::SmartIndent,
        value_type: SayaOptionType::Boolean,
        owner: SayaOptionOwner::CoreOwned,
        startup_public: true,
        aliases: &["si"],
    },
    SayaOptionDefinition {
        name: SayaOptionName::SoftTabStop,
        value_type: SayaOptionType::Number,
        owner: SayaOptionOwner::CoreOwned,
        startup_public: true,
        aliases: &["sts"],
    },
    SayaOptionDefinition {
        name: SayaOptionName::TabSize,
        value_type: SayaOptionType::Number,
        owner: SayaOptionOwner::CoreOwned,
        startup_public: true,
        aliases: &["ts", "tabSize", "tabstop"],
    },
    SayaOptionDefinition {
        name: SayaOptionName::Undofile,
        value_type: SayaOptionType::Boolean,
        owner: SayaOptionOwner::UnsupportedPlanned,
        startup_public: false,
        aliases: &["udf"],
    },
    SayaOptionDefinition {
        name: SayaOptionName::Wrap,
        value_type: SayaOptionType::Boolean,
        owner: SayaOptionOwner::PresentationOwned,
        startup_public: true,
        aliases: &[],
    },
    SayaOptionDefinition {
        name: SayaOptionName::WriteBackup,
        value_type: SayaOptionType::Boolean,
        owner: SayaOptionOwner::UnsupportedPlanned,
        startup_public: false,
        aliases: &["wb"],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_resolves_alias_owner_type_and_startup_public_contract() {
        let expandtab = SayaOptionRegistry::resolve("et").expect("expandtab alias");
        assert_eq!(expandtab.name, SayaOptionName::ExpandTab);
        assert_eq!(expandtab.value_type, SayaOptionType::Boolean);
        assert_eq!(expandtab.owner, SayaOptionOwner::CoreOwned);
        assert!(expandtab.startup_public);

        let cursorline = SayaOptionRegistry::resolve("cul").expect("cursorline alias");
        assert_eq!(cursorline.owner, SayaOptionOwner::PresentationOwned);

        let clipboard = SayaOptionRegistry::resolve("clipboard").expect("clipboard");
        assert_eq!(clipboard.owner, SayaOptionOwner::HostOwned);
        assert!(!clipboard.startup_public);
    }

    #[test]
    fn parse_set_command_handles_boolean_forms_and_assignments() {
        assert_eq!(
            SayaOptionRegistry::parse_set_command(":set noet").expect("noet"),
            ParsedSayaSet {
                definition: SayaOptionRegistry::resolve("expandtab").unwrap(),
                operation: SayaSetOperation::Assign(SayaOptionValue::Boolean(false)),
            }
        );
        assert_eq!(
            SayaOptionRegistry::parse_set_command("set invwrap")
                .expect("invwrap")
                .operation,
            SayaSetOperation::Toggle
        );
        assert_eq!(
            SayaOptionRegistry::parse_set_command("set shiftwidth=4")
                .expect("shiftwidth")
                .operation,
            SayaSetOperation::Assign(SayaOptionValue::Number(4))
        );
        assert_eq!(
            SayaOptionRegistry::parse_set_command("set listchars=tab:>-,trail:-")
                .expect("listchars")
                .operation,
            SayaSetOperation::Assign(SayaOptionValue::String("tab:>-,trail:-".to_string()))
        );
    }
}
