//! Workspace file tree rendering.

use super::*;
use crate::icon_theme;
use crate::layout::tree_fits;
use crate::theme;

pub(crate) fn render_tree(model: &Model, frame: &mut Frame, area: Rect) -> Option<Rect> {
    if !model.workspace.tree.visible || !tree_fits(area.width) {
        return None;
    }
    let sidebar_width = model.sidebar_width(area.width);
    let tree_area = Rect {
        width: sidebar_width,
        ..area
    };
    let header_area = Rect {
        height: tree_area.height.min(1),
        ..tree_area
    };
    let list_area = Rect {
        y: tree_area.y.saturating_add(header_area.height),
        height: tree_area.height.saturating_sub(header_area.height),
        ..tree_area
    };
    let mut lines = Vec::new();
    let rollups = tree_rollups(model);
    let visible_height = list_area.height as usize;
    let start = crate::layout::tree_viewport_start(model.workspace.tree.selected, visible_height);
    let ascii_icons = model.appearance.ascii_icons;
    for (index, entry) in model
        .workspace
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
        let find_match = model
            .workspace
            .tree
            .find
            .as_ref()
            .is_some_and(|find| find.matches.contains(&index));
        let style = if model.workspace.tree.selected == Some(index) {
            theme::selected()
        } else if find_match {
            theme::panel()
                .fg(theme::warning())
                .add_modifier(Modifier::BOLD)
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
        let modified =
            model.session.path.as_ref() == Some(&entry.path) && model.file.modified
                || model.buffers.documents.values().any(|document| {
                    document.path.as_ref() == Some(&entry.path) && document.modified
                });
        if modified {
            spans.push(Span::styled(
                " *",
                style.fg(theme::warning()).add_modifier(Modifier::BOLD),
            ));
        }
        let git_marker = rollups.git[index];
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
        let diagnostic_severity = rollups.diagnostic[index];
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
    let tree_title = if model.workspace.tree.stale {
        "Files · stale"
    } else if model.workspace.tree.watch_limited {
        "Files · watch limit"
    } else if model.workspace.tree.show_hidden {
        "Files · hidden"
    } else {
        "Files"
    };
    let title_style = if model.ui.focus.tree {
        theme::title().fg(theme::accent())
    } else {
        theme::panel().fg(theme::muted())
    };
    frame.render_widget(
        Paragraph::new(Line::styled(format!(" {tree_title}"), title_style)).style(theme::panel()),
        header_area,
    );
    if list_area.height > 0 {
        frame.render_widget(Paragraph::new(lines).style(theme::panel()), list_area);
    }
    Some(Rect {
        x: area.x + sidebar_width,
        width: area.width.saturating_sub(sidebar_width),
        ..area
    })
}

struct TreeRollups {
    git: Vec<Option<char>>,
    diagnostic: Vec<Option<u32>>,
}

fn tree_rollups(model: &Model) -> TreeRollups {
    let entries = &model.workspace.tree.entries;
    let mut rollups = TreeRollups {
        git: vec![None; entries.len()],
        diagnostic: vec![None; entries.len()],
    };
    let mut stack: Vec<(usize, usize, Option<char>, Option<u32>)> = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        while stack
            .last()
            .is_some_and(|(depth, _, _, _)| *depth >= entry.depth)
        {
            let (_, child_index, child_git, child_diagnostic) = stack.pop().expect("checked stack");
            rollups.git[child_index] = child_git;
            rollups.diagnostic[child_index] = child_diagnostic;
            if let Some((_, _, parent_git, parent_diagnostic)) = stack.last_mut() {
                merge_git(parent_git, child_git);
                merge_diagnostic(parent_diagnostic, child_diagnostic);
            }
        }
        let git = (!entry.is_dir)
            .then(|| model.workspace.git.status.get(&entry.path).copied())
            .flatten();
        let diagnostic = (!entry.is_dir)
            .then(|| {
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
            })
            .flatten();
        stack.push((entry.depth, index, git, diagnostic));
    }
    while let Some((_, index, git, diagnostic)) = stack.pop() {
        rollups.git[index] = git;
        rollups.diagnostic[index] = diagnostic;
        if let Some((_, _, parent_git, parent_diagnostic)) = stack.last_mut() {
            merge_git(parent_git, git);
            merge_diagnostic(parent_diagnostic, diagnostic);
        }
    }
    rollups
}

fn merge_git(target: &mut Option<char>, candidate: Option<char>) {
    if let Some(next) = candidate
        && git_priority(next) > target.map_or(0, git_priority)
    {
        *target = Some(next);
    }
}

fn merge_diagnostic(target: &mut Option<u32>, candidate: Option<u32>) {
    if let Some(next) = candidate {
        *target = Some(target.map_or(next, |current| current.min(next)));
    }
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
