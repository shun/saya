//! メッセージペジャーの session 実装。

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MessagePagerAction {
    Enter,
    ForwardLine,
    ForwardHalfPage,
    ForwardPage,
    BackwardLine,
    BackwardHalfPage,
    BackwardPage,
    Top,
    Bottom,
    Dismiss,
}

impl MessagePagerAction {
    fn from_key(key: &KeyInput) -> Option<Self> {
        match key {
            KeyInput::Enter => Some(Self::Enter),
            KeyInput::Char('j') | KeyInput::Down => Some(Self::ForwardLine),
            KeyInput::Char('d') => Some(Self::ForwardHalfPage),
            KeyInput::Char(' ')
            | KeyInput::Char('f')
            | KeyInput::PageDown
            | KeyInput::Ctrl('f')
            | KeyInput::Ctrl('F') => Some(Self::ForwardPage),
            KeyInput::Char('k') | KeyInput::Up => Some(Self::BackwardLine),
            KeyInput::Char('u') => Some(Self::BackwardHalfPage),
            KeyInput::Char('b') | KeyInput::PageUp | KeyInput::Ctrl('b') | KeyInput::Ctrl('B') => {
                Some(Self::BackwardPage)
            }
            KeyInput::Char('g') => Some(Self::Top),
            KeyInput::Char('G') => Some(Self::Bottom),
            KeyInput::Escape | KeyInput::Ctrl('[') | KeyInput::Char('q') => Some(Self::Dismiss),
            _ => None,
        }
    }
}

fn normalize_message_pager_key(message: &str) -> String {
    message
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MessagePagerState {
    message_key: String,
    max_offset: u16,
    dismissed: bool,
}

impl EditorSessionState {
    pub fn sync_message_pager(&mut self, message: &str, visible_height: u16) -> bool {
        let normalized = normalize_message_pager_key(message);
        let line_count = normalized.lines().count();
        let visible_height = visible_height.max(1);
        let max_offset = u16::try_from(line_count.saturating_sub(usize::from(visible_height)))
            .unwrap_or(u16::MAX);
        let before_active = self.message_pager_active();
        let before_offset = self.message_scroll_offset;

        if normalized.is_empty() || line_count <= 1 {
            self.message_pager = None;
            self.message_scroll_offset = 0;
            log::debug!(
                "[editor_session] message pager cleared: reason=no_multiline_message, line_count={}, visible_height={}",
                line_count,
                visible_height
            );
            return before_active || before_offset != 0;
        }

        match self.message_pager.as_mut() {
            Some(pager) if pager.message_key == normalized => {
                pager.max_offset = max_offset;
                self.message_scroll_offset = self.message_scroll_offset.min(max_offset);
            }
            _ => {
                log::debug!(
                    "[editor_session] message pager activated: line_count={}, visible_height={}, max_offset={}",
                    line_count,
                    visible_height,
                    max_offset
                );
                self.message_scroll_offset = 0;
                self.message_pager = Some(MessagePagerState {
                    message_key: normalized,
                    max_offset,
                    dismissed: false,
                });
            }
        }

        before_active != self.message_pager_active() || before_offset != self.message_scroll_offset
    }

    pub fn message_pager_active(&self) -> bool {
        self.message_pager
            .as_ref()
            .is_some_and(|pager| !pager.dismissed)
    }

    pub fn message_pager_prompt_kind(&self) -> Option<CorePagerPromptKind> {
        let pager = self.message_pager.as_ref()?;
        if pager.dismissed {
            return None;
        }
        if self.message_scroll_offset >= pager.max_offset {
            Some(CorePagerPromptKind::HitReturn)
        } else {
            Some(CorePagerPromptKind::More)
        }
    }

    pub fn message_pager_hides_message(&self, message: &str) -> bool {
        let normalized = normalize_message_pager_key(message);
        self.message_pager.as_ref().is_some_and(|pager| {
            pager.dismissed && !normalized.is_empty() && pager.message_key == normalized
        })
    }

    pub fn reopen_message_pager(&mut self) -> Option<String> {
        let pager = self.message_pager.as_mut()?;
        pager.dismissed = false;
        self.message_scroll_offset = 0;
        log::debug!(
            "[editor_session] message pager reopened: max_offset={}, message_len={}",
            pager.max_offset,
            pager.message_key.len()
        );
        Some(pager.message_key.clone())
    }

    pub fn handle_message_pager_key(&mut self, key: &KeyInput) -> bool {
        let Some(action) = MessagePagerAction::from_key(key) else {
            return false;
        };
        if !self.message_pager_active() {
            return false;
        }
        let Some(pager) = self.message_pager.as_mut() else {
            return false;
        };

        let before = self.message_scroll_offset;
        match action {
            MessagePagerAction::Enter => {
                if self.message_scroll_offset >= pager.max_offset {
                    pager.dismissed = true;
                } else {
                    self.message_scroll_offset = self
                        .message_scroll_offset
                        .saturating_add(1)
                        .min(pager.max_offset);
                }
            }
            MessagePagerAction::ForwardLine => {
                self.message_scroll_offset = self
                    .message_scroll_offset
                    .saturating_add(1)
                    .min(pager.max_offset);
            }
            MessagePagerAction::ForwardHalfPage => {
                let delta = (self.message_area_height / 2).max(1);
                self.message_scroll_offset = self
                    .message_scroll_offset
                    .saturating_add(delta)
                    .min(pager.max_offset);
            }
            MessagePagerAction::ForwardPage => {
                self.message_scroll_offset = self
                    .message_scroll_offset
                    .saturating_add(self.message_area_height.max(1))
                    .min(pager.max_offset);
            }
            MessagePagerAction::BackwardLine => {
                self.message_scroll_offset = self.message_scroll_offset.saturating_sub(1);
            }
            MessagePagerAction::BackwardHalfPage => {
                let delta = (self.message_area_height / 2).max(1);
                self.message_scroll_offset = self.message_scroll_offset.saturating_sub(delta);
            }
            MessagePagerAction::BackwardPage => {
                self.message_scroll_offset = self
                    .message_scroll_offset
                    .saturating_sub(self.message_area_height.max(1));
            }
            MessagePagerAction::Top => {
                self.message_scroll_offset = 0;
            }
            MessagePagerAction::Bottom => {
                self.message_scroll_offset = pager.max_offset;
            }
            MessagePagerAction::Dismiss => {
                pager.dismissed = true;
            }
        }
        log::debug!(
            "[editor_session] message pager key handled: key={:?}, action={:?}, before={}, after={}, max_offset={}, active={}",
            key,
            action,
            before,
            self.message_scroll_offset,
            pager.max_offset,
            !pager.dismissed
        );
        true
    }

    pub fn scroll_message_area_by(&mut self, delta: i16, max_offset: u16) -> bool {
        let before = self.message_scroll_offset.min(max_offset);
        let after = if delta < 0 {
            before.saturating_sub(delta.unsigned_abs())
        } else {
            before.saturating_add(delta as u16).min(max_offset)
        };
        self.message_scroll_offset = after;
        log::debug!(
            "[editor_session] message area scroll: before={}, after={}, delta={}, max_offset={}",
            before,
            after,
            delta,
            max_offset
        );
        before != after
    }
}
