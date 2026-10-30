use anvil_core::{TextBuffer, TextDocument};
use std::path::{Path, PathBuf};
use tree_sitter::{Language, Parser, Query, QueryCursor, StreamingIterator};

const MAX_HIGHLIGHT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntaxClass {
    Keyword,
    String,
    Comment,
    Number,
    Function,
    Type,
    Property,
    Constant,
}

#[derive(Clone, Debug)]
pub struct Highlight {
    pub start: usize,
    pub end: usize,
    pub class: SyntaxClass,
}

#[derive(Default)]
pub struct SyntaxCache {
    revision: u64,
    path: Option<PathBuf>,
    highlights: Vec<Highlight>,
}

impl SyntaxCache {
    pub fn ensure(
        &mut self,
        text: &TextDocument,
        revision: u64,
        path: Option<&Path>,
    ) -> &[Highlight] {
        if self.revision == revision && self.path.as_deref() == path {
            return &self.highlights;
        }
        self.revision = revision;
        self.path = path.map(Path::to_path_buf);
        self.highlights.clear();
        let Some(path) = path else {
            return &self.highlights;
        };
        if text.len_bytes() > MAX_HIGHLIGHT_BYTES {
            return &self.highlights;
        }
        let Some((language, query)) = language_for(path) else {
            return &self.highlights;
        };
        let source = text.slice(0..text.len_bytes());
        let mut parser = Parser::new();
        if parser.set_language(&language).is_err() {
            return &self.highlights;
        }
        let Some(tree) = parser.parse(&source, None) else {
            return &self.highlights;
        };
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
        while let Some(captures) = matches.next() {
            for capture in captures.captures {
                let Some(class) = class_for(query.capture_names()[capture.index as usize]) else {
                    continue;
                };
                let range = capture.node.byte_range();
                self.highlights.push(Highlight {
                    start: range.start,
                    end: range.end,
                    class,
                });
            }
        }
        walk_keywords(tree.root_node(), &mut self.highlights);
        self.highlights
            .sort_by_key(|highlight| (highlight.start, highlight.end));
        &self.highlights
    }
}

const KEYWORDS: &[&str] = &[
    "fn", "let", "mut", "const", "static", "struct", "enum", "impl", "trait", "for", "while",
    "loop", "if", "else", "match", "return", "use", "pub", "mod", "crate", "self", "Self", "async",
    "await", "move", "ref", "where", "unsafe", "extern", "type", "as", "in", "break", "continue",
    "dyn", "true", "false", "null",
];

fn walk_keywords(node: tree_sitter::Node<'_>, highlights: &mut Vec<Highlight>) {
    if node.child_count() == 0 {
        let kind = node.kind();
        if KEYWORDS.contains(&kind)
            && (!node.is_named() || matches!(kind, "true" | "false" | "null"))
        {
            let range = node.byte_range();
            highlights.push(Highlight {
                start: range.start,
                end: range.end,
                class: if matches!(kind, "true" | "false" | "null") {
                    SyntaxClass::Constant
                } else {
                    SyntaxClass::Keyword
                },
            });
        }
        return;
    }
    for index in 0..node.child_count() {
        if let Some(child) = node.child(index) {
            walk_keywords(child, highlights);
        }
    }
}

fn class_for(name: &str) -> Option<SyntaxClass> {
    Some(match name {
        "keyword" => SyntaxClass::Keyword,
        "string" => SyntaxClass::String,
        "comment" => SyntaxClass::Comment,
        "number" => SyntaxClass::Number,
        "function" => SyntaxClass::Function,
        "type" => SyntaxClass::Type,
        "property" => SyntaxClass::Property,
        "constant" => SyntaxClass::Constant,
        _ => return None,
    })
}

fn language_for(path: &Path) -> Option<(Language, Query)> {
    let extension = path.extension()?.to_str()?;
    let (language, query) = match extension {
        "rs" => (
            Language::new(tree_sitter_rust::LANGUAGE),
            include_str!("syntax/rust.scm"),
        ),
        "json" => (
            Language::new(tree_sitter_json::LANGUAGE),
            include_str!("syntax/json.scm"),
        ),
        _ => return None,
    };
    let query = Query::new(&language, query).ok()?;
    Some((language, query))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> TextDocument {
        TextDocument::from_text(value)
    }

    #[test]
    fn rust_highlights_keywords_strings_numbers() {
        let mut cache = SyntaxCache::default();
        let highlights = cache.ensure(
            &text("fn main() { let x = \"s\"; let n = 1; } // c"),
            1,
            Some(Path::new("x.rs")),
        );
        assert!(highlights.iter().any(|h| h.class == SyntaxClass::Keyword));
        assert!(highlights.iter().any(|h| h.class == SyntaxClass::String));
        assert!(highlights.iter().any(|h| h.class == SyntaxClass::Number));
        assert!(highlights.iter().any(|h| h.class == SyntaxClass::Comment));
    }

    #[test]
    fn json_highlights_keys_and_constants() {
        let mut cache = SyntaxCache::default();
        let highlights = cache.ensure(
            &text(r#"{"key": true, "n": 1}"#),
            1,
            Some(Path::new("x.json")),
        );
        assert!(highlights.iter().any(|h| h.class == SyntaxClass::Property));
        assert!(highlights.iter().any(|h| h.class == SyntaxClass::Constant));
    }
}
