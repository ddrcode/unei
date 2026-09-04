//! The grammar registry — THE per-language configuration (ticket #17).
//!
//! Enabling or disabling a grammar is an edit here plus a rebuild: add or
//! remove the entry in `LANGUAGES` (and the dependency in Cargo.toml).
//! A file whose language isn't registered renders in the default color —
//! no fallback highlighting, per the rules.

use std::path::Path;
use std::sync::OnceLock;

use tree_sitter::{Language, Query};

use super::theme;

struct LangSpec {
    /// Registry name; also the key injections resolve (markdown fences name
    /// languages like `rust`, and the block grammar injects `markdown_inline`).
    name: &'static str,
    /// Extensions (lowercase) that select this language.
    extensions: &'static [&'static str],
    /// Exact file names that select this language.
    filenames: &'static [&'static str],
    language: fn() -> Language,
    highlights: &'static str,
    injections: &'static str,
    cell: OnceLock<LangConfig>,
}

/// A compiled language: grammar, queries, and the query-capture → theme
/// mapping (longest dotted-prefix match against `theme::CAPTURES`).
pub struct LangConfig {
    pub language: Language,
    pub highlights: Query,
    pub injections: Option<Query>,
    /// Highlight-query capture index → theme capture index.
    pub theme_map: Vec<Option<u16>>,
}

static LANGUAGES: [LangSpec; 4] = [
    LangSpec {
        name: "rust",
        extensions: &["rs"],
        filenames: &[],
        language: || tree_sitter_rust::LANGUAGE.into(),
        highlights: tree_sitter_rust::HIGHLIGHTS_QUERY,
        injections: tree_sitter_rust::INJECTIONS_QUERY,
        cell: OnceLock::new(),
    },
    LangSpec {
        name: "markdown",
        extensions: &["md", "markdown"],
        filenames: &[],
        language: || tree_sitter_md::LANGUAGE.into(),
        highlights: tree_sitter_md::HIGHLIGHT_QUERY_BLOCK,
        injections: tree_sitter_md::INJECTION_QUERY_BLOCK,
        cell: OnceLock::new(),
    },
    LangSpec {
        name: "markdown_inline",
        extensions: &[], // injection-only, never picked by file
        filenames: &[],
        language: || tree_sitter_md::INLINE_LANGUAGE.into(),
        highlights: tree_sitter_md::HIGHLIGHT_QUERY_INLINE,
        injections: tree_sitter_md::INJECTION_QUERY_INLINE,
        cell: OnceLock::new(),
    },
    LangSpec {
        name: "toml",
        extensions: &["toml"],
        filenames: &["Cargo.lock"],
        language: || tree_sitter_toml_ng::LANGUAGE.into(),
        highlights: tree_sitter_toml_ng::HIGHLIGHTS_QUERY,
        injections: "",
        cell: OnceLock::new(),
    },
];

/// Longest dotted-prefix match: `keyword.control` falls back to `keyword`.
fn theme_index(capture: &str) -> Option<u16> {
    let mut name = capture;
    loop {
        if let Some(i) = theme::CAPTURES.iter().position(|c| *c == name) {
            return Some(i as u16);
        }
        name = name.rsplit_once('.')?.0;
    }
}

fn build(spec: &LangSpec) -> LangConfig {
    let language = (spec.language)();
    let highlights = Query::new(&language, spec.highlights).expect("highlight query");
    let injections = (!spec.injections.is_empty())
        .then(|| Query::new(&language, spec.injections).expect("injection query"));
    let theme_map = highlights
        .capture_names()
        .iter()
        .map(|name| theme_index(name))
        .collect();
    LangConfig {
        language,
        highlights,
        injections,
        theme_map,
    }
}

/// Language name for a file, by exact name first, then extension.
pub fn detect(path: Option<&Path>) -> Option<&'static str> {
    let path = path?;
    let file_name = path.file_name()?.to_str()?;
    for spec in &LANGUAGES {
        if spec.filenames.contains(&file_name) {
            return Some(spec.name);
        }
    }
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    LANGUAGES
        .iter()
        .find(|spec| spec.extensions.contains(&ext.as_str()))
        .map(|spec| spec.name)
}

/// The lazily compiled configuration for a registered language; also the
/// resolver used for injections.
pub fn config_for(name: &str) -> Option<&'static LangConfig> {
    let spec = LANGUAGES.iter().find(|spec| spec.name == name)?;
    Some(spec.cell.get_or_init(|| build(spec)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection_rules() {
        let p = |s: &str| detect(Some(Path::new(s)));
        assert_eq!(p("src/main.rs"), Some("rust"));
        assert_eq!(p("README.md"), Some("markdown"));
        assert_eq!(p("notes.MARKDOWN"), Some("markdown"));
        assert_eq!(p("Cargo.toml"), Some("toml"));
        assert_eq!(p("Cargo.lock"), Some("toml"));
        assert_eq!(p("script.sh"), None);
        assert_eq!(p("Makefile"), None);
        assert_eq!(detect(None), None);
    }

    #[test]
    fn configs_build() {
        for name in ["rust", "markdown", "markdown_inline", "toml"] {
            assert!(config_for(name).is_some(), "{name} must build");
        }
        assert!(config_for("cobol").is_none());
    }

    #[test]
    fn prefix_fallback() {
        assert_eq!(theme_index("keyword"), theme_index("keyword.control"));
        assert!(theme_index("totally.unknown").is_none());
        assert!(theme_index("none").is_some());
    }
}
