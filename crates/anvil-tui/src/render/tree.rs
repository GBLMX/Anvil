//! Workspace file tree rendering.

use super::*;
use crate::icon_theme;
use crate::theme;

pub(crate) fn render_tree(model: &Model, frame: &mut Frame, area: Rect) -> Option<Rect> {
    if !model.tree.visible || area.width <= 50 {
        return None;
    }
    let sidebar_width = (area.width / 5).clamp(18, 40).min(area.width - 20);
    let tree_area = Rect {
        width: sidebar_width,
        ..area
    };
    let mut lines = Vec::new();
    let visible_height = tree_area.height.saturating_sub(2) as usize;
    let start = model
        .tree
        .selected
        .unwrap_or(0)
        .saturating_sub(visible_height.saturating_sub(1));
    let ascii_icons = model.appearance.ascii_icons;
    for (index, entry) in model
        .tree
        .entries
        .iter()
        .enumerate()
        .skip(start)
        .take(visible_height)
    {
        let name = model.tree_entry_label(entry);
        let icon_name = entry
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let untrusted = model.tree_entry_untrusted(entry);
        let icon = if entry.is_dir {
            icon_theme::folder_icon(entry.expanded)
        } else {
            icon_theme::icon_for(&icon_name).unwrap_or_else(icon_theme::default_file_icon)
        };
        let glyph = model.appearance.icons.glyph(icon, ascii_icons);
        let icon_padding = if UnicodeWidthStr::width(glyph) == 1 {
            " "
        } else {
            ""
        };
        let style = if model.tree.selected == Some(index) {
            theme::selected()
        } else if untrusted {
            theme::panel().fg(theme::dim())
        } else if entry.is_dir {
            Style::default().fg(theme::accent()).bg(theme::surface())
        } else {
            theme::panel()
        };
        let icon_color = if untrusted {
            style.fg(theme::dim())
        } else {
            style.fg((icon.color)())
        };
        let mut spans = vec![
            Span::raw("  ".repeat(entry.depth)),
            Span::styled(glyph, icon_color),
            Span::raw(icon_padding),
            Span::styled(name, style),
        ];
        let modified = model.session.path.as_ref() == Some(&entry.path) && model.file.modified
            || model
                .buffers
                .entries
                .iter()
                .any(|buffer| buffer.path.as_ref() == Some(&entry.path) && buffer.modified)
            || model.buffers.split.as_ref().is_some_and(|pane| {
                pane.buffer.path.as_ref() == Some(&entry.path) && pane.buffer.modified
            });
        if modified {
            spans.push(Span::styled(
                " *",
                style.fg(theme::warning()).add_modifier(Modifier::BOLD),
            ));
        }
        let git_marker = if entry.is_dir {
            tree_rollup_git(model, &model.tree.entries, index)
        } else {
            model.git.status.get(&entry.path).copied()
        };
        if let Some(marker) = git_marker {
            let color = match marker {
                'A' => theme::success(),
                'D' => theme::error(),
                'R' | 'C' => theme::accent(),
                '?' => theme::warning(),
                _ => theme::warning(),
            };
            spans.push(Span::styled(
                format!(" {marker}"),
                style.fg(color).add_modifier(Modifier::BOLD),
            ));
        }
        let diagnostic_severity = if entry.is_dir {
            tree_rollup_diagnostic(model, &model.tree.entries, index)
        } else {
            model
                .language
                .diagnostics
                .by_path
                .get(&entry.path)
                .and_then(|diagnostics| {
                    diagnostics
                        .iter()
                        .filter_map(|diagnostic| diagnostic.severity)
                        .min()
                })
        };
        if let Some(severity) = diagnostic_severity {
            let color = match severity {
                1 => theme::error(),
                2 => theme::warning(),
                _ => theme::accent(),
            };
            spans.push(Span::styled(
                " !",
                style.fg(color).add_modifier(Modifier::BOLD),
            ));
        }
        lines.push(Line::from(spans));
    }
    if lines.is_empty() {
        lines.push(Line::styled("empty", theme::panel().fg(theme::muted())));
    }
    let tree_title = if model.tree.stale {
        "Files · stale"
    } else if model.tree.watch_limited {
        "Files · watch limit"
    } else if model.tree.show_hidden {
        "Files · hidden"
    } else {
        "Files"
    };
    let mut tree_block = panel_block(tree_title);
    if model.ui.focus.tree {
        tree_block = tree_block.border_style(theme::border(true));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::panel())
            .block(tree_block),
        tree_area,
    );
    Some(Rect {
        x: area.x + sidebar_width,
        width: area.width.saturating_sub(sidebar_width),
        ..area
    })
}

fn tree_rollup_git(model: &Model, entries: &[TreeEntry], index: usize) -> Option<char> {
    let depth = entries[index].depth;
    let mut marker = None;
    for child in entries.iter().skip(index + 1) {
        if child.depth <= depth {
            break;
        }
        if let Some(next) = model.git.status.get(&child.path)
            && git_priority(*next) > marker.map_or(0, git_priority)
        {
            marker = Some(*next);
        }
    }
    marker
}

fn git_priority(marker: char) -> u8 {
    match marker {
        'D' => 4,
        'R' | 'C' => 3,
        'A' => 2,
        'M' => 1,
        _ => 0,
    }
}

fn tree_rollup_diagnostic(model: &Model, entries: &[TreeEntry], index: usize) -> Option<u32> {
    let depth = entries[index].depth;
    let mut severity = None;
    for child in entries.iter().skip(index + 1) {
        if child.depth <= depth {
            break;
        }
        if let Some(diagnostics) = model.language.diagnostics.by_path.get(&child.path)
            && let Some(next) = diagnostics
                .iter()
                .filter_map(|diagnostic| diagnostic.severity)
                .min()
        {
            severity = Some(severity.map_or(next, |current: u32| current.min(next)));
        }
    }
    severity
}
