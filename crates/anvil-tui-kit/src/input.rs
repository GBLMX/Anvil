//! Terminal key normalization and plain-input classification.

use std::ops::Deref;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// Whether an event should be dispatched as an editing action.
pub fn is_action_key(key: &KeyEvent) -> bool {
    matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat)
}

/// A terminal key with the modifier queries used by dispatch.
#[derive(Clone, Copy)]
pub struct KeyPress(KeyEvent);

impl KeyPress {
    pub fn from_key(key: KeyEvent) -> Self {
        Self(key)
    }

    pub fn event(self) -> KeyEvent {
        self.0
    }

    pub fn ctrl(self) -> bool {
        self.0.modifiers.contains(KeyModifiers::CONTROL)
    }

    pub fn alt(self) -> bool {
        self.0.modifiers.contains(KeyModifiers::ALT)
    }

    pub fn super_key(self) -> bool {
        self.0.modifiers.contains(KeyModifiers::SUPER)
    }

    /// Plain or shifted character input, excluding command modifiers.
    pub fn text_input(self) -> bool {
        !self.ctrl() && !self.alt() && !self.super_key()
    }

    /// Character form used for command bindings.
    ///
    /// Terminals disagree on whether Alt-modified letters arrive uppercase or
    /// lowercase; command matching deliberately folds that difference.
    pub fn command_char(self) -> Option<char> {
        match self.0.code {
            KeyCode::Char(character) if self.alt() => Some(character.to_ascii_lowercase()),
            KeyCode::Char(character) => Some(character),
            _ => None,
        }
    }
}

impl Deref for KeyPress {
    type Target = KeyEvent;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyPress {
        KeyPress::from_key(KeyEvent::new(code, modifiers))
    }

    #[test]
    fn alt_command_chars_fold_case() {
        assert_eq!(
            key(KeyCode::Char('C'), KeyModifiers::ALT).command_char(),
            Some('c')
        );
        assert_eq!(
            key(KeyCode::Char('c'), KeyModifiers::ALT).command_char(),
            Some('c')
        );
    }

    #[test]
    fn text_input_rejects_command_modifiers() {
        assert!(key(KeyCode::Char('c'), KeyModifiers::SHIFT).text_input());
        assert!(!key(KeyCode::Char('c'), KeyModifiers::ALT).text_input());
        assert!(!key(KeyCode::Char('c'), KeyModifiers::CONTROL).text_input());
        assert!(!key(KeyCode::Char('c'), KeyModifiers::SUPER).text_input());
    }

    #[test]
    fn release_is_not_an_action_key() {
        let release = KeyEvent::new_with_kind(
            KeyCode::Char('c'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        );
        assert!(!is_action_key(&release));
    }
}
