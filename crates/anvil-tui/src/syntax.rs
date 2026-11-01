use anvil_core::{TextBuffer, TextDocument};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};
use tree_sitter::{Language, Parser, Query, QueryCursor, StreamingIterator};

const MAX_HIGHLIGHT_BYTES: usize = 256 * 1024;
const DEBOUNCE: Duration = Duration::from_millis(50);

#[derive(Clone, Copy)]
enum SyntaxKind {
    TreeSitter,
    GoMod,
}

struct SyntaxLanguage {
    kind: SyntaxKind,
    language: Option<Language>,
    query: Option<Query>,
    rust_keywords: bool,
}

impl SyntaxLanguage {
    fn grammar(language: Language, query: &str, rust_keywords: bool) -> Self {
        let compiled = Query::new(&language, query).ok();
        Self {
            kind: SyntaxKind::TreeSitter,
            language: Some(language),
            query: compiled,
            rust_keywords,
        }
    }

    fn go_mod() -> Self {
        Self {
            kind: SyntaxKind::GoMod,
            language: None,
            query: None,
            rust_keywords: false,
        }
    }
}

static RUST: LazyLock<SyntaxLanguage> = LazyLock::new(|| {
    SyntaxLanguage::grammar(
        Language::new(tree_sitter_rust::LANGUAGE),
        include_str!("syntax/rust.scm"),
        true,
    )
});
static JSON: LazyLock<SyntaxLanguage> = LazyLock::new(|| {
    SyntaxLanguage::grammar(
        Language::new(tree_sitter_json::LANGUAGE),
        include_str!("syntax/json.scm"),
        false,
    )
});
static GO: LazyLock<SyntaxLanguage> = LazyLock::new(|| {
    SyntaxLanguage::grammar(
        Language::new(tree_sitter_go::LANGUAGE),
        tree_sitter_go::HIGHLIGHTS_QUERY,
        false,
    )
});
static BASH: LazyLock<SyntaxLanguage> = LazyLock::new(|| {
    SyntaxLanguage::grammar(
        Language::new(tree_sitter_bash::LANGUAGE),
        tree_sitter_bash::HIGHLIGHT_QUERY,
        false,
    )
});
static PYTHON: LazyLock<SyntaxLanguage> = LazyLock::new(|| {
    SyntaxLanguage::grammar(
        Language::new(tree_sitter_python::LANGUAGE),
        tree_sitter_python::HIGHLIGHTS_QUERY,
        false,
    )
});
static TOML: LazyLock<SyntaxLanguage> = LazyLock::new(|| {
    SyntaxLanguage::grammar(
        Language::new(tree_sitter_toml_ng::LANGUAGE),
        include_str!("syntax/toml.scm"),
        false,
    )
});
static YAML: LazyLock<SyntaxLanguage> = LazyLock::new(|| {
    SyntaxLanguage::grammar(
        Language::new(tree_sitter_yaml::LANGUAGE),
        tree_sitter_yaml::HIGHLIGHTS_QUERY,
        false,
    )
});
static MAKE: LazyLock<SyntaxLanguage> = LazyLock::new(|| {
    SyntaxLanguage::grammar(
        Language::new(tree_sitter_make::LANGUAGE),
        tree_sitter_make::HIGHLIGHTS_QUERY,
        false,
    )
});
static DOCKERFILE: LazyLock<SyntaxLanguage> = LazyLock::new(|| {
    SyntaxLanguage::grammar(
        tree_sitter_dockerfile_updated::language(),
        include_str!("syntax/dockerfile.scm"),
        false,
    )
});
static GO_MOD: LazyLock<SyntaxLanguage> = LazyLock::new(SyntaxLanguage::go_mod);

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
    epoch: u64,
    revision: u64,
    path: PathBuf,
    source: String,
}

struct ParseResult {
    epoch: u64,
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
                    epoch: request.epoch,
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
    epoch: u64,
}

impl Default for SyntaxState {
    fn default() -> Self {
        Self {
            cache: SyntaxCache::default(),
            worker: SyntaxWorker::start(),
            last_activity: Instant::now(),
            in_flight: false,
            epoch: 0,
        }
    }
}

impl SyntaxState {
    pub fn note_activity(&mut self) {
        self.last_activity = Instant::now();
    }

    pub fn invalidate(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.cache.clear(0, None);
        self.in_flight = false;
    }

    /// Returns true when the cache changed and the caller should redraw.
    pub fn poll(&mut self, text: &TextDocument, revision: u64, path: Option<&Path>) -> bool {
        let mut changed = false;
        while let Some(result) = self.worker.try_result() {
            self.in_flight = false;
            if result.epoch == self.epoch
                && result.revision == revision
                && path == Some(result.path.as_path())
            {
                self.cache
                    .apply(result.revision, result.path, result.highlights);
                changed = true;
            }
        }
        let path_changed = self.cache.path.as_deref() != path;
        if path_changed {
            self.cache.clear(u64::MAX, path);
            changed = true;
        }
        if !path_changed && self.cache.matches(revision, path) {
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
            epoch: self.epoch,
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
    let definition = language_for(path)?;
    let mut highlights = match definition.kind {
        SyntaxKind::GoMod => parse_go_mod(source),
        SyntaxKind::TreeSitter => {
            let language = definition.language.as_ref()?;
            let query = definition.query.as_ref()?;
            let mut parser = Parser::new();
            parser.set_language(language).ok()?;
            let tree = parser.parse(source, None)?;
            let mut highlights = Vec::new();
            let mut cursor = QueryCursor::new();
            let mut matches = cursor.matches(query, tree.root_node(), source.as_bytes());
            while let Some(captures) = matches.next() {
                for capture in captures.captures {
                    let Some(class) = class_for(query.capture_names()[capture.index as usize])
                    else {
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
            if definition.rust_keywords {
                walk_keywords(tree.root_node(), &mut highlights);
            }
            highlights
        }
    };
    highlights.sort_by_key(|highlight| (highlight.start, highlight.end));
    Some(highlights)
}

fn parse_go_mod(source: &str) -> Vec<Highlight> {
    let mut highlights = Vec::new();
    let mut offset = 0;
    for line in source.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        let leading = line.len() - line.trim_start().len();
        let body = line[leading..].trim_end_matches(['\r', '\n']);
        if body.is_empty() {
            continue;
        }
        let body_start = start + leading;
        if body.starts_with("//") {
            highlights.push(Highlight {
                start: body_start,
                end: body_start + body.len(),
                class: SyntaxClass::Comment,
            });
            continue;
        }
        let first_end = body.find(char::is_whitespace).unwrap_or(body.len());
        if matches!(
            &body[..first_end],
            "module" | "go" | "toolchain" | "require" | "replace" | "exclude" | "retract"
        ) {
            highlights.push(Highlight {
                start: body_start,
                end: body_start + first_end,
                class: SyntaxClass::Keyword,
            });
        }
        let mut quote_start = None;
        for (index, character) in body.char_indices() {
            if character != '"' {
                continue;
            }
            if let Some(quote_start) = quote_start.take() {
                highlights.push(Highlight {
                    start: body_start + quote_start,
                    end: body_start + index + 1,
                    class: SyntaxClass::String,
                });
            } else {
                quote_start = Some(index);
            }
        }
        if let Some(quote_start) = quote_start {
            highlights.push(Highlight {
                start: body_start + quote_start,
                end: body_start + body.len(),
                class: SyntaxClass::String,
            });
        }
    }
    highlights
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
        "string" | "string.special" | "string.regex" | "escape" => SyntaxClass::String,
        "comment" => SyntaxClass::Comment,
        "number" => SyntaxClass::Number,
        "function" | "function.builtin" | "function.method" | "constructor" => {
            SyntaxClass::Function
        }
        "type" => SyntaxClass::Type,
        "property" | "attribute" | "label" => SyntaxClass::Property,
        "constant" | "constant.builtin" | "constant.macro" | "boolean" => SyntaxClass::Constant,
        _ => return None,
    })
}

fn language_for(path: &Path) -> Option<&'static SyntaxLanguage> {
    let file_name = path.file_name()?.to_str()?;
    match file_name {
        "Makefile" | "makefile" => return Some(&*MAKE),
        "Dockerfile" => return Some(&*DOCKERFILE),
        "go.mod" => return Some(&*GO_MOD),
        _ => {}
    }
    match path.extension()?.to_str()? {
        "rs" => Some(&*RUST),
        "json" | "jsonc" => Some(&*JSON),
        "go" => Some(&*GO),
        "sh" | "bash" => Some(&*BASH),
        "py" => Some(&*PYTHON),
        "toml" => Some(&*TOML),
        "yml" | "yaml" => Some(&*YAML),
        _ => None,
    }
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
    fn detects_project_formats_by_extension_and_name() {
        for path in [
            "x.rs",
            "x.toml",
            "x.go",
            "x.sh",
            "x.bash",
            "x.py",
            "x.yml",
            "x.yaml",
            "x.json",
            "x.jsonc",
            "Makefile",
            "Dockerfile",
            "go.mod",
        ] {
            assert!(language_for(Path::new(path)).is_some(), "{path}");
        }
        assert!(language_for(Path::new("x.md")).is_none());
        assert!(language_for(Path::new("x.unknown")).is_none());
    }

    #[test]
    fn project_formats_produce_expected_classes() {
        let cases = [
            ("x.toml", "key = \"value\"\n", SyntaxClass::Property),
            ("x.go", "func main() {}\n", SyntaxClass::Keyword),
            ("x.sh", "if true; then echo hi; fi\n", SyntaxClass::Keyword),
            ("x.py", "def f():\n    return 1\n", SyntaxClass::Keyword),
            ("x.yaml", "key: true\n", SyntaxClass::Property),
            ("Makefile", "all:\n\t@echo hi\n", SyntaxClass::Constant),
            ("Dockerfile", "FROM rust:1\n", SyntaxClass::Keyword),
            ("go.mod", "module example.com/x\n", SyntaxClass::Keyword),
            ("x.jsonc", "{\"key\": true}\n// c\n", SyntaxClass::Comment),
        ];
        for (path, source, class) in cases {
            let highlights =
                parse_source(source, Path::new(path)).unwrap_or_else(|| panic!("parse {path}"));
            assert!(
                highlights.iter().any(|highlight| highlight.class == class),
                "{path} did not produce {class:?}: {highlights:?}"
            );
        }
    }

    #[test]
    fn state_does_not_parse_inline_before_debounce() {
        let mut state = SyntaxState::default();
        let document = text("fn main() {}");
        state.note_activity();
        let changed = state.poll(&document, 1, Some(Path::new("x.rs")));
        assert!(changed);
        assert!(state.highlights().is_empty());
    }
}
