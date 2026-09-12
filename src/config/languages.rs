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

// The bespoke 6502/ACME grammar (#18), vendored under grammars/asm6502 and
// compiled by build.rs. Selected via the asm modeline, not by extension —
// `.s` is dialect-ambiguous — so it carries no extensions in the registry.
unsafe extern "C" {
    fn tree_sitter_asm6502() -> *const ();
}
pub const ASM6502_LANGUAGE: tree_sitter_language::LanguageFn =
    unsafe { tree_sitter_language::LanguageFn::from_raw(tree_sitter_asm6502) };

unsafe extern "C" {
    fn tree_sitter_just() -> *const ();
}
/// The justfile grammar (#108), vendored under `grammars/just` and compiled
/// by build.rs — the published crate pins a tree-sitter unei can't link.
pub const JUST_LANGUAGE: tree_sitter_language::LanguageFn =
    unsafe { tree_sitter_language::LanguageFn::from_raw(tree_sitter_just) };
const JUST_HIGHLIGHTS: &str = include_str!("../../grammars/just/queries/highlights.scm");
const JUST_INJECTIONS: &str = include_str!("../../grammars/just/queries/injections.scm");
/// Symbols of a justfile: recipes, variables, aliases, modules.
const JUST_SYMBOLS: &str = r#"
(recipe_header name: (identifier) @recipe)
(assignment left: (identifier) @var)
(alias left: (identifier) @alias)
(module name: (identifier) @mod)
"#;

struct LangSpec {
    /// Registry name; also the key injections resolve (markdown fences name
    /// languages like `rust`, and the block grammar injects `markdown_inline`).
    name: &'static str,
    /// Extensions (lowercase) that select this language.
    extensions: &'static [&'static str],
    /// Exact file names that select this language.
    filenames: &'static [&'static str],
    /// Alternative names injections may use (markdown fences say `js`).
    aliases: &'static [&'static str],
    language: fn() -> Language,
    highlights: &'static str,
    /// Appended to the bundled query; later patterns win ties, so this is
    /// the place for small local overrides without vendoring whole queries.
    highlights_extra: &'static str,
    injections: &'static str,
    /// Definition query for the symbol picker (#62): each pattern captures a
    /// definition's name node under a kind-named capture (`@fn`, `@struct`,
    /// …). Empty when the grammar has no symbol support yet.
    symbols: &'static str,
    /// Syntax text objects (#78): patterns capture whole nodes as
    /// `@function.around` / `@class.around` and their bodies as
    /// `@function.inner` / `@class.inner`, for `af`/`nf`, `ac`/`nc`.
    textobjects: &'static str,
    cell: OnceLock<LangConfig>,
}

/// Conservative Rust definitions for the symbol picker — the 90% that is
/// function/method/type navigation; esoteric constructs are intentionally
/// omitted (#62).
const RUST_SYMBOLS: &str = r#"
(function_item name: (identifier) @fn)
(struct_item name: (type_identifier) @struct)
(union_item name: (type_identifier) @struct)
(enum_item name: (type_identifier) @enum)
(trait_item name: (type_identifier) @trait)
(mod_item name: (identifier) @mod)
(const_item name: (identifier) @const)
(static_item name: (identifier) @static)
(type_item name: (type_identifier) @type)
(macro_definition name: (identifier) @macro)
(impl_item type: (type_identifier) @impl)
"#;

/// Rust syntax text objects (#78): the whole item is `@*.around`, its body is
/// `@*.inner`. Functions and the type-defining items (impl/struct/enum/union/
/// trait/mod); `body: (_)` matches whatever body the item has (a unit struct
/// has none, so only its `.around` matches — still selectable).
const RUST_TEXTOBJECTS: &str = r#"
(function_item) @function.around
(function_item body: (_) @function.inner)
(impl_item) @class.around
(impl_item body: (_) @class.inner)
(struct_item) @class.around
(struct_item body: (_) @class.inner)
(enum_item) @class.around
(enum_item body: (_) @class.inner)
(union_item) @class.around
(union_item body: (_) @class.inner)
(trait_item) @class.around
(trait_item body: (_) @class.inner)
(mod_item) @class.around
(mod_item body: (_) @class.inner)
"#;

/// A compiled language: grammar, queries, and the query-capture → theme
/// mapping (longest dotted-prefix match against `theme::CAPTURES`).
pub struct LangConfig {
    pub language: Language,
    pub highlights: Query,
    pub injections: Option<Query>,
    /// Compiled symbol-definition query (#62), if the grammar has one.
    pub symbols: Option<Query>,
    /// Compiled syntax-text-object query (#78), if the grammar has one.
    pub textobjects: Option<Query>,
    /// Highlight-query capture index → theme capture index.
    pub theme_map: Vec<Option<u16>>,
    /// First pattern index belonging to `highlights_extra`. Bundled patterns
    /// (below this) resolve identical-range ties first-wins, the official
    /// tree-sitter convention their queries are written for; extra patterns
    /// (this and above) always override.
    pub extra_pattern_start: usize,
}

static LANGUAGES: [LangSpec; 14] = [
    LangSpec {
        name: "asm6502",
        aliases: &["6502", "acme"],
        extensions: &[],
        filenames: &[],
        language: || ASM6502_LANGUAGE.into(),
        highlights_extra: "",
        highlights: include_str!("../../grammars/asm6502/queries/highlights.scm"),
        injections: "",
        symbols: "",
        textobjects: "",
        cell: OnceLock::new(),
    },
    LangSpec {
        name: "rust",
        aliases: &["rs"],
        extensions: &["rs"],
        filenames: &[],
        language: || tree_sitter_rust::LANGUAGE.into(),
        highlights_extra: "",
        highlights: tree_sitter_rust::HIGHLIGHTS_QUERY,
        injections: tree_sitter_rust::INJECTIONS_QUERY,
        symbols: RUST_SYMBOLS,
        textobjects: RUST_TEXTOBJECTS,
        cell: OnceLock::new(),
    },
    LangSpec {
        name: "markdown",
        aliases: &["md"],
        extensions: &["md", "markdown"],
        filenames: &[],
        language: || tree_sitter_md::LANGUAGE.into(),
        highlights_extra: "",
        highlights: tree_sitter_md::HIGHLIGHT_QUERY_BLOCK,
        injections: tree_sitter_md::INJECTION_QUERY_BLOCK,
        symbols: "",
        textobjects: "",
        cell: OnceLock::new(),
    },
    LangSpec {
        name: "markdown_inline",
        aliases: &[],
        extensions: &[], // injection-only, never picked by file
        filenames: &[],
        language: || tree_sitter_md::INLINE_LANGUAGE.into(),
        highlights_extra: "",
        highlights: tree_sitter_md::HIGHLIGHT_QUERY_INLINE,
        injections: tree_sitter_md::INJECTION_QUERY_INLINE,
        symbols: "",
        textobjects: "",
        cell: OnceLock::new(),
    },
    LangSpec {
        name: "toml",
        aliases: &[],
        extensions: &["toml"],
        filenames: &["Cargo.lock"],
        language: || tree_sitter_toml_ng::LANGUAGE.into(),
        // keys render property-pale (nvim look, per the author — "toml is
        // very yellow in nature"); [table] headers stay @type
        highlights_extra: "(pair (bare_key) @property) (pair (dotted_key (bare_key) @property))",
        highlights: tree_sitter_toml_ng::HIGHLIGHTS_QUERY,
        injections: "",
        symbols: "",
        textobjects: "",
        cell: OnceLock::new(),
    },
    LangSpec {
        name: "yaml",
        aliases: &["yml"],
        extensions: &["yml", "yaml"],
        filenames: &[],
        language: || tree_sitter_yaml::LANGUAGE.into(),
        highlights_extra: "(block_mapping_pair key: (flow_node (plain_scalar (string_scalar) @property))) (flow_pair key: (flow_node (plain_scalar (string_scalar) @property)))",
        highlights: tree_sitter_yaml::HIGHLIGHTS_QUERY,
        injections: "",
        symbols: "",
        textobjects: "",
        cell: OnceLock::new(),
    },
    LangSpec {
        name: "json",
        aliases: &[],
        extensions: &["json"],
        filenames: &[],
        language: || tree_sitter_json::LANGUAGE.into(),
        highlights_extra: "",
        highlights: tree_sitter_json::HIGHLIGHTS_QUERY,
        injections: "",
        symbols: "",
        textobjects: "",
        cell: OnceLock::new(),
    },
    LangSpec {
        name: "javascript",
        aliases: &["js"],
        extensions: &["js", "mjs", "cjs"],
        filenames: &[],
        language: || tree_sitter_javascript::LANGUAGE.into(),
        highlights_extra: "",
        highlights: tree_sitter_javascript::HIGHLIGHT_QUERY,
        injections: tree_sitter_javascript::INJECTIONS_QUERY,
        symbols: "",
        textobjects: "",
        cell: OnceLock::new(),
    },
    LangSpec {
        name: "html",
        aliases: &[],
        extensions: &["html", "htm"],
        filenames: &[],
        language: || tree_sitter_html::LANGUAGE.into(),
        highlights_extra: "",
        highlights: tree_sitter_html::HIGHLIGHTS_QUERY,
        injections: tree_sitter_html::INJECTIONS_QUERY,
        symbols: "",
        textobjects: "",
        cell: OnceLock::new(),
    },
    LangSpec {
        name: "bash",
        aliases: &["sh", "shell", "zsh"],
        extensions: &["sh", "bash", "zsh"],
        filenames: &[],
        language: || tree_sitter_bash::LANGUAGE.into(),
        highlights_extra: "",
        highlights: tree_sitter_bash::HIGHLIGHT_QUERY,
        injections: "",
        symbols: "",
        textobjects: "",
        cell: OnceLock::new(),
    },
    LangSpec {
        name: "nix",
        aliases: &[],
        extensions: &["nix"],
        filenames: &[],
        language: || tree_sitter_nix::LANGUAGE.into(),
        highlights_extra: "",
        highlights: tree_sitter_nix::HIGHLIGHTS_QUERY,
        injections: tree_sitter_nix::INJECTIONS_QUERY,
        symbols: "",
        textobjects: "",
        cell: OnceLock::new(),
    },
    LangSpec {
        name: "just",
        aliases: &["justfile"],
        extensions: &["just"],
        filenames: &["justfile", "Justfile", "JUSTFILE", ".justfile", ".Justfile"],
        language: || JUST_LANGUAGE.into(),
        highlights_extra: "",
        highlights: JUST_HIGHLIGHTS,
        injections: JUST_INJECTIONS,
        symbols: JUST_SYMBOLS,
        textobjects: "",
        cell: OnceLock::new(),
    },
    LangSpec {
        name: "python",
        aliases: &["py"],
        extensions: &["py", "pyi"],
        filenames: &[],
        language: || tree_sitter_python::LANGUAGE.into(),
        highlights_extra: "",
        highlights: tree_sitter_python::HIGHLIGHTS_QUERY,
        injections: "",
        symbols: "",
        textobjects: "",
        cell: OnceLock::new(),
    },
    LangSpec {
        name: "css",
        aliases: &[],
        extensions: &["css"],
        filenames: &[],
        language: || tree_sitter_css::LANGUAGE.into(),
        highlights_extra: "",
        highlights: tree_sitter_css::HIGHLIGHTS_QUERY,
        injections: "",
        symbols: "",
        textobjects: "",
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
    let highlights_src = format!("{}\n{}", spec.highlights, spec.highlights_extra);
    let highlights = Query::new(&language, &highlights_src).expect("highlight query");
    let extra_pattern_start = if spec.highlights_extra.is_empty() {
        highlights.pattern_count()
    } else {
        Query::new(&language, spec.highlights)
            .expect("bundled highlight query")
            .pattern_count()
    };
    let injections = (!spec.injections.is_empty())
        .then(|| Query::new(&language, spec.injections).expect("injection query"));
    let symbols = (!spec.symbols.is_empty())
        .then(|| Query::new(&language, spec.symbols).expect("symbols query"));
    let textobjects = (!spec.textobjects.is_empty())
        .then(|| Query::new(&language, spec.textobjects).expect("textobjects query"));
    let theme_map = highlights
        .capture_names()
        .iter()
        .map(|name| theme_index(name))
        .collect();
    LangConfig {
        language,
        highlights,
        injections,
        symbols,
        textobjects,
        theme_map,
        extra_pattern_start,
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
    let spec = LANGUAGES
        .iter()
        .find(|spec| spec.name == name || spec.aliases.contains(&name))?;
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
        assert_eq!(p("script.sh"), Some("bash"));
        assert_eq!(p("deploy.yml"), Some("yaml"));
        assert_eq!(p("flake.nix"), Some("nix"));
        // justfiles are found by name (any case) and by the .just extension
        assert_eq!(p("justfile"), Some("just"));
        assert_eq!(p("Justfile"), Some("just"));
        assert_eq!(p(".justfile"), Some("just"));
        assert_eq!(p("build.just"), Some("just"));
        assert_eq!(p("script.lua"), None);
        assert_eq!(p("Makefile"), None);
        assert_eq!(detect(None), None);
    }

    #[test]
    fn configs_build() {
        for name in [
            "rust",
            "markdown",
            "markdown_inline",
            "toml",
            "yaml",
            "json",
            "javascript",
            "html",
            "bash",
            "nix",
            "python",
            "css",
        ] {
            assert!(config_for(name).is_some(), "{name} must build");
        }
        assert!(config_for("cobol").is_none());
        assert!(config_for("js").is_some()); // alias
        assert!(config_for("py").is_some());
    }

    #[test]
    fn prefix_fallback() {
        assert_eq!(theme_index("keyword"), theme_index("keyword.control"));
        assert!(theme_index("totally.unknown").is_none());
        assert!(theme_index("none").is_some());
    }
}
