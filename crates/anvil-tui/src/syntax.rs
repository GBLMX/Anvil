use anvil_core::{TextBuffer, TextDocument};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};
use tree_sitter::{Language, Parser, Query, QueryCursor, StreamingIterator};

const MAX_HIGHLIGHT_BYTES: usize = 256 * 1024;
const DEBOUNCE: Duration = Duration::from_millis(50);

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
    fn matches(&self, revision: u64, path: Option<&Path>) -> bool {
        self.revision == revision && self.path.as_deref() == path
    }

    fn apply(&mut self, revision: u64, path: PathBuf, highlights: Vec<Highlight>) {
        self.revision = revision;
        self.path = Some(path);
        self.highlights = highlights;
    }

    fn clear(&mut self, revision: u64, path: Option<&Path>) {
        self.revision = revision;
        self.path = path.map(Path::to_path_buf);
        self.highlights.clear();
    }

    pub fn highlights(&self) -> &[Highlight] {
        &self.highlights
    }
}

struct ParseRequest {
    revision: u64,
    path: PathBuf,
    source: String,
}

struct ParseResult {
    revision: u64,
    path: PathBuf,
    highlights: Vec<Highlight>,
}

struct SyntaxWorker {
    requests: Sender<ParseRequest>,
    results: Receiver<ParseResult>,
}

impl SyntaxWorker {
    fn start() -> Self {
        let (requests, request_rx) = mpsc::channel::<ParseRequest>();
        let (result_tx, results) = mpsc::channel::<ParseResult>();
        std::thread::spawn(move || {
            for request in request_rx {
                let Some(highlights) = parse_source(&request.source, &request.path) else {
                    continue;
                };
                let _ = result_tx.send(ParseResult {
                    revision: request.revision,
                    path: request.path,
                    highlights,
                });
            }
        });
        Self { requests, results }
    }

    fn request(&self, request: ParseRequest) {
        let _ = self.requests.send(request);
    }

    fn try_result(&self) -> Option<ParseResult> {
        self.results.try_recv().ok()
    }
}

pub struct SyntaxState {
    cache: SyntaxCache,
    worker: SyntaxWorker,
    last_activity: Instant,
    in_flight: bool,
}

impl Default for SyntaxState {
    fn default() -> Self {
        Self {
            cache: SyntaxCache::default(),
            worker: SyntaxWorker::start(),
            last_activity: Instant::now(),
            in_flight: false,
        }
    }
}

impl SyntaxState {
    pub fn note_activity(&mut self) {
        self.last_activity = Instant::now();
    }

    /// Returns true when the cache changed and the caller should redraw.
    pub fn poll(&mut self, text: &TextDocument, revision: u64, path: Option<&Path>) -> bool {
        let mut changed = false;
        while let Some(result) = self.worker.try_result() {
            self.in_flight = false;
            if result.revision == revision && path == Some(result.path.as_path()) {
                self.cache
                    .apply(result.revision, result.path, result.highlights);
                changed = true;
            }
        }
        if self.cache.matches(revision, path) {
            return changed;
        }
        if self.in_flight || self.last_activity.elapsed() < DEBOUNCE {
            return changed;
        }
        let Some(path) = path else {
            self.cache.clear(revision, None);
            return true;
        };
        if text.len_bytes() > MAX_HIGHLIGHT_BYTES || language_for(path).is_none() {
            self.cache.clear(revision, Some(path));
            return true;
        }
        let source = text.slice(0..text.len_bytes());
        self.worker.request(ParseRequest {
            revision,
            path: path.to_path_buf(),
            source,
        });
        self.in_flight = true;
        changed
    }

    pub fn highlights(&self) -> &[Highlight] {
        self.cache.highlights()
    }
}

fn parse_source(source: &str, path: &Path) -> Option<Vec<Highlight>> {
    let (language, query) = language_for(path)?;
    let mut parser = Parser::new();
    parser.set_language(&language).ok()?;
    let tree = parser.parse(source, None)?;
    let mut highlights = Vec::new();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
    while let Some(captures) = matches.next() {
        for capture in captures.captures {
            let Some(class) = class_for(query.capture_names()[capture.index as usize]) else {
                continue;
            };
            let range = capture.node.byte_range();
            highlights.push(Highlight {
                start: range.start,
                end: range.end,
                class,
            });
        }
    }
    walk_keywords(tree.root_node(), &mut highlights);
    highlights.sort_by_key(|highlight| (highlight.start, highlight.end));
    Some(highlights)
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
        let highlights = parse_source(
            "fn main() { let x = \"s\"; let n = 1; } // c",
            Path::new("x.rs"),
        )
        .expect("parse");
        assert!(highlights.iter().any(|h| h.class == SyntaxClass::Keyword));
        assert!(highlights.iter().any(|h| h.class == SyntaxClass::String));
        assert!(highlights.iter().any(|h| h.class == SyntaxClass::Number));
        assert!(highlights.iter().any(|h| h.class == SyntaxClass::Comment));
    }

    #[test]
    fn json_highlights_keys_and_constants() {
        let highlights =
            parse_source(r#"{"key": true, "n": 1}"#, Path::new("x.json")).expect("parse");
        assert!(highlights.iter().any(|h| h.class == SyntaxClass::Property));
        assert!(highlights.iter().any(|h| h.class == SyntaxClass::Constant));
    }

    #[test]
    fn state_does_not_parse_inline_before_debounce() {
        let mut state = SyntaxState::default();
        let document = text("fn main() {}");
        state.note_activity();
        let changed = state.poll(&document, 1, Some(Path::new("x.rs")));
        assert!(!changed);
        assert!(state.highlights().is_empty());
    }
}
