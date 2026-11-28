//! Line-comment detection and toggle edits.

use std::ops::Range;
use std::path::Path;

pub const MAX_COMMENT_LINES: usize = 10_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommentStyle {
    pub line: &'static str,
}

/// Returns the line-comment marker for a supported file type.
///
/// HTML, XML, and CSS are block-comment-only and deliberately return `None`
/// rather than inserting an invalid line marker.
pub fn comment_style(path: &Path) -> Option<CommentStyle> {
    let name = path.file_name()?.to_str()?;
    if name == "go.mod" {
        return Some(CommentStyle { line: "//" });
    }
    if matches!(name, "Makefile" | "makefile" | "GNUmakefile" | "Dockerfile") {
        return Some(CommentStyle { line: "#" });
    }
    if matches!(
        name,
        ".zshrc"
            | ".zshenv"
            | ".zprofile"
            | ".zlogin"
            | ".zlogout"
            | ".zsh_aliases"
            | ".bashrc"
            | ".bash_profile"
            | ".bash_login"
            | ".bash_logout"
            | ".bash_aliases"
            | ".profile"
            | ".xprofile"
            | "zshrc"
            | "bashrc"
            | ".env"
            | ".editorconfig"
            | ".gitignore"
            | ".gitattributes"
            | ".dockerignore"
            | ".npmrc"
            | ".prettierignore"
            | ".eslintignore"
    ) {
        return Some(CommentStyle { line: "#" });
    }
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "rs" | "go" | "jsonc" | "c" | "h" | "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" | "cs"
        | "java" | "kt" | "kts" | "swift" | "scala" | "dart" | "php" | "js" | "jsx" | "mjs"
        | "cjs" | "ts" | "mts" | "cts" | "tsx" | "m" | "mm" => Some(CommentStyle { line: "//" }),
        "sql" | "lua" | "hs" | "lhs" => Some(CommentStyle { line: "--" }),
        "sh" | "bash" | "zsh" | "brush" | "fish" | "py" | "toml" | "yaml" | "yml" | "mk" | "rb"
        | "rake" | "gemspec" => Some(CommentStyle { line: "#" }),
        _ => None,
    }
}

/// Builds non-overlapping line-prefix edits for toggling `marker`.
///
/// A line is considered commented when its content after indentation starts
/// with the marker. Empty or whitespace-only lines do not decide the toggle
/// direction and are left untouched when removing.
pub fn toggle_edits(
    text: &str,
    lines: &[Range<usize>],
    marker: &str,
) -> Option<Vec<(Range<usize>, String)>> {
    if !matches!(marker, "//" | "#" | "--") || lines.is_empty() || lines.len() > MAX_COMMENT_LINES {
        return None;
    }
    let mut lines = lines.to_vec();
    lines.sort_by_key(|line| line.start);
    lines.dedup_by_key(|line| line.start);
    if lines.iter().any(|line| {
        line.start > line.end
            || line.end > text.len()
            || !text.is_char_boundary(line.start)
            || !text.is_char_boundary(line.end)
    }) || lines.windows(2).any(|pair| pair[0].end > pair[1].start)
    {
        return None;
    }
    let selected = lines
        .iter()
        .filter(|line| !line_content(text, line).trim().is_empty())
        .collect::<Vec<_>>();
    let all_commented =
        !selected.is_empty() && selected.iter().all(|line| is_commented(text, line, marker));
    let mut edits = Vec::new();
    for line in &lines {
        let content = line_content(text, line);
        let indent = content
            .bytes()
            .take_while(|byte| matches!(byte, b' ' | b'\t'))
            .count();
        let at = line.start + indent;
        if all_commented {
            if is_commented(text, line, marker) {
                let mut end = at + marker.len();
                if text[end..].starts_with(' ') {
                    end += 1;
                }
                edits.push((at..end, String::new()));
            }
        } else if !is_commented(text, line, marker) {
            edits.push((at..at, format!("{marker} ")));
        }
    }
    (!edits.is_empty()).then_some(edits)
}

fn line_content<'a>(text: &'a str, line: &Range<usize>) -> &'a str {
    let mut end = line.end;
    if end > line.start && text.as_bytes().get(end - 1) == Some(&b'\n') {
        end -= 1;
    }
    if end > line.start && text.as_bytes().get(end - 1) == Some(&b'\r') {
        end -= 1;
    }
    text.get(line.start..end).unwrap_or_default()
}

fn is_commented(text: &str, line: &Range<usize>, marker: &str) -> bool {
    line_content(text, line)
        .trim_start_matches([' ', '\t'])
        .starts_with(marker)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn lines(text: &str) -> Vec<Range<usize>> {
        text.split_inclusive('\n')
            .scan(0, |start, line| {
                let range = *start..*start + line.len();
                *start += line.len();
                Some(range)
            })
            .collect()
    }

    #[test]
    fn detects_supported_comment_styles() {
        assert_eq!(
            comment_style(Path::new("src/main.rs")),
            Some(CommentStyle { line: "//" })
        );
        assert_eq!(
            comment_style(Path::new("scripts/run.sh")),
            Some(CommentStyle { line: "#" })
        );
        assert_eq!(
            comment_style(Path::new("Cargo.toml")),
            Some(CommentStyle { line: "#" })
        );
        assert_eq!(
            comment_style(Path::new("src/Main.hs")),
            Some(CommentStyle { line: "--" })
        );
        assert_eq!(comment_style(Path::new("data.json")), None);
        assert_eq!(comment_style(Path::new("notes.md")), None);
    }

    #[test]
    fn toggles_lines_and_preserves_indent() {
        let text = "  one\n\ttwo\n";
        let edits = toggle_edits(text, &lines(text), "//").expect("edits");
        assert_eq!(
            edits,
            vec![(2..2, "// ".to_owned()), (7..7, "// ".to_owned()),]
        );
    }

    #[test]
    fn removes_marker_and_one_space() {
        let text = "// one\n  // two\n";
        let edits = toggle_edits(text, &lines(text), "//").expect("edits");
        assert_eq!(edits, vec![(0..3, String::new()), (9..12, String::new())]);
    }

    #[test]
    fn mixed_selection_only_comments_uncommented_lines() {
        let text = "// one\ntwo\n";
        let edits = toggle_edits(text, &lines(text), "//").expect("edits");
        assert_eq!(edits, vec![(7..7, "// ".to_owned())]);
    }

    #[test]
    fn blank_lines_do_not_block_removal() {
        let text = "// one\n\n// two\n";
        let edits = toggle_edits(text, &lines(text), "//").expect("edits");
        assert_eq!(edits.len(), 2);
    }

    #[test]
    fn empty_buffer_gets_a_marker() {
        let text = "";
        let line = 0..0;
        let edits = toggle_edits(text, std::slice::from_ref(&line), "//").expect("edits");
        assert_eq!(edits, vec![(0..0, "// ".to_owned())]);
    }

    #[test]
    fn unknown_file_has_no_style() {
        assert_eq!(comment_style(Path::new("file.unknown")), None);
    }

    #[test]
    fn covers_dotfiles_and_common_line_comment_languages() {
        for (path, marker) in [
            (".bashrc", "#"),
            (".zshenv", "#"),
            (".zlogin", "#"),
            (".bash_aliases", "#"),
            (".xprofile", "#"),
            ("bashrc", "#"),
            (".env", "#"),
            ("main.c", "//"),
            ("main.cpp", "//"),
            ("Main.java", "//"),
            ("Main.kt", "//"),
            ("main.swift", "//"),
            ("query.sql", "--"),
            ("init.lua", "--"),
            ("app.rb", "#"),
            ("index.php", "//"),
            ("main.mts", "//"),
            ("main.cts", "//"),
            ("run.brush", "#"),
        ] {
            assert_eq!(
                comment_style(Path::new(path)),
                Some(CommentStyle { line: marker }),
                "{path}"
            );
        }
        assert_eq!(comment_style(Path::new("index.html")), None);
        assert_eq!(comment_style(Path::new("style.css")), None);
        assert_eq!(comment_style(Path::new("data.xml")), None);
    }

    #[test]
    fn rejects_overlapping_out_of_bounds_and_unknown_marker_inputs() {
        let text = "one\ntwo\n";
        assert!(toggle_edits(text, &[0..5, 3..8], "//").is_none());
        let out_of_bounds = 0..text.len() + 1;
        assert!(toggle_edits(text, std::slice::from_ref(&out_of_bounds), "//").is_none());
        assert!(toggle_edits(text, &lines(text), "-").is_none());
        assert!(toggle_edits(text, &lines(text), "").is_none());
    }

    #[test]
    fn sql_marker_does_not_match_a_single_dash() {
        let text = "-123\n";
        let edits = toggle_edits(text, &lines(text), "--").expect("edits");
        assert_eq!(edits, vec![(0..0, "-- ".to_owned())]);
    }

    #[test]
    fn removes_crlf_comments_without_touching_the_line_ending() {
        let text = "// one\r\n//\r\n";
        let edits = toggle_edits(text, &lines(text), "//").expect("edits");
        assert_eq!(edits, vec![(0..3, String::new()), (8..10, String::new())]);
    }
}
