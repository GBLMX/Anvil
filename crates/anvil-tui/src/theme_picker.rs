//! Theme preview and cancel ownership, independent of list selection.

use crate::theme::{self, Choice};

#[derive(Clone, Debug)]
pub(crate) struct ThemePicker {
    original: Choice,
}

impl ThemePicker {
    pub(crate) fn new() -> Self {
        Self {
            original: theme::choice(),
        }
    }

    /// Applies a non-persistent preview; `None` restores the original choice.
    pub(crate) fn preview(&self, name: Option<&'static str>) {
        if let Some(name) = name {
            let _ = theme::apply_choice(Choice::Fixed(name), false);
        } else {
            self.cancel();
        }
    }

    pub(crate) fn cancel(&self) {
        let _ = theme::apply_choice(self.original, false);
    }

    pub(crate) fn commit(&self, name: &str) -> Result<(), String> {
        theme::set_theme(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_cancel_and_commit_are_owned_here() {
        theme::set_startup_theme("anvil-dark").expect("reset");
        let picker = ThemePicker::new();
        picker.preview(Some("nord"));
        assert_eq!(theme::name(), "nord");
        picker.cancel();
        assert_eq!(theme::name(), "anvil-dark");
        picker.commit("solarized-light").expect("commit");
        assert_eq!(theme::name(), "solarized-light");
        theme::set_startup_theme("anvil-dark").expect("reset");
    }
}
