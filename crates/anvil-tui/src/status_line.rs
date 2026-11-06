//! Breadcrumbs, middle ellipsis, and stable status segments.

use super::Model;
use crate::lsp_ui::lsp_settings;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use unicode_width::UnicodeWidthStr;

const DIAGNOSTIC_STABLE: Duration = Duration::from_millis(200);
const STATUS_SEPARATOR: &str = " · ";

impl Model {
    pub(crate) fn tick_status(&mut self) {
        if !self.git_branch_checked {
            self.git_branch_checked = true;
            self.git_branch = detect_git_branch(&self.workspace);
            self.dirty = true;
        }
        if self.diagnostic_path != self.path {
            self.diagnostic_path = self.path.clone();
            self.diagnostic_count_shown = self.diagnostic_count_now();
            self.diagnostic_count_pending = None;
            self.dirty = true;
        }
        let actual = self.diagnostic_count_now();
        if let Some((pending, since)) = self.diagnostic_count_pending {
            if pending != actual {
                self.diagnostic_count_pending = Some((actual, Instant::now()));
                self.dirty = true;
            } else if since.elapsed() >= DIAGNOSTIC_STABLE {
                self.diagnostic_count_pending = None;
                if self.diagnostic_count_shown != actual {
                    self.diagnostic_count_shown = actual;
                }
                self.dirty = true;
            } else {
                self.spinner = self.spinner.wrapping_add(1);
                self.dirty = true;
            }
        } else if self.diagnostic_count_shown != actual {
            self.diagnostic_count_pending = Some((actual, Instant::now()));
            self.dirty = true;
        }
    }

    fn diagnostic_count_now(&self) -> usize {
        self.path
            .as_ref()
            .and_then(|path| self.diagnostics.get(path))
            .map_or(0, Vec::len)
    }

    fn relative_path_text(&self) -> String {
        let Some(path) = self.path.as_ref() else {
            return "[no file]".to_owned();
        };
        path.strip_prefix(&self.workspace)
            .unwrap_or(path)
            .display()
            .to_string()
    }

    pub(crate) fn breadcrumb_text(&self, width: usize) -> String {
        let path = self.relative_path_text();
        let symbol = self.path.as_ref().and_then(|path| {
            self.syntax
                .borrow()
                .scope_label(Some(path.as_path()), self.cursor)
                .map(str::to_owned)
        });
        breadcrumb_with_symbol(&path, symbol.as_deref(), width)
    }

    pub(crate) fn status_text(&self, width: usize) -> String {
        let width = width.max(1);
        if width == 1 {
            return "…".to_owned();
        }
        let mut segments = Vec::new();
        if !self.status.is_empty() {
            segments.push(self.status.clone());
        }
        segments.push(middle_ellipsis(
            &self.relative_path_text(),
            width.saturating_sub(3).min(32),
        ));
        if self.modified {
            segments.push("●".to_owned());
        }
        if let Some(branch) = &self.git_branch {
            segments.push(branch.clone());
        }
        let diagnostics = if self.diagnostic_count_pending.is_some() {
            format!(
                "diag [{} {}]",
                crate::render::SPINNER[self.spinner % crate::render::SPINNER.len()],
                self.diagnostic_count_shown
            )
        } else {
            format!("diag {}", self.diagnostic_count_shown)
        };
        segments.push(diagnostics);
        if let Some((tool, _)) = &self.lsp_config {
            segments.push(format!("lsp {tool}"));
        } else if self
            .path
            .as_ref()
            .is_some_and(|path| lsp_settings(path).is_some())
        {
            segments.push("lsp pending".to_owned());
        }
        let selections = self.editor.selection_bytes().len();
        if selections > 1 {
            segments.push(format!("sel {selections}"));
        }
        if let Some(position) = self
            .editor
            .index()
            .position(self.editor.text(), self.cursor)
        {
            segments.push(format!(
                "Ln {}, Col {}",
                position.line + 1,
                position.grapheme_column + 1
            ));
        }
        fit_segments(&segments, width)
    }
}

fn breadcrumb_with_symbol(path: &str, symbol: Option<&str>, width: usize) -> String {
    let Some(symbol) = symbol else {
        return middle_ellipsis(path, width);
    };
    const SEPARATOR_WIDTH: usize = 3;
    if width <= SEPARATOR_WIDTH + 4 {
        return middle_ellipsis(path, width);
    }
    let symbol_budget = (width / 3).clamp(4, 16);
    let path_budget = width - SEPARATOR_WIDTH - symbol_budget;
    if path_budget < 4 {
        return middle_ellipsis(path, width);
    }
    format!(
        "{} › {}",
        middle_ellipsis(path, path_budget),
        take_prefix_ellipsis(symbol, symbol_budget)
    )
}

pub(crate) fn middle_ellipsis(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(text) <= max_width {
        return text.to_owned();
    }
    if max_width <= 3 {
        return take_prefix(text, max_width);
    }
    let budget = max_width - 3;
    let (prefix, name) = match text.rsplit_once('/') {
        Some((prefix, name)) => (format!("{prefix}/"), name),
        None => (String::new(), text),
    };
    let suffix = take_suffix(name, budget);
    let prefix = take_prefix(
        &prefix,
        budget.saturating_sub(UnicodeWidthStr::width(suffix.as_str())),
    );
    format!("{prefix}...{suffix}")
}

fn take_prefix_ellipsis(text: &str, max_width: usize) -> String {
    if UnicodeWidthStr::width(text) <= max_width {
        return text.to_owned();
    }
    if max_width <= 1 {
        return take_prefix(text, max_width);
    }
    format!("{}…", take_prefix(text, max_width - 1))
}

fn fit_segments(segments: &[String], width: usize) -> String {
    let mut output = String::new();
    let mut used = 0;
    for segment in segments {
        if segment.is_empty() {
            continue;
        }
        let separator = if output.is_empty() { 0 } else { 3 };
        let segment_width = UnicodeWidthStr::width(segment.as_str());
        if used + separator + segment_width <= width {
            if !output.is_empty() {
                output.push_str(STATUS_SEPARATOR);
            }
            output.push_str(segment);
            used += separator + segment_width;
        } else if output.is_empty() {
            output = take_prefix(segment, width);
            break;
        } else {
            break;
        }
    }
    output
}

fn take_prefix(text: &str, max_width: usize) -> String {
    let mut width = 0;
    let mut end = 0;
    for character in text.chars() {
        let next = UnicodeWidthStr::width(character.to_string().as_str());
        if width + next > max_width {
            break;
        }
        width += next;
        end += character.len_utf8();
    }
    text[..end].to_owned()
}

fn take_suffix(text: &str, max_width: usize) -> String {
    let mut width = 0;
    let mut start = text.len();
    for (index, character) in text.char_indices().rev() {
        let next = UnicodeWidthStr::width(character.to_string().as_str());
        if width + next > max_width {
            break;
        }
        width += next;
        start = index;
    }
    text[start..].to_owned()
}

fn detect_git_branch(workspace: &Path) -> Option<String> {
    let dot_git = workspace.join(".git");
    let git_dir = if dot_git.is_dir() {
        dot_git
    } else {
        let contents = fs::read_to_string(&dot_git).ok()?;
        let target = contents
            .lines()
            .find_map(|line| line.strip_prefix("gitdir:"))
            .map(str::trim)?;
        let target = PathBuf::from(target);
        if target.is_absolute() {
            target
        } else {
            workspace.join(target)
        }
    };
    let head = fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    if let Some(branch) = head.strip_prefix("ref: refs/heads/") {
        return Some(branch.to_owned());
    }
    let short = head.chars().take(8).collect::<String>();
    (!short.is_empty()).then(|| format!("detached {short}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp;
    use anvil_editor::EditorState;

    #[test]
    fn middle_ellipsis_keeps_the_filename() {
        let clipped = middle_ellipsis("src/company/project/module/Button.tsx", 24);
        assert_eq!(UnicodeWidthStr::width(clipped.as_str()), 24);
        assert!(clipped.contains("..."));
        assert!(clipped.ends_with("Button.tsx"));
        assert!(clipped.starts_with("src/"));
    }

    #[test]
    fn middle_ellipsis_is_safe_at_tiny_widths() {
        assert_eq!(middle_ellipsis("abcdef", 0), "");
        assert_eq!(middle_ellipsis("abcdef", 1), "a");
        assert_eq!(middle_ellipsis("abcdef", 3), "abc");
    }

    #[test]
    fn long_symbols_do_not_drop_the_filename() {
        let text = breadcrumb_with_symbol(
            "src/company/project/module/Button.tsx",
            Some("very_long_function_name_that_would_swallow_the_path"),
            40,
        );
        assert!(UnicodeWidthStr::width(text.as_str()) <= 40);
        assert!(text.contains("Button.tsx"), "{text}");
        assert!(text.contains(" › "), "{text}");
        assert!(text.starts_with("src/"), "{text}");
    }

    #[test]
    fn diagnostics_use_hysteresis() {
        let mut model = Model::new(EditorState::from_text(""), None);
        model.path = Some(PathBuf::from("/ws/src/main.rs"));
        model.diagnostics.insert(
            PathBuf::from("/ws/src/main.rs"),
            vec![lsp::Diagnostic {
                range: lsp::Range {
                    start: lsp::Position {
                        line: 0,
                        character: 0,
                    },
                    end: lsp::Position {
                        line: 0,
                        character: 1,
                    },
                },
                severity: None,
                message: "test".to_owned(),
                source: None,
                extra: Default::default(),
            }],
        );
        model.diagnostic_path = model.path.clone();
        model.tick_status();
        assert_eq!(model.diagnostic_count_shown, 0);
        assert!(model.diagnostic_count_pending.is_some());

        model.diagnostic_count_pending = Some((
            1,
            Instant::now() - DIAGNOSTIC_STABLE - Duration::from_millis(1),
        ));
        model.tick_status();
        assert_eq!(model.diagnostic_count_shown, 1);
        assert!(model.diagnostic_count_pending.is_none());
    }

    #[test]
    fn status_segments_fit_small_widths() {
        let mut model = Model::new(EditorState::from_text("alpha"), None);
        model.path = Some(PathBuf::from("/ws/src/company/project/Button.tsx"));
        model.workspace = PathBuf::from("/ws");
        model.status = "saved".to_owned();
        for width in [1, 12, 40, 200] {
            let text = model.status_text(width);
            assert!(UnicodeWidthStr::width(text.as_str()) <= width);
        }
    }

    #[test]
    fn pending_diagnostics_keep_the_stable_count() {
        let mut model = Model::new(EditorState::from_text("alpha"), None);
        model.path = Some(PathBuf::from("/ws/src/main.rs"));
        model.workspace = PathBuf::from("/ws");
        model.diagnostic_count_shown = 2;
        model.diagnostic_count_pending = Some((5, Instant::now()));
        let text = model.status_text(200);
        assert!(text.contains("diag [⠋ 2]"), "{text}");
        assert!(!text.contains("diag [⠋ 5]"), "{text}");
    }

    #[test]
    fn breadcrumbs_fall_back_to_the_relative_path() {
        let mut model = Model::new(EditorState::from_text(""), None);
        model.workspace = PathBuf::from("/ws");
        model.path = Some(PathBuf::from("/ws/src/main.rs"));
        assert_eq!(model.breadcrumb_text(80), "src/main.rs");
    }

    #[test]
    fn detects_git_branch_from_head() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::create_dir(dir.path().join(".git")).expect("git dir");
        fs::write(dir.path().join(".git/HEAD"), "ref: refs/heads/main\n").expect("head");
        assert_eq!(detect_git_branch(dir.path()).as_deref(), Some("main"));
    }
}
