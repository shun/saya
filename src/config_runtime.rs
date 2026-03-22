//! TypeScript 設定の評価を担当するモジュール。
//!
//! 設定ファイルの読み込み、限定 API での評価、設定コマンドの生成を行う。
//! Vim script を前提としない公開面を提供し、失敗時は default 設定へ fallback する。

use std::path::PathBuf;

/// 設定評価から得られるコマンド。
///
/// TypeScript 設定から得る不変コマンドで、session 初期化前に確定する。
/// 適用後は immutable log として扱う。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigCommand {
    /// オプション値の設定
    SetOption {
        name: ConfigOptionName,
        value: ConfigOptionValue,
    },
    /// キーマッピングの設定
    MapKey {
        mode: ConfigKeyMode,
        lhs: String,
        rhs: String,
    },
}

/// 設定可能なオプション名（限定 API）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigOptionName {
    TabSize,
    LineNumbers,
}

/// オプション値の型。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigOptionValue {
    Number(i64),
    Boolean(bool),
}

/// キーマッピング対象のモード。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigKeyMode {
    Normal,
    Insert,
}

/// 設定読み込みの結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigLoadResult {
    /// 設定コマンドの適用成功
    Success { commands: Vec<ConfigCommand> },
    /// 設定なし（既定値使用）
    DefaultUsed,
    /// 読み込み失敗（warning 付きで継続）
    ReadFailed { path: PathBuf, message: String },
    /// 評価失敗（warning 付きで継続）
    EvalFailed { path: PathBuf, message: String },
}

/// 設定入力のソースを表す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigInput {
    /// 設定なし（既定値を使用）
    None,
    /// ファイルパスから読み込む
    FilePath(PathBuf),
}

/// 設定ファイルを読み取り、文字列入力として取得する。
///
/// 起動時に設定ファイルを見つけ、文字列入力として取得する。
/// 設定未指定時は既定値扱いにする。
pub fn read_config_source(input: &ConfigInput) -> ConfigSourceResult {
    log::debug!("[config_runtime] reading config source: {:?}", input);
    match input {
        ConfigInput::None => {
            log::debug!("[config_runtime] no config input, using defaults");
            ConfigSourceResult::Default
        }
        ConfigInput::FilePath(path) => {
            log::debug!("[config_runtime] reading config file: {}", path.display());
            match std::fs::read_to_string(path) {
                Ok(source) => {
                    log::debug!(
                        "[config_runtime] config file read success: path={}, len={}",
                        path.display(),
                        source.len()
                    );
                    ConfigSourceResult::Loaded {
                        path: path.clone(),
                        source,
                    }
                }
                Err(error) => {
                    log::debug!(
                        "[config_runtime] config file read failed: path={}, error={}",
                        path.display(),
                        error
                    );
                    ConfigSourceResult::ReadFailed {
                        path: path.clone(),
                        message: error.to_string(),
                    }
                }
            }
        }
    }
}

/// 設定ファイル読み取りの結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigSourceResult {
    /// 既定値を使用
    Default,
    /// ファイルから読み込み成功
    Loaded { path: PathBuf, source: String },
    /// ファイル読み込み失敗
    ReadFailed { path: PathBuf, message: String },
}

/// 設定ソースを評価し、ConfigCommand 列に変換する。
///
/// option 変更と key mapping に必要な最小コマンドへ変換する。
/// Vim script 前提の入力を受け付けない形にする。
pub fn evaluate_config(source_result: &ConfigSourceResult) -> ConfigLoadResult {
    log::debug!(
        "[config_runtime] evaluating config: {:?}",
        match source_result {
            ConfigSourceResult::Default => "default".to_string(),
            ConfigSourceResult::Loaded { path, .. } => format!("loaded:{}", path.display()),
            ConfigSourceResult::ReadFailed { path, .. } =>
                format!("read_failed:{}", path.display()),
        }
    );

    match source_result {
        ConfigSourceResult::Default => {
            log::debug!("[config_runtime] using default config");
            ConfigLoadResult::DefaultUsed
        }
        ConfigSourceResult::ReadFailed { path, message } => {
            log::debug!(
                "[config_runtime] config read failed, returning ReadFailed: path={}, message={}",
                path.display(),
                message
            );
            ConfigLoadResult::ReadFailed {
                path: path.clone(),
                message: message.clone(),
            }
        }
        ConfigSourceResult::Loaded { path, source } => {
            log::debug!(
                "[config_runtime] evaluating config source: path={}, len={}",
                path.display(),
                source.len()
            );
            evaluate_config_source(path, source)
        }
    }
}

/// 設定ソース文字列を評価し、ConfigCommand 列に変換する。
///
/// MVP では JSON ベースの設定形式を受け付ける。
/// deno_core による TypeScript 評価は後続タスクで拡張する。
fn evaluate_config_source(path: &std::path::Path, source: &str) -> ConfigLoadResult {
    log::debug!(
        "[config_runtime] parsing config source: path={}, source_preview={:?}",
        path.display(),
        &source[..source.len().min(100)]
    );

    // Vim script 形式を検出して拒否する
    if is_vim_script_syntax(source) {
        log::debug!(
            "[config_runtime] rejected: Vim script syntax detected in config: {}",
            path.display()
        );
        return ConfigLoadResult::EvalFailed {
            path: path.to_path_buf(),
            message: "Vim script 形式の設定は受け付けません。TypeScript 形式で記述してください。"
                .to_string(),
        };
    }

    // JSON 形式のパース（MVP 最小限の評価ロジック）
    match parse_config_json(source) {
        Ok(commands) => {
            log::debug!(
                "[config_runtime] config evaluation success: commands_count={}",
                commands.len()
            );
            ConfigLoadResult::Success { commands }
        }
        Err(message) => {
            log::debug!(
                "[config_runtime] config evaluation failed: path={}, error={}",
                path.display(),
                message
            );
            ConfigLoadResult::EvalFailed {
                path: path.to_path_buf(),
                message,
            }
        }
    }
}

/// Vim script 構文を検出する。
///
/// `set`, `let`, `map`, `noremap` などの Vim script 特有のコマンドを検出する。
fn is_vim_script_syntax(source: &str) -> bool {
    let trimmed = source.trim();
    let vim_patterns = [
        "set ",
        "let ",
        "let g:",
        "noremap ",
        "nnoremap ",
        "inoremap ",
        "vnoremap ",
        "map ",
        "nmap ",
        "imap ",
        "vmap ",
        "autocmd ",
        "augroup ",
        "function!",
        "endfunction",
        "if has(",
        "source ",
        "colorscheme ",
        "syntax ",
        "filetype ",
    ];

    for line in trimmed.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('"') {
            continue;
        }
        for pattern in &vim_patterns {
            if line.starts_with(pattern) {
                log::debug!(
                    "[config_runtime] vim script pattern detected: {:?} in line: {:?}",
                    pattern,
                    line
                );
                return true;
            }
        }
    }
    false
}

/// MVP 用の JSON 設定パーサー。
///
/// `{ "options": {...}, "keyMappings": [...] }` 形式を受け付ける。
/// deno_core による TypeScript 評価への移行を前提とした最小実装。
fn parse_config_json(source: &str) -> Result<Vec<ConfigCommand>, String> {
    let trimmed = source.trim();
    if trimmed.is_empty() || trimmed == "{}" {
        log::debug!("[config_runtime] empty or trivial config, returning empty commands");
        return Ok(Vec::new());
    }

    // 簡易 JSON パーサー（外部依存なし、MVP 最小限）
    let mut commands = Vec::new();

    // "tabSize": <number> を検出
    if let Some(value) = extract_json_number(trimmed, "tabSize") {
        log::debug!("[config_runtime] found tabSize option: {}", value);
        commands.push(ConfigCommand::SetOption {
            name: ConfigOptionName::TabSize,
            value: ConfigOptionValue::Number(value),
        });
    }

    // "lineNumbers": <bool> を検出
    if let Some(value) = extract_json_bool(trimmed, "lineNumbers") {
        log::debug!("[config_runtime] found lineNumbers option: {}", value);
        commands.push(ConfigCommand::SetOption {
            name: ConfigOptionName::LineNumbers,
            value: ConfigOptionValue::Boolean(value),
        });
    }

    // "keyMappings" 配列は MVP では簡易的に扱う
    // 完全な JSON パースは deno_core 移行時に置き換え予定

    log::debug!(
        "[config_runtime] parsed {} commands from JSON config",
        commands.len()
    );
    Ok(commands)
}

/// JSON 文字列から指定キーの数値を簡易抽出する。
fn extract_json_number(json: &str, key: &str) -> Option<i64> {
    let pattern = format!("\"{}\"", key);
    let pos = json.find(&pattern)?;
    let after_key = &json[pos + pattern.len()..];
    let colon_pos = after_key.find(':')?;
    let after_colon = after_key[colon_pos + 1..].trim_start();

    // 数値の開始位置から連続する数字を取得
    let num_str: String = after_colon
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '-')
        .collect();
    num_str.parse().ok()
}

/// JSON 文字列から指定キーの真偽値を簡易抽出する。
fn extract_json_bool(json: &str, key: &str) -> Option<bool> {
    let pattern = format!("\"{}\"", key);
    let pos = json.find(&pattern)?;
    let after_key = &json[pos + pattern.len()..];
    let colon_pos = after_key.find(':')?;
    let after_colon = after_key[colon_pos + 1..].trim_start();

    if after_colon.starts_with("true") {
        Some(true)
    } else if after_colon.starts_with("false") {
        Some(false)
    } else {
        None
    }
}

/// 設定コマンドを起動時の editor 状態へ適用する。
///
/// 初期 session 準備後、描画開始前に設定を反映する。
/// 設定適用順が毎回ぶれないように固定する。
pub fn apply_config_commands(
    commands: &[ConfigCommand],
    state: &mut ConfigApplyState,
) -> ConfigApplyResult {
    log::debug!(
        "[config_runtime] applying {} config commands to editor state",
        commands.len()
    );

    let mut applied_count = 0;
    let mut errors = Vec::new();

    for (index, command) in commands.iter().enumerate() {
        log::debug!(
            "[config_runtime] applying command {}/{}: {:?}",
            index + 1,
            commands.len(),
            command
        );
        match apply_single_command(command, state) {
            Ok(()) => {
                applied_count += 1;
                log::debug!(
                    "[config_runtime] command {}/{} applied successfully",
                    index + 1,
                    commands.len()
                );
            }
            Err(message) => {
                log::debug!(
                    "[config_runtime] command {}/{} failed: {}",
                    index + 1,
                    commands.len(),
                    message
                );
                errors.push(ConfigApplyError {
                    command_index: index,
                    message,
                });
            }
        }
    }

    let result = ConfigApplyResult {
        applied_count,
        errors,
    };
    log::debug!(
        "[config_runtime] config apply complete: applied={}, errors={}",
        result.applied_count,
        result.errors.len()
    );
    result
}

/// 設定適用先の状態。editor session の設定可能な部分を表す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigApplyState {
    pub tab_size: i64,
    pub line_numbers: bool,
    pub key_mappings: Vec<AppliedKeyMapping>,
}

/// 適用済みキーマッピング。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedKeyMapping {
    pub mode: ConfigKeyMode,
    pub lhs: String,
    pub rhs: String,
}

impl ConfigApplyState {
    /// 既定値で初期化する。
    pub fn default_state() -> Self {
        log::debug!("[config_runtime] creating default config apply state");
        Self {
            tab_size: 8,
            line_numbers: false,
            key_mappings: Vec::new(),
        }
    }
}

/// 設定適用の結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigApplyResult {
    pub applied_count: usize,
    pub errors: Vec<ConfigApplyError>,
}

impl ConfigApplyResult {
    pub fn is_fully_applied(&self) -> bool {
        self.errors.is_empty()
    }
}

/// 個別の設定適用エラー。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigApplyError {
    pub command_index: usize,
    pub message: String,
}

/// 個別のコマンドを適用する。
fn apply_single_command(
    command: &ConfigCommand,
    state: &mut ConfigApplyState,
) -> Result<(), String> {
    match command {
        ConfigCommand::SetOption { name, value } => match (name, value) {
            (ConfigOptionName::TabSize, ConfigOptionValue::Number(n)) => {
                if *n < 1 || *n > 32 {
                    return Err(format!(
                        "tabSize の値は 1〜32 の範囲で指定してください: {}",
                        n
                    ));
                }
                log::debug!(
                    "[config_runtime] setting tabSize: {} -> {}",
                    state.tab_size,
                    n
                );
                state.tab_size = *n;
                Ok(())
            }
            (ConfigOptionName::LineNumbers, ConfigOptionValue::Boolean(b)) => {
                log::debug!(
                    "[config_runtime] setting lineNumbers: {} -> {}",
                    state.line_numbers,
                    b
                );
                state.line_numbers = *b;
                Ok(())
            }
            (name, value) => Err(format!(
                "オプション {:?} に対して不正な値型 {:?} が指定されました",
                name, value
            )),
        },
        ConfigCommand::MapKey { mode, lhs, rhs } => {
            if lhs.is_empty() {
                return Err("キーマッピングの lhs が空です".to_string());
            }
            if rhs.is_empty() {
                return Err("キーマッピングの rhs が空です".to_string());
            }
            log::debug!(
                "[config_runtime] adding key mapping: mode={:?}, lhs={:?}, rhs={:?}",
                mode,
                lhs,
                rhs
            );
            state.key_mappings.push(AppliedKeyMapping {
                mode: mode.clone(),
                lhs: lhs.clone(),
                rhs: rhs.clone(),
            });
            Ok(())
        }
    }
}

/// 設定の全フローを実行する統合関数。
///
/// 読み込み -> 評価 -> 適用 を一貫して実行し、
/// 失敗時は warning 付きで既定値を返す。
pub fn load_and_apply_config(input: &ConfigInput) -> (ConfigApplyState, Vec<String>) {
    log::debug!("[config_runtime] starting full config load-and-apply flow");
    let mut warnings = Vec::new();
    let mut state = ConfigApplyState::default_state();

    let source_result = read_config_source(input);
    let load_result = evaluate_config(&source_result);

    match load_result {
        ConfigLoadResult::Success { commands } => {
            let apply_result = apply_config_commands(&commands, &mut state);
            if !apply_result.is_fully_applied() {
                for error in &apply_result.errors {
                    let warning = format!(
                        "設定コマンド {} の適用に失敗しました: {}",
                        error.command_index + 1,
                        error.message
                    );
                    log::debug!("[config_runtime] apply warning: {}", warning);
                    warnings.push(warning);
                }
            }
            log::debug!(
                "[config_runtime] config applied: applied={}, warnings={}",
                apply_result.applied_count,
                warnings.len()
            );
        }
        ConfigLoadResult::DefaultUsed => {
            log::debug!("[config_runtime] using default config, no warnings");
        }
        ConfigLoadResult::ReadFailed { path, message } => {
            let warning = format!(
                "設定ファイルの読み込みに失敗したため既定値で起動します ({}): {}",
                path.display(),
                message
            );
            log::debug!("[config_runtime] read failure warning: {}", warning);
            warnings.push(warning);
        }
        ConfigLoadResult::EvalFailed { path, message } => {
            let warning = format!(
                "設定ファイルの評価に失敗したため既定値で起動します ({}): {}",
                path.display(),
                message
            );
            log::debug!("[config_runtime] eval failure warning: {}", warning);
            warnings.push(warning);
        }
    }

    log::debug!(
        "[config_runtime] load-and-apply complete: state={:?}, warnings={}",
        state,
        warnings.len()
    );
    (state, warnings)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn unique_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        std::env::temp_dir().join(format!("saya-config-{name}-{nanos}"))
    }

    // ==== タスク 8.1: 限定された設定入力を読み取れるようにする ====

    #[test]
    fn read_config_returns_default_when_no_input() {
        let result = read_config_source(&ConfigInput::None);

        assert_eq!(
            result,
            ConfigSourceResult::Default,
            "設定未指定時は既定値扱いになること"
        );
    }

    #[test]
    fn read_config_returns_loaded_when_file_exists() {
        let config_path = unique_path("config-exists");
        std::fs::write(&config_path, "{ \"tabSize\": 4 }").expect("write config");

        let result = read_config_source(&ConfigInput::FilePath(config_path.clone()));

        match result {
            ConfigSourceResult::Loaded { path, source } => {
                assert_eq!(path, config_path);
                assert_eq!(source, "{ \"tabSize\": 4 }");
            }
            other => panic!("既存ファイルは Loaded を返すこと, got: {:?}", other),
        }

        std::fs::remove_file(config_path).expect("cleanup");
    }

    #[test]
    fn read_config_returns_read_failed_when_file_missing() {
        let missing_path = unique_path("config-missing");

        let result = read_config_source(&ConfigInput::FilePath(missing_path.clone()));

        match result {
            ConfigSourceResult::ReadFailed { path, message } => {
                assert_eq!(path, missing_path);
                assert!(!message.is_empty(), "読み込み失敗メッセージは空でないこと");
            }
            other => panic!(
                "存在しないファイルは ReadFailed を返すこと, got: {:?}",
                other
            ),
        }
    }

    #[test]
    fn read_config_treats_unspecified_as_default() {
        // 設定未指定は既定値として扱い、エラーにならないこと
        let result = read_config_source(&ConfigInput::None);
        assert_eq!(result, ConfigSourceResult::Default);

        let load_result = evaluate_config(&result);
        assert_eq!(
            load_result,
            ConfigLoadResult::DefaultUsed,
            "設定未指定は DefaultUsed になること"
        );
    }

    // ==== タスク 8.2: 限定 API だけを使って設定を評価できるようにする ====

    #[test]
    fn evaluate_config_parses_tab_size_option() {
        let source = ConfigSourceResult::Loaded {
            path: PathBuf::from("test.json"),
            source: "{ \"tabSize\": 4 }".to_string(),
        };

        let result = evaluate_config(&source);

        match result {
            ConfigLoadResult::Success { commands } => {
                assert_eq!(commands.len(), 1);
                assert_eq!(
                    commands[0],
                    ConfigCommand::SetOption {
                        name: ConfigOptionName::TabSize,
                        value: ConfigOptionValue::Number(4),
                    },
                    "tabSize オプションが正しくパースされること"
                );
            }
            other => panic!("Success を返すこと, got: {:?}", other),
        }
    }

    #[test]
    fn evaluate_config_parses_line_numbers_option() {
        let source = ConfigSourceResult::Loaded {
            path: PathBuf::from("test.json"),
            source: "{ \"lineNumbers\": true }".to_string(),
        };

        let result = evaluate_config(&source);

        match result {
            ConfigLoadResult::Success { commands } => {
                assert_eq!(commands.len(), 1);
                assert_eq!(
                    commands[0],
                    ConfigCommand::SetOption {
                        name: ConfigOptionName::LineNumbers,
                        value: ConfigOptionValue::Boolean(true),
                    }
                );
            }
            other => panic!("Success を返すこと, got: {:?}", other),
        }
    }

    #[test]
    fn evaluate_config_parses_multiple_options() {
        let source = ConfigSourceResult::Loaded {
            path: PathBuf::from("test.json"),
            source: "{ \"tabSize\": 2, \"lineNumbers\": false }".to_string(),
        };

        let result = evaluate_config(&source);

        match result {
            ConfigLoadResult::Success { commands } => {
                assert_eq!(commands.len(), 2, "複数オプションが全てパースされること");
            }
            other => panic!("Success を返すこと, got: {:?}", other),
        }
    }

    #[test]
    fn evaluate_config_rejects_vim_script_syntax() {
        let source = ConfigSourceResult::Loaded {
            path: PathBuf::from("init.vim"),
            source: "set tabstop=4\nset number\n".to_string(),
        };

        let result = evaluate_config(&source);

        match result {
            ConfigLoadResult::EvalFailed { path, message } => {
                assert_eq!(path, PathBuf::from("init.vim"));
                assert!(
                    message.contains("Vim script"),
                    "Vim script 拒否メッセージを含むこと: {}",
                    message
                );
            }
            other => panic!("Vim script は EvalFailed を返すこと, got: {:?}", other),
        }
    }

    #[test]
    fn evaluate_config_rejects_noremap_vim_script() {
        let source = ConfigSourceResult::Loaded {
            path: PathBuf::from("config.vim"),
            source: "nnoremap <leader>f :Files<CR>".to_string(),
        };

        let result = evaluate_config(&source);

        assert!(
            matches!(result, ConfigLoadResult::EvalFailed { .. }),
            "noremap 構文は拒否されること"
        );
    }

    #[test]
    fn evaluate_config_returns_empty_commands_for_empty_config() {
        let source = ConfigSourceResult::Loaded {
            path: PathBuf::from("empty.json"),
            source: "{}".to_string(),
        };

        let result = evaluate_config(&source);

        match result {
            ConfigLoadResult::Success { commands } => {
                assert!(
                    commands.is_empty(),
                    "空の設定は空のコマンドリストを返すこと"
                );
            }
            other => panic!("Success を返すこと, got: {:?}", other),
        }
    }

    #[test]
    fn evaluate_config_returns_default_used_for_no_config() {
        let source = ConfigSourceResult::Default;

        let result = evaluate_config(&source);

        assert_eq!(
            result,
            ConfigLoadResult::DefaultUsed,
            "設定なしは DefaultUsed を返すこと"
        );
    }

    #[test]
    fn evaluate_config_propagates_read_failure() {
        let source = ConfigSourceResult::ReadFailed {
            path: PathBuf::from("missing.json"),
            message: "file not found".to_string(),
        };

        let result = evaluate_config(&source);

        assert_eq!(
            result,
            ConfigLoadResult::ReadFailed {
                path: PathBuf::from("missing.json"),
                message: "file not found".to_string(),
            },
            "読み込み失敗がそのまま伝播すること"
        );
    }

    // ==== タスク 8.3: 設定コマンドを起動時の editor 状態へ適用する ====

    #[test]
    fn apply_tab_size_command_updates_state() {
        let commands = vec![ConfigCommand::SetOption {
            name: ConfigOptionName::TabSize,
            value: ConfigOptionValue::Number(4),
        }];
        let mut state = ConfigApplyState::default_state();
        assert_eq!(state.tab_size, 8, "既定値は 8 であること");

        let result = apply_config_commands(&commands, &mut state);

        assert_eq!(state.tab_size, 4, "tabSize が 4 に変更されること");
        assert!(result.is_fully_applied());
        assert_eq!(result.applied_count, 1);
    }

    #[test]
    fn apply_line_numbers_command_updates_state() {
        let commands = vec![ConfigCommand::SetOption {
            name: ConfigOptionName::LineNumbers,
            value: ConfigOptionValue::Boolean(true),
        }];
        let mut state = ConfigApplyState::default_state();
        assert!(!state.line_numbers, "既定値は false であること");

        let result = apply_config_commands(&commands, &mut state);

        assert!(state.line_numbers, "lineNumbers が true に変更されること");
        assert!(result.is_fully_applied());
    }

    #[test]
    fn apply_key_mapping_command_adds_to_state() {
        let commands = vec![ConfigCommand::MapKey {
            mode: ConfigKeyMode::Normal,
            lhs: "<leader>f".to_string(),
            rhs: ":find ".to_string(),
        }];
        let mut state = ConfigApplyState::default_state();

        let result = apply_config_commands(&commands, &mut state);

        assert_eq!(state.key_mappings.len(), 1);
        assert_eq!(state.key_mappings[0].lhs, "<leader>f");
        assert_eq!(state.key_mappings[0].rhs, ":find ");
        assert_eq!(state.key_mappings[0].mode, ConfigKeyMode::Normal);
        assert!(result.is_fully_applied());
    }

    #[test]
    fn apply_commands_in_deterministic_order() {
        let commands = vec![
            ConfigCommand::SetOption {
                name: ConfigOptionName::TabSize,
                value: ConfigOptionValue::Number(2),
            },
            ConfigCommand::SetOption {
                name: ConfigOptionName::LineNumbers,
                value: ConfigOptionValue::Boolean(true),
            },
            ConfigCommand::MapKey {
                mode: ConfigKeyMode::Insert,
                lhs: "jk".to_string(),
                rhs: "\x1b".to_string(),
            },
        ];
        let mut state = ConfigApplyState::default_state();

        let result = apply_config_commands(&commands, &mut state);

        // 適用順が固定されていること
        assert_eq!(state.tab_size, 2);
        assert!(state.line_numbers);
        assert_eq!(state.key_mappings.len(), 1);
        assert_eq!(result.applied_count, 3);
        assert!(result.is_fully_applied());
    }

    #[test]
    fn apply_rejects_invalid_tab_size() {
        let commands = vec![ConfigCommand::SetOption {
            name: ConfigOptionName::TabSize,
            value: ConfigOptionValue::Number(0),
        }];
        let mut state = ConfigApplyState::default_state();

        let result = apply_config_commands(&commands, &mut state);

        assert!(!result.is_fully_applied());
        assert_eq!(result.errors.len(), 1);
        assert_eq!(state.tab_size, 8, "不正な値の場合は既定値が維持されること");
    }

    #[test]
    fn apply_rejects_type_mismatch() {
        let commands = vec![ConfigCommand::SetOption {
            name: ConfigOptionName::TabSize,
            value: ConfigOptionValue::Boolean(true),
        }];
        let mut state = ConfigApplyState::default_state();

        let result = apply_config_commands(&commands, &mut state);

        assert!(!result.is_fully_applied());
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn apply_empty_commands_is_noop() {
        let commands: Vec<ConfigCommand> = Vec::new();
        let mut state = ConfigApplyState::default_state();

        let result = apply_config_commands(&commands, &mut state);

        assert!(result.is_fully_applied());
        assert_eq!(result.applied_count, 0);
        assert_eq!(state.tab_size, 8, "空コマンドでは状態が変わらないこと");
    }

    // ==== タスク 8.4: 設定失敗時に warning 付きで起動継続できるようにする ====

    #[test]
    fn load_and_apply_with_no_config_returns_defaults_and_no_warnings() {
        let (state, warnings) = load_and_apply_config(&ConfigInput::None);

        assert_eq!(state.tab_size, 8);
        assert!(!state.line_numbers);
        assert!(state.key_mappings.is_empty());
        assert!(
            warnings.is_empty(),
            "設定なしの場合は warning なしで起動すること"
        );
    }

    #[test]
    fn load_and_apply_with_missing_file_returns_defaults_with_warning() {
        let missing_path = unique_path("config-load-missing");

        let (state, warnings) = load_and_apply_config(&ConfigInput::FilePath(missing_path));

        assert_eq!(state.tab_size, 8, "読み込み失敗時は既定値で起動すること");
        assert_eq!(
            warnings.len(),
            1,
            "読み込み失敗時は warning が 1 つ出ること"
        );
        assert!(
            warnings[0].contains("読み込みに失敗"),
            "読み込み失敗の warning メッセージ: {}",
            warnings[0]
        );
    }

    #[test]
    fn load_and_apply_with_vim_script_returns_defaults_with_warning() {
        let vim_config = unique_path("config-vim");
        std::fs::write(&vim_config, "set tabstop=4\nset number\n").expect("write vim config");

        let (state, warnings) = load_and_apply_config(&ConfigInput::FilePath(vim_config.clone()));

        assert_eq!(
            state.tab_size, 8,
            "Vim script 設定は無視され既定値になること"
        );
        assert_eq!(warnings.len(), 1, "評価失敗時は warning が 1 つ出ること");
        assert!(
            warnings[0].contains("評価に失敗"),
            "評価失敗の warning メッセージ: {}",
            warnings[0]
        );

        std::fs::remove_file(vim_config).expect("cleanup");
    }

    #[test]
    fn load_and_apply_with_valid_config_applies_successfully() {
        let config_path = unique_path("config-valid");
        std::fs::write(&config_path, "{ \"tabSize\": 4, \"lineNumbers\": true }")
            .expect("write config");

        let (state, warnings) = load_and_apply_config(&ConfigInput::FilePath(config_path.clone()));

        assert_eq!(state.tab_size, 4, "tabSize が設定値に変更されること");
        assert!(state.line_numbers, "lineNumbers が設定値に変更されること");
        assert!(
            warnings.is_empty(),
            "有効な設定では warning なしで適用されること"
        );

        std::fs::remove_file(config_path).expect("cleanup");
    }

    #[test]
    fn load_and_apply_with_empty_config_uses_defaults_without_warning() {
        let config_path = unique_path("config-empty");
        std::fs::write(&config_path, "{}").expect("write empty config");

        let (state, warnings) = load_and_apply_config(&ConfigInput::FilePath(config_path.clone()));

        assert_eq!(state.tab_size, 8, "空設定は既定値のまま");
        assert!(warnings.is_empty(), "空設定は warning なし");

        std::fs::remove_file(config_path).expect("cleanup");
    }

    #[test]
    fn config_failure_does_not_block_editor_basic_workflow() {
        // 設定失敗後も既定値で完全に動作する状態が返ること
        let missing_path = unique_path("config-fail-workflow");

        let (state, warnings) = load_and_apply_config(&ConfigInput::FilePath(missing_path));

        // 既定値で editor が動作可能な状態であること
        assert_eq!(state.tab_size, 8);
        assert!(!state.line_numbers);
        assert!(state.key_mappings.is_empty());

        // warning は出ているが、state は完全に有効
        assert!(!warnings.is_empty());

        // 状態は clone 可能（session に渡せる）
        let _cloned = state.clone();
    }

    #[test]
    fn read_failure_and_eval_failure_share_common_fallback() {
        // 読み込み失敗と評価失敗の両方が同じ既定値 fallback を使うこと
        let missing_path = unique_path("config-read-fail");
        let (state_read_fail, _) = load_and_apply_config(&ConfigInput::FilePath(missing_path));

        let vim_config = unique_path("config-eval-fail");
        std::fs::write(&vim_config, "set number").expect("write vim config");
        let (state_eval_fail, _) =
            load_and_apply_config(&ConfigInput::FilePath(vim_config.clone()));

        // 両方とも同じ既定値状態であること
        assert_eq!(
            state_read_fail, state_eval_fail,
            "読み込み失敗と評価失敗で同じ既定値 fallback が使われること"
        );

        std::fs::remove_file(vim_config).expect("cleanup");
    }
}
