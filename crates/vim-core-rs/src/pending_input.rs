//! pending input / motion grammar の純粋パーサ群。
//!
//! クレートルート（lib.rs）から抽出した、キー列から `CorePendingInput` を
//! 組み立てる純粋な文字列パーサ。Normal/Visual/Operator-pending のシーケンス
//! 文法、count プレフィックス解析、レジスタ・オペレータ・g 系列の待ち引数推定を
//! 担う。FFI に依存せず自己完結する。公開 Core 型である `CorePendingInput` /
//! `CorePendingArgumentKind` / `CoreMode` 自体は `VimCoreSession` の dispatch と
//! 密結合するため lib.rs に残し、本モジュールからは `use super::*;` 経由で参照する。

use super::*;

fn pending_input_with_keys(
    pending_keys: impl Into<String>,
    awaited_argument: Option<CorePendingArgumentKind>,
) -> CorePendingInput {
    CorePendingInput {
        pending_keys: pending_keys.into(),
        count: None,
        awaited_argument,
    }
}

fn pending_input_with_state(
    pending_keys: impl Into<String>,
    count: Option<usize>,
    awaited_argument: Option<CorePendingArgumentKind>,
) -> CorePendingInput {
    CorePendingInput {
        pending_keys: pending_keys.into(),
        count,
        awaited_argument,
    }
}

pub(super) fn pending_for_dispatch_sequence(sequence: &str) -> CorePendingInput {
    if sequence.is_empty() {
        return CorePendingInput::none();
    }

    let (count, command, _) = parse_count_prefix(sequence);
    if command.is_empty() {
        return pending_input_with_state(sequence, count, None);
    }

    let mut chars = command.chars();
    let Some(first) = chars.next() else {
        return CorePendingInput::none();
    };
    let rest = chars.as_str();

    match first {
        '"' => pending_for_register_prefixed_sequence(sequence, rest, count),
        'd' | 'y' | 'c' | '>' | '<' | '=' => {
            pending_for_operator_sequence(sequence, first, rest, count)
        }
        'f' | 'F' | 't' | 'T' => {
            if rest.is_empty() {
                pending_input_with_state(sequence, count, Some(CorePendingArgumentKind::Char))
            } else {
                CorePendingInput::none()
            }
        }
        'r' => {
            if rest.is_empty() {
                pending_input_with_state(
                    sequence,
                    count,
                    Some(CorePendingArgumentKind::ReplaceChar),
                )
            } else {
                CorePendingInput::none()
            }
        }
        'm' => {
            if rest.is_empty() {
                pending_input_with_state(sequence, count, Some(CorePendingArgumentKind::MarkSet))
            } else {
                CorePendingInput::none()
            }
        }
        '\'' | '`' => {
            if rest.is_empty() {
                pending_input_with_state(sequence, count, Some(CorePendingArgumentKind::MarkJump))
            } else {
                CorePendingInput::none()
            }
        }
        'g' => pending_for_g_sequence(sequence, rest, count),
        _ => CorePendingInput::none(),
    }
}

fn pending_for_register_prefixed_sequence(
    sequence: &str,
    rest: &str,
    count: Option<usize>,
) -> CorePendingInput {
    if rest.is_empty() {
        return pending_input_with_state(sequence, count, Some(CorePendingArgumentKind::Register));
    }

    let mut rest_chars = rest.chars();
    let _register_name = rest_chars.next();
    let command_tail = rest_chars.as_str();
    if command_tail.is_empty() {
        return pending_input_with_state(
            sequence,
            count,
            Some(CorePendingArgumentKind::NormalCommand),
        );
    }

    let (tail_count, tail_command, _) = parse_count_prefix(command_tail);
    let combined_count = combine_counts(count, tail_count);
    if tail_command.is_empty() {
        return pending_input_with_state(
            sequence,
            combined_count,
            Some(CorePendingArgumentKind::NormalCommand),
        );
    }

    let tail_pending = pending_for_dispatch_sequence(tail_command);
    if tail_pending.is_pending() {
        return pending_input_with_state(sequence, combined_count, tail_pending.awaited_argument);
    }

    CorePendingInput::none()
}

fn pending_for_operator_sequence(
    sequence: &str,
    operator: char,
    rest: &str,
    count: Option<usize>,
) -> CorePendingInput {
    if rest.is_empty() {
        return pending_input_with_state(
            sequence,
            count,
            Some(CorePendingArgumentKind::MotionOrTextObject),
        );
    }

    let (motion_count, motion_fragment, _) = parse_count_prefix(rest);
    let combined_count = combine_counts(count, motion_count);
    if motion_fragment.is_empty() {
        return pending_input_with_state(
            sequence,
            combined_count,
            Some(CorePendingArgumentKind::MotionOrTextObject),
        );
    }

    let mut rest_chars = motion_fragment.chars();
    let Some(first_tail) = rest_chars.next() else {
        return pending_input_with_state(
            sequence,
            combined_count,
            Some(CorePendingArgumentKind::MotionOrTextObject),
        );
    };
    let tail_after_first = rest_chars.as_str();

    if motion_fragment.chars().count() == 1 && first_tail == operator {
        return CorePendingInput::none();
    }

    if tail_after_first.is_empty() && (first_tail == 'i' || first_tail == 'a' || first_tail == 'g')
    {
        return pending_input_with_state(
            sequence,
            combined_count,
            Some(CorePendingArgumentKind::MotionOrTextObject),
        );
    }

    if tail_after_first.is_empty() && matches!(first_tail, 'f' | 'F' | 't' | 'T' | '\'' | '`') {
        return pending_input_with_state(
            sequence,
            combined_count,
            Some(CorePendingArgumentKind::MotionOrTextObject),
        );
    }

    CorePendingInput::none()
}

fn pending_for_g_sequence(sequence: &str, rest: &str, count: Option<usize>) -> CorePendingInput {
    if rest.is_empty() {
        return pending_input_with_state(sequence, count, None);
    }

    if rest.chars().count() == 1 && matches!(rest.chars().next(), Some('q' | 'u' | 'U' | '~')) {
        return pending_input_with_state(
            sequence,
            count,
            Some(CorePendingArgumentKind::MotionOrTextObject),
        );
    }

    CorePendingInput::none()
}

pub(super) fn derive_direct_pending_input(
    command: &str,
    mode: CoreMode,
    native_pending: Option<CorePendingArgumentKind>,
) -> CorePendingInput {
    let pending_command = normalize_pending_command_fragment(command);

    if pending_command.is_empty() {
        return CorePendingInput::none();
    }

    let predicted_pending = if mode_uses_normal_sequence_grammar(mode) {
        pending_for_dispatch_sequence(pending_command)
    } else {
        CorePendingInput::none()
    };
    if predicted_pending.is_pending() {
        debug_log!(
            "[DEBUG] derive_direct_pending_input: predicted_pending command={:?} mode={:?} predicted={:?}",
            pending_command,
            mode,
            predicted_pending
        );
        return predicted_pending;
    }

    if let Some(awaited_argument) = native_pending
        .filter(|_| mode_uses_normal_sequence_grammar(mode))
        .filter(|_| pending_command.chars().count() == 1)
    {
        debug_log!(
            "[DEBUG] derive_direct_pending_input: native_pending command={:?} mode={:?} awaited={:?}",
            pending_command,
            mode,
            awaited_argument
        );
        return pending_input_with_keys(pending_command, Some(awaited_argument));
    }

    if mode == CoreMode::OperatorPending {
        debug_log!(
            "[DEBUG] derive_direct_pending_input: operator-pending command={:?}",
            pending_command
        );
        return pending_input_with_keys(
            pending_command,
            Some(CorePendingArgumentKind::MotionOrTextObject),
        );
    }

    if mode_uses_normal_sequence_grammar(mode) && pending_command == "g" {
        debug_log!(
            "[DEBUG] derive_direct_pending_input: prefix command={:?}",
            pending_command
        );
        return pending_input_with_keys(pending_command, None);
    }

    CorePendingInput::none()
}

fn normalize_pending_command_fragment(command: &str) -> &str {
    command.rsplit('\x1b').next().unwrap_or(command)
}

pub(super) fn derive_sequential_pending_input(
    previous_pending: &CorePendingInput,
    key: &str,
    mode: CoreMode,
    native_pending: Option<CorePendingArgumentKind>,
) -> CorePendingInput {
    let predicted_sequence = format!("{}{}", previous_pending.pending_keys, key);
    if previous_pending.is_pending() {
        let predicted_pending = pending_for_dispatch_sequence(&predicted_sequence);
        if predicted_pending.is_pending() {
            debug_log!(
                "[DEBUG] derive_sequential_pending_input: previous={:?} key={:?} mode={:?} native_pending={:?} predicted_sequence={:?} predicted_pending={:?}",
                previous_pending,
                key,
                mode,
                native_pending,
                predicted_sequence,
                predicted_pending
            );
            return predicted_pending;
        }
    }

    let next_count = next_count_state(previous_pending, key, mode);
    let key_was_count = next_count != previous_pending.count;
    let next_pending_keys = if key_was_count {
        previous_pending.pending_keys.clone()
    } else {
        format!("{}{}", previous_pending.pending_keys, key)
    };

    let next = if let Some(awaited_argument) = native_pending {
        pending_input_with_state(
            next_pending_keys.clone(),
            next_count,
            Some(awaited_argument),
        )
    } else if mode == CoreMode::OperatorPending {
        pending_input_with_state(
            next_pending_keys.clone(),
            next_count,
            Some(CorePendingArgumentKind::MotionOrTextObject),
        )
    } else if mode_uses_normal_sequence_grammar(mode) {
        let predicted = pending_for_dispatch_sequence(&next_pending_keys);
        if predicted.is_pending() {
            pending_input_with_state(
                next_pending_keys.clone(),
                next_count,
                predicted.awaited_argument,
            )
        } else if key_was_count {
            pending_input_with_state(
                next_pending_keys.clone(),
                next_count,
                awaited_argument_after_count(previous_pending, mode, native_pending),
            )
        } else {
            CorePendingInput::none()
        }
    } else {
        CorePendingInput::none()
    };

    debug_log!(
        "[DEBUG] derive_sequential_pending_input: previous={:?} key={:?} mode={:?} native_pending={:?} next_pending_keys={:?} next_count={:?} key_was_count={} awaited_after_count={:?} next={:?}",
        previous_pending,
        key,
        mode,
        native_pending,
        next_pending_keys,
        next_count,
        key_was_count,
        awaited_argument_after_count(previous_pending, mode, native_pending),
        next
    );

    next
}

pub(super) fn mode_uses_normal_sequence_grammar(mode: CoreMode) -> bool {
    matches!(
        mode,
        CoreMode::Normal
            | CoreMode::Visual
            | CoreMode::VisualLine
            | CoreMode::VisualBlock
            | CoreMode::Select
            | CoreMode::SelectLine
            | CoreMode::SelectBlock
            | CoreMode::OperatorPending
    )
}

fn parse_count_prefix(sequence: &str) -> (Option<usize>, &str, usize) {
    let mut count: Option<usize> = None;
    let mut consumed_bytes = 0;

    for (index, ch) in sequence.char_indices() {
        if !ch.is_ascii_digit() {
            break;
        }

        let digit = ch.to_digit(10).unwrap_or(0) as usize;
        if count.is_none() && digit == 0 {
            break;
        }

        count = Some(count.unwrap_or(0).saturating_mul(10).saturating_add(digit));
        consumed_bytes = index + ch.len_utf8();
    }

    (count, &sequence[consumed_bytes..], consumed_bytes)
}

fn combine_counts(left: Option<usize>, right: Option<usize>) -> Option<usize> {
    match (left, right) {
        (Some(lhs), Some(rhs)) => Some(lhs.saturating_mul(rhs)),
        (Some(lhs), None) => Some(lhs),
        (None, Some(rhs)) => Some(rhs),
        (None, None) => None,
    }
}

fn next_count_state(
    previous_pending: &CorePendingInput,
    key: &str,
    mode: CoreMode,
) -> Option<usize> {
    if !mode_uses_normal_sequence_grammar(mode) {
        return previous_pending.count;
    }

    let Some(digit) = key
        .chars()
        .next()
        .filter(|_| key.chars().count() == 1)
        .and_then(|value| value.to_digit(10))
        .map(|value| value as usize)
    else {
        return previous_pending.count;
    };

    let count_is_allowed = match previous_pending.awaited_argument {
        Some(CorePendingArgumentKind::Char)
        | Some(CorePendingArgumentKind::ReplaceChar)
        | Some(CorePendingArgumentKind::MarkSet)
        | Some(CorePendingArgumentKind::MarkJump)
        | Some(CorePendingArgumentKind::Register) => false,
        Some(CorePendingArgumentKind::MotionOrTextObject)
        | Some(CorePendingArgumentKind::NormalCommand)
        | None => true,
    };

    if !count_is_allowed {
        return previous_pending.count;
    }

    if previous_pending.count.is_none() && digit == 0 {
        return previous_pending.count;
    }

    Some(
        previous_pending
            .count
            .unwrap_or(0)
            .saturating_mul(10)
            .saturating_add(digit),
    )
}

fn awaited_argument_after_count(
    previous_pending: &CorePendingInput,
    mode: CoreMode,
    native_pending: Option<CorePendingArgumentKind>,
) -> Option<CorePendingArgumentKind> {
    native_pending.or_else(|| {
        if mode == CoreMode::OperatorPending {
            Some(CorePendingArgumentKind::MotionOrTextObject)
        } else {
            match previous_pending.awaited_argument {
                Some(CorePendingArgumentKind::MotionOrTextObject)
                | Some(CorePendingArgumentKind::NormalCommand) => previous_pending.awaited_argument,
                _ => None,
            }
        }
    })
}
