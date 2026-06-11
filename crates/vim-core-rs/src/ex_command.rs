//! Ex コマンド文字列のパーサ群。
//!
//! クレートルート（lib.rs）から抽出した、Ex コマンドのパイプ分割・範囲解析・
//! substitute 解析などを行う純粋な文字列パーサ。FFI に依存せず自己完結する。
//! パース結果である `ParsedExIntent` 型自体は公開 Core 型群および
//! `VimCoreSession` の dispatch と密結合するため lib.rs に残し、本モジュールからは
//! `use super::*;` 経由で参照する。

use super::*;

/// 単一のExコマンド文字列をパースしてインテントに変換する。
/// パイプを含まない単一コマンド専用。
pub(super) fn parse_single_ex_intent(trimmed: &str) -> Option<ParsedExIntent> {
    let mut parts = trimmed.split_whitespace();
    let head = parts.next()?;
    let bang = head.ends_with('!');
    let command_name = head.trim_end_matches('!');
    let tail = parts.collect::<Vec<_>>().join(" ");

    match command_name {
        "edit" | "e" if !tail.is_empty() => Some(ParsedExIntent::Edit { locator: tail }),
        "write" | "w" => Some(ParsedExIntent::Write {
            path: tail,
            force: bang,
        }),
        "update" | "up" => Some(ParsedExIntent::Update {
            path: tail,
            force: bang,
        }),
        "wq" | "wqall" => Some(ParsedExIntent::SaveAndClose {
            force: bang,
            path: String::new(),
        }),
        "exit" | "xit" | "x" | "xall" => Some(ParsedExIntent::SaveIfDirtyAndClose {
            path: String::new(),
        }),
        "quit" | "q" | "quitall" | "qall" | "qa" => Some(ParsedExIntent::Quit { force: bang }),
        "suspend" | "stop" => Some(ParsedExIntent::Suspend),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedExPrefix {
    command_start: usize,
    range_span: Option<std::ops::Range<usize>>,
    modifier_spans: Vec<std::ops::Range<usize>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubstituteFlavor {
    Substitute,
    Magic,
    NoMagic,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SubstituteFamilyToken {
    flavor: SubstituteFlavor,
    token_span: std::ops::Range<usize>,
    payload_start: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RecognizedCommandFamily {
    Substitute(SubstituteFamilyToken),
    Write {
        force: bool,
        token_span: std::ops::Range<usize>,
    },
    Update {
        force: bool,
        token_span: std::ops::Range<usize>,
    },
    Quit {
        force: bool,
        token_span: std::ops::Range<usize>,
    },
    Other {
        token_span: std::ops::Range<usize>,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct SubstituteFlags {
    keep_previous: bool,
    confirm: bool,
    no_error: bool,
    global: bool,
    ignore_case: bool,
    no_ignore_case: bool,
    print_list: bool,
    report_only: bool,
    print_last: bool,
    reuse_last_search: bool,
    print_with_number: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SubstituteMode {
    WithPattern {
        delimiter: u8,
        delimiter_span: std::ops::Range<usize>,
        trailing_flags_span: std::ops::Range<usize>,
        count: Option<u32>,
    },
    RepeatLast {
        flags: SubstituteFlags,
        flags_span: std::ops::Range<usize>,
        count: Option<u32>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SubstituteHeader {
    flavor: SubstituteFlavor,
    mode: SubstituteMode,
    token_span: std::ops::Range<usize>,
    header_span: std::ops::Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UnresolvedSubstituteHeader {
    token_span: std::ops::Range<usize>,
    payload_span: std::ops::Range<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ParsedExCommandKind {
    Substitute(SubstituteHeader),
    SubstituteUnresolved(UnresolvedSubstituteHeader),
    Write { force: bool },
    Update { force: bool },
    Quit { force: bool },
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedExCommandHeader {
    kind: ParsedExCommandKind,
    token_span: std::ops::Range<usize>,
    command_span: std::ops::Range<usize>,
}

fn parse_ex_prefix(input: &str) -> ParsedExPrefix {
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len && bytes[i].is_ascii_whitespace() {
        i += 1;
    }

    let mut modifier_spans = Vec::new();
    while let Some((modifier_start, modifier_end)) = parse_command_modifier(input, i) {
        modifier_spans.push(modifier_start..modifier_end);
        i = modifier_end;
        while i < len && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
    }

    let range_span = parse_ex_range(input, i);
    if let Some(range) = &range_span {
        i = range.end;
        while i < len && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
    }

    while let Some((modifier_start, modifier_end)) = parse_command_modifier(input, i) {
        modifier_spans.push(modifier_start..modifier_end);
        i = modifier_end;
        while i < len && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
    }

    ParsedExPrefix {
        command_start: i,
        range_span,
        modifier_spans,
    }
}

fn parse_command_modifier(input: &str, start: usize) -> Option<(usize, usize)> {
    const MODIFIERS: &[&str] = &[
        "keeppatterns",
        "keepjumps",
        "noautocmd",
        "keepalt",
        "keepmarks",
        "lockmarks",
        "silent",
    ];

    let bytes = input.as_bytes();
    if start >= bytes.len() || !bytes[start].is_ascii_alphabetic() {
        return None;
    }

    for modifier in MODIFIERS {
        let end = start + modifier.len();
        if input.get(start..end) != Some(*modifier) {
            continue;
        }
        if bytes.get(end).is_none_or(|next| next.is_ascii_whitespace()) {
            return Some((start, end));
        }
    }

    None
}

fn parse_ex_range(input: &str, start: usize) -> Option<std::ops::Range<usize>> {
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut i = start;
    let mut consumed = false;

    while i < len {
        match bytes[i] {
            b'%' | b'.' | b'$' | b',' | b';' => {
                consumed = true;
                i += 1;
            }
            b'0'..=b'9' => {
                consumed = true;
                i += 1;
                while i < len && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
            b'\'' => {
                if i + 1 >= len {
                    break;
                }
                consumed = true;
                i += 2;
            }
            b'/' | b'?' => {
                let delimiter = bytes[i];
                consumed = true;
                i += 1;
                while i < len {
                    if bytes[i] == b'\\' {
                        i += 1;
                        if i < len {
                            i += 1;
                        }
                        continue;
                    }
                    if bytes[i] == delimiter {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
            }
            b'+' | b'-' => {
                consumed = true;
                i += 1;
                while i < len && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
            _ => break,
        }
    }

    consumed.then_some(start..i)
}

fn recognize_command_family(input: &str, prefix: &ParsedExPrefix) -> RecognizedCommandFamily {
    let start = prefix.command_start;
    let bytes = input.as_bytes();
    let len = bytes.len();
    if start >= len {
        return RecognizedCommandFamily::Other {
            token_span: start..start,
        };
    }

    let mut end = start;
    while end < len && bytes[end].is_ascii_alphabetic() {
        end += 1;
    }
    if end == start {
        return RecognizedCommandFamily::Other {
            token_span: start..start,
        };
    }

    let token = &input[start..end];
    let next = bytes.get(end).copied();

    if is_substitute_boundary(next) {
        if is_substitute_abbreviation(token, "sno", "snomagic") {
            return RecognizedCommandFamily::Substitute(SubstituteFamilyToken {
                flavor: SubstituteFlavor::NoMagic,
                token_span: start..end,
                payload_start: end,
            });
        }
        if is_substitute_abbreviation(token, "sm", "smagic") {
            return RecognizedCommandFamily::Substitute(SubstituteFamilyToken {
                flavor: SubstituteFlavor::Magic,
                token_span: start..end,
                payload_start: end,
            });
        }
        if is_substitute_abbreviation(token, "s", "substitute") {
            return RecognizedCommandFamily::Substitute(SubstituteFamilyToken {
                flavor: SubstituteFlavor::Substitute,
                token_span: start..end,
                payload_start: end,
            });
        }
    }

    let (force, family_end) = if next == Some(b'!') {
        (true, end + 1)
    } else {
        (false, end)
    };
    let family_boundary = bytes
        .get(family_end)
        .is_none_or(|next| next.is_ascii_whitespace());

    if family_boundary {
        if matches!(token, "write" | "w") {
            return RecognizedCommandFamily::Write {
                force,
                token_span: start..family_end,
            };
        }
        if matches!(token, "update" | "up") {
            return RecognizedCommandFamily::Update {
                force,
                token_span: start..family_end,
            };
        }
        if matches!(token, "quit" | "q" | "quitall" | "qall" | "qa") {
            return RecognizedCommandFamily::Quit {
                force,
                token_span: start..family_end,
            };
        }
    }

    RecognizedCommandFamily::Other {
        token_span: start..end,
    }
}

fn is_substitute_abbreviation(token: &str, minimum: &str, canonical: &str) -> bool {
    token.len() >= minimum.len() && canonical.starts_with(token)
}

fn is_substitute_boundary(next: Option<u8>) -> bool {
    match next {
        None => true,
        Some(byte) => {
            byte.is_ascii_whitespace() || byte.is_ascii_digit() || !byte.is_ascii_alphabetic()
        }
    }
}

fn is_substitute_delimiter(byte: u8) -> bool {
    !byte.is_ascii_alphanumeric() && !matches!(byte, b'\\' | b'"' | b'|')
}

fn select_substitute_mode(
    input: &str,
    token: &SubstituteFamilyToken,
) -> Result<SubstituteHeader, UnresolvedSubstituteHeader> {
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut i = token.payload_start;
    while i < len && bytes[i].is_ascii_whitespace() {
        i += 1;
    }

    if i >= len {
        return Ok(SubstituteHeader {
            flavor: token.flavor,
            mode: SubstituteMode::RepeatLast {
                flags: SubstituteFlags::default(),
                flags_span: i..i,
                count: None,
            },
            token_span: token.token_span.clone(),
            header_span: token.token_span.start..i,
        });
    }

    let first = bytes[i];
    if is_substitute_delimiter(first) {
        return Ok(SubstituteHeader {
            flavor: token.flavor,
            mode: SubstituteMode::WithPattern {
                delimiter: first,
                delimiter_span: i..i + 1,
                trailing_flags_span: len..len,
                count: None,
            },
            token_span: token.token_span.clone(),
            header_span: token.token_span.start..len,
        });
    }

    let segment_end = input.find('|').unwrap_or(len);
    let Some((flags, flags_span, count, consumed_end)) =
        parse_repeat_last_payload(input, i, segment_end)
    else {
        return Err(UnresolvedSubstituteHeader {
            token_span: token.token_span.clone(),
            payload_span: i..len,
        });
    };

    Ok(SubstituteHeader {
        flavor: token.flavor,
        mode: SubstituteMode::RepeatLast {
            flags,
            flags_span,
            count,
        },
        token_span: token.token_span.clone(),
        header_span: token.token_span.start..consumed_end,
    })
}

fn parse_repeat_last_payload(
    input: &str,
    start: usize,
    end: usize,
) -> Option<(SubstituteFlags, std::ops::Range<usize>, Option<u32>, usize)> {
    let bytes = input.as_bytes();
    let len = end.min(bytes.len());
    let mut i = start;
    let flags_start = i;
    let mut flags = SubstituteFlags::default();

    while i < len {
        let accepted = match bytes[i] {
            b'&' => {
                flags.keep_previous = true;
                true
            }
            b'c' => {
                flags.confirm = true;
                true
            }
            b'e' => {
                flags.no_error = true;
                true
            }
            b'g' => {
                flags.global = true;
                true
            }
            b'i' => {
                flags.ignore_case = true;
                true
            }
            b'I' => {
                flags.no_ignore_case = true;
                true
            }
            b'l' => {
                flags.print_list = true;
                true
            }
            b'n' => {
                flags.report_only = true;
                true
            }
            b'p' => {
                flags.print_last = true;
                true
            }
            b'r' => {
                flags.reuse_last_search = true;
                true
            }
            b'#' => {
                flags.print_with_number = true;
                true
            }
            _ => false,
        };

        if !accepted {
            break;
        }
        i += 1;
    }

    let flags_end = i;
    while i < len && bytes[i].is_ascii_whitespace() {
        i += 1;
    }

    let count_start = i;
    while i < len && bytes[i].is_ascii_digit() {
        i += 1;
    }
    let count = if i > count_start {
        input[count_start..i].parse::<u32>().ok()
    } else {
        None
    };

    while i < len && bytes[i].is_ascii_whitespace() {
        i += 1;
    }

    if i != len {
        return None;
    }

    Some((flags, flags_start..flags_end, count, i))
}

fn analyze_ex_command_header(input: &str) -> ParsedExCommandHeader {
    let prefix = parse_ex_prefix(input);
    match recognize_command_family(input, &prefix) {
        RecognizedCommandFamily::Substitute(token) => match select_substitute_mode(input, &token) {
            Ok(header) => ParsedExCommandHeader {
                kind: ParsedExCommandKind::Substitute(header.clone()),
                token_span: header.token_span.clone(),
                command_span: prefix.command_start..input.len(),
            },
            Err(unresolved) => ParsedExCommandHeader {
                kind: ParsedExCommandKind::SubstituteUnresolved(unresolved.clone()),
                token_span: unresolved.token_span.clone(),
                command_span: prefix.command_start..input.len(),
            },
        },
        RecognizedCommandFamily::Write { force, token_span } => ParsedExCommandHeader {
            kind: ParsedExCommandKind::Write { force },
            token_span,
            command_span: prefix.command_start..input.len(),
        },
        RecognizedCommandFamily::Update { force, token_span } => ParsedExCommandHeader {
            kind: ParsedExCommandKind::Update { force },
            token_span,
            command_span: prefix.command_start..input.len(),
        },
        RecognizedCommandFamily::Quit { force, token_span } => ParsedExCommandHeader {
            kind: ParsedExCommandKind::Quit { force },
            token_span,
            command_span: prefix.command_start..input.len(),
        },
        RecognizedCommandFamily::Other { token_span } => ParsedExCommandHeader {
            kind: ParsedExCommandKind::Other,
            token_span,
            command_span: prefix.command_start..input.len(),
        },
    }
}

/// 複合コマンド（パイプ区切り）を安全に分割する。
/// 引用符やスラッシュ内の `|` はパイプ区切りとして扱わない。
pub(super) fn split_compound_ex(input: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut start = 0;
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    let mut substitute_state = match &analyze_ex_command_header(&input[start..]).kind {
        ParsedExCommandKind::Substitute(SubstituteHeader {
            mode:
                SubstituteMode::WithPattern {
                    delimiter,
                    delimiter_span,
                    ..
                },
            ..
        }) => Some((*delimiter, start + delimiter_span.end, false)),
        ParsedExCommandKind::SubstituteUnresolved(_) => {
            segments.push(&input[start..]);
            return segments;
        }
        _ => None,
    };

    while i < len {
        if let Some((delimiter, ref mut state_start, ref mut in_replacement)) = substitute_state {
            if i < *state_start {
                i = *state_start;
                continue;
            }

            if bytes[i] == b'\\' {
                i += 1;
                if i < len {
                    i += 1;
                }
                continue;
            }
            if bytes[i] == delimiter {
                if *in_replacement {
                    substitute_state = None;
                } else {
                    *in_replacement = true;
                }
                i += 1;
                continue;
            }

            i += 1;
            continue;
        }

        let ch = bytes[i];
        // 引用符内はスキップ
        if ch == b'"' || ch == b'\'' {
            let quote = ch;
            i += 1;
            while i < len && bytes[i] != quote {
                if bytes[i] == b'\\' {
                    i += 1; // エスケープ文字をスキップ
                }
                i += 1;
            }
            if i < len {
                i += 1; // 閉じ引用符をスキップ
            }
            continue;
        }
        // パイプ区切り検出
        if ch == b'|' {
            segments.push(&input[start..i]);
            start = i + 1;
            i = start;
            substitute_state = match &analyze_ex_command_header(&input[start..]).kind {
                ParsedExCommandKind::Substitute(SubstituteHeader {
                    mode:
                        SubstituteMode::WithPattern {
                            delimiter,
                            delimiter_span,
                            ..
                        },
                    ..
                }) => Some((*delimiter, start + delimiter_span.end, false)),
                ParsedExCommandKind::SubstituteUnresolved(_) => {
                    segments.push(&input[start..]);
                    return segments;
                }
                _ => None,
            };
            continue;
        }
        i += 1;
    }
    segments.push(&input[start..]);
    segments
}

pub(super) fn parse_ex_intent(command: &str) -> Option<ParsedExIntent> {
    let trimmed = command.trim();
    let trimmed = trimmed.strip_prefix(':').unwrap_or(trimmed).trim();
    if trimmed.is_empty() {
        return None;
    }

    // パイプを含まないコマンドは従来どおり単一パース
    if !trimmed.contains('|') {
        return parse_single_ex_intent(trimmed);
    }

    // 複合コマンド: 各サブコマンドを分割して、インターセプト対象を探す
    let segments = split_compound_ex(trimmed);
    debug_log!(
        "[DEBUG] parse_ex_intent: compound command split into {} segments: {:?}",
        segments.len(),
        segments
    );

    // インターセプト対象のサブコマンドを先頭から探し、最初に見つかったものを返す
    for seg in &segments {
        let seg_trimmed = seg.trim();
        if seg_trimmed.is_empty() {
            continue;
        }
        if let Some(intent) = parse_single_ex_intent(seg_trimmed) {
            debug_log!(
                "[DEBUG] parse_ex_intent: found interceptable sub-command '{}' -> {:?}",
                seg_trimmed,
                intent
            );
            return Some(intent);
        }
    }

    None
}

pub(super) fn should_chain_trailing_quit_for_local_write(
    session: &VimCoreSession,
    intent: &ParsedExIntent,
    trailing_segments: &[&str],
) -> bool {
    if !matches!(
        intent,
        ParsedExIntent::Write { .. } | ParsedExIntent::Update { .. }
    ) {
        return false;
    }

    if find_first_trailing_quit_intent(trailing_segments).is_none() {
        return false;
    }

    let snapshot = session.snapshot();
    let Some(active_buf) = snapshot.buffers.iter().find(|buffer| buffer.is_active) else {
        return false;
    };

    !session
        .document_coordinator
        .borrow()
        .is_vfs_buffer(active_buf.id)
}

pub(super) fn rewrite_vfs_trailing_quit_intent(
    session: &VimCoreSession,
    intent: ParsedExIntent,
    trailing_segments: &[&str],
) -> ParsedExIntent {
    if find_first_trailing_quit_intent(trailing_segments).is_none() {
        return intent;
    }

    let snapshot = session.snapshot();
    let Some(active_buf) = snapshot.buffers.iter().find(|buffer| buffer.is_active) else {
        return intent;
    };

    if !session
        .document_coordinator
        .borrow()
        .is_vfs_buffer(active_buf.id)
    {
        return intent;
    }

    match intent {
        ParsedExIntent::Write { force, path } => ParsedExIntent::SaveAndClose { force, path },
        ParsedExIntent::Update { force: false, path } => {
            ParsedExIntent::SaveIfDirtyAndClose { path }
        }
        other => other,
    }
}

pub(super) fn find_first_trailing_quit_intent(
    trailing_segments: &[&str],
) -> Option<ParsedExIntent> {
    for seg in trailing_segments {
        let seg_trimmed = seg.trim();
        if seg_trimmed.is_empty() {
            continue;
        }
        match parse_single_ex_intent(seg_trimmed) {
            Some(intent @ ParsedExIntent::Quit { .. }) => return Some(intent),
            _ => return None,
        }
    }
    None
}

#[cfg(test)]
mod compound_ex_parser_tests {
    use super::*;

    #[test]
    fn parse_ex_prefix_skips_range_and_known_modifiers() {
        let prefix = parse_ex_prefix("silent keepjumps 1,2s#foo#bar#");

        assert_eq!(prefix.command_start, "silent keepjumps 1,2".len());
        assert_eq!(
            prefix.range_span,
            Some("silent keepjumps ".len().."silent keepjumps 1,2".len())
        );
        assert_eq!(
            prefix.modifier_spans,
            vec![0.."silent".len(), "silent ".len().."silent keepjumps".len()]
        );
    }

    #[test]
    fn recognize_command_family_distinguishes_substitute_families_and_force_commands() {
        let prefix = parse_ex_prefix("s!foo!bar!");
        assert!(matches!(
            recognize_command_family("s!foo!bar!", &prefix),
            RecognizedCommandFamily::Substitute(SubstituteFamilyToken {
                flavor: SubstituteFlavor::Substitute,
                ..
            })
        ));

        let prefix = parse_ex_prefix("sm/foo/bar/");
        assert!(matches!(
            recognize_command_family("sm/foo/bar/", &prefix),
            RecognizedCommandFamily::Substitute(SubstituteFamilyToken {
                flavor: SubstituteFlavor::Magic,
                ..
            })
        ));

        let prefix = parse_ex_prefix("sno?foo?bar?");
        assert!(matches!(
            recognize_command_family("sno?foo?bar?", &prefix),
            RecognizedCommandFamily::Substitute(SubstituteFamilyToken {
                flavor: SubstituteFlavor::NoMagic,
                ..
            })
        ));

        let prefix = parse_ex_prefix("write! output.txt");
        assert!(matches!(
            recognize_command_family("write! output.txt", &prefix),
            RecognizedCommandFamily::Write { force: true, .. }
        ));
    }

    #[test]
    fn recognize_command_family_rejects_non_boundary_matches() {
        let prefix = parse_ex_prefix("submarine");
        assert!(matches!(
            recognize_command_family("submarine", &prefix),
            RecognizedCommandFamily::Other { .. }
        ));

        let prefix = parse_ex_prefix("smudge");
        assert!(matches!(
            recognize_command_family("smudge", &prefix),
            RecognizedCommandFamily::Other { .. }
        ));
    }

    #[test]
    fn select_substitute_mode_supports_dynamic_delimiters_and_repeat_last() {
        let prefix = parse_ex_prefix("s!foo!bar!");
        let RecognizedCommandFamily::Substitute(token) =
            recognize_command_family("s!foo!bar!", &prefix)
        else {
            panic!("expected substitute family");
        };
        let header = select_substitute_mode("s!foo!bar!", &token).expect("with-pattern");
        assert!(matches!(
            header.mode,
            SubstituteMode::WithPattern {
                delimiter: b'!',
                ..
            }
        ));

        let prefix = parse_ex_prefix("substitute g 3");
        let RecognizedCommandFamily::Substitute(token) =
            recognize_command_family("substitute g 3", &prefix)
        else {
            panic!("expected substitute family");
        };
        let header = select_substitute_mode("substitute g 3", &token).expect("repeat-last");
        assert!(matches!(
            header.mode,
            SubstituteMode::RepeatLast {
                flags: SubstituteFlags { global: true, .. },
                count: Some(3),
                ..
            }
        ));
    }

    #[test]
    fn select_substitute_mode_marks_invalid_headers_unresolved() {
        let prefix = parse_ex_prefix(r"s\bad");
        let RecognizedCommandFamily::Substitute(token) =
            recognize_command_family(r"s\bad", &prefix)
        else {
            panic!("expected substitute family");
        };

        assert!(select_substitute_mode(r"s\bad", &token).is_err());
    }

    #[test]
    fn split_compound_ex_preserves_substitute_payload_and_trailing_separator() {
        assert_eq!(
            split_compound_ex("s#foo|bar#baz# | write"),
            vec!["s#foo|bar#baz# ", " write"]
        );
        assert_eq!(
            split_compound_ex("sm!foo|bar!baz! | quit"),
            vec!["sm!foo|bar!baz! ", " quit"]
        );
        assert_eq!(
            split_compound_ex("substitute g | write"),
            vec!["substitute g ", " write"]
        );
        assert_eq!(split_compound_ex(r"s\bad | write"), vec![r"s\bad | write"]);
    }
}
