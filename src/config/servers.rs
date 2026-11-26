//! The language servers (ticket #118), compiled in like everything else:
//! which server serves a language, how it is started, where its project
//! root lies, and what it is told at `initialize`. The client in `lsp/` is
//! generic LSP; everything that differs between servers lives here, so a
//! new language means one more entry in `SERVERS`.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::languages;

pub struct ServerSpec {
    /// The executable on PATH — also the server's name in messages.
    pub command: &'static str,
    pub args: &'static [&'static str],
    /// Short tag for the statusline (`ra:indexing`).
    pub tag: &'static str,
    /// Grammar-registry names (`languages::detect`) this server serves.
    pub languages: &'static [&'static str],
    /// `languageId` sent with every document.
    pub language_id: &'static str,
    /// The project root for a file, given the editor's working folder as a
    /// fallback; None means "no project here, don't start a server".
    pub root: fn(file: &Path, editor_root: &Path) -> Option<PathBuf>,
    /// `initializationOptions` for `initialize`.
    pub init_options: fn() -> Value,
    /// Serves rust-analyzer's `rust-analyzer/expandMacro` extension (the
    /// `Space r m` chord). A server never answers a method it doesn't know,
    /// so the editor must not ask.
    pub expands_macros: bool,
}

pub static SERVERS: [ServerSpec; 2] = [
    ServerSpec {
        command: "rust-analyzer",
        args: &[],
        tag: "ra",
        languages: &["rust"],
        language_id: "rust",
        root: cargo_root,
        init_options: rust_analyzer_options,
        expands_macros: true,
    },
    ServerSpec {
        command: "slang-server",
        args: &[],
        tag: "slang",
        languages: &["systemverilog"],
        language_id: "systemverilog",
        root: slang_root,
        init_options: || json!({}),
        expands_macros: false,
    },
];

/// The server serving a grammar-registry language, if any.
pub fn server_for(language: &str) -> Option<&'static ServerSpec> {
    SERVERS.iter().find(|s| s.languages.contains(&language))
}

/// The server serving a file, by the language its path detects.
pub fn server_for_path(path: Option<&Path>) -> Option<&'static ServerSpec> {
    server_for(languages::detect(path)?)
}

/// The cargo project root governing `file`, if any: the *outermost*
/// `Cargo.toml`, so a workspace member is served by the workspace.
fn cargo_root(file: &Path, _editor_root: &Path) -> Option<PathBuf> {
    let mut dir = file.parent()?;
    let mut found = None;
    loop {
        if dir.join("Cargo.toml").is_file() {
            found = Some(dir.to_path_buf()); // keep climbing: workspace root wins
        }
        match dir.parent() {
            Some(p) => dir = p,
            None => return found,
        }
    }
}

/// slang-server crawls its workspace folder for sources, so the root is
/// the project: the nearest ancestor holding `.slang/` (its config) or
/// `.git`. Without either, the editor's working folder when the file lies
/// inside it, else the file's own folder — never a folder the user did not
/// point at (crawling `~` is not a thing to do by accident).
fn slang_root(file: &Path, editor_root: &Path) -> Option<PathBuf> {
    let parent = file.parent()?;
    let marked = parent
        .ancestors()
        .find(|d| d.join(".slang").is_dir() || d.join(".git").exists());
    Some(match marked {
        Some(d) => d.to_path_buf(),
        None if file.starts_with(editor_root) => editor_root.to_path_buf(),
        None => parent.to_path_buf(),
    })
}

fn rust_analyzer_options() -> Value {
    json!({
        "checkOnSave": true,
        // direnv's `.direnv/flake-profile-*` symlinks into the nix store's
        // shell environment; rust-analyzer's scanner follows it and never
        // finishes loading the workspace — no hover, no hints, no
        // diagnostics, silently (#93)
        "files": { "excludeDirs": [".direnv"] },
        // every hint kind the compiler's-eye view (#93) prints; nothing is
        // shown inline, so the breadth costs nothing
        "inlayHints": {
            "maxLength": null,
            "typeHints": { "enable": true, "hideClosureInitialization": false, "hideNamedConstructor": false },
            "parameterHints": { "enable": true },
            "chainingHints": { "enable": true },
            "closingBraceHints": { "enable": true, "minLines": 25 },
            "closureReturnTypeHints": { "enable": "always" },
            "lifetimeElisionHints": { "enable": "always", "useParameterNames": true },
            // not expressionAdjustmentHints: every auto-ref/deref as `&**&x`
            // is the MIR's eye, not the compiler's, and "reborrow" hides
            // almost none of them (tried, #93)
            "bindingModeHints": { "enable": true }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox(name: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("unei-servers-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    #[test]
    fn a_language_names_its_server() {
        assert_eq!(server_for("rust").map(|s| s.command), Some("rust-analyzer"));
        assert_eq!(
            server_for("systemverilog").map(|s| s.command),
            Some("slang-server")
        );
        assert_eq!(server_for("lpf").map(|s| s.command), None);
        assert_eq!(server_for("markdown").map(|s| s.command), None);
        let by_path = |s: &str| server_for_path(Some(Path::new(s))).map(|s| s.command);
        assert_eq!(by_path("src/main.rs"), Some("rust-analyzer"));
        assert_eq!(by_path("hdl/blink.sv"), Some("slang-server"));
        assert_eq!(by_path("rtl/top.v"), Some("slang-server"));
        assert_eq!(by_path("inc/defs.svh"), Some("slang-server"));
        assert_eq!(by_path("ulx3s.lpf"), None);
        assert!(server_for_path(None).is_none());
    }

    #[test]
    fn every_language_has_one_server_and_a_tag() {
        for spec in &SERVERS {
            for lang in spec.languages {
                assert!(
                    SERVERS
                        .iter()
                        .filter(|s| s.languages.contains(lang))
                        .count()
                        == 1,
                    "{lang} is served twice"
                );
            }
            assert!(!spec.tag.is_empty() && spec.tag.len() <= 5);
        }
    }

    #[test]
    fn cargo_root_prefers_the_outermost_manifest() {
        let base = sandbox("cargo");
        std::fs::create_dir_all(base.join("member/src")).unwrap();
        std::fs::write(base.join("Cargo.toml"), "[workspace]\n").unwrap();
        std::fs::write(base.join("member/Cargo.toml"), "[package]\n").unwrap();
        let file = base.join("member/src/lib.rs");
        assert_eq!(cargo_root(&file, &base), Some(base.clone()));
        assert_eq!(
            cargo_root(Path::new("/tmp/nowhere/x.rs"), Path::new("/tmp")),
            None,
            "no manifest, no server"
        );
    }

    #[test]
    fn slang_root_is_the_project_then_the_working_folder_then_the_file_folder() {
        let base = sandbox("slang");
        // a repo: `.git` marks the root, however deep the file
        std::fs::create_dir_all(base.join("repo/.git")).unwrap();
        std::fs::create_dir_all(base.join("repo/rtl/core")).unwrap();
        let file = base.join("repo/rtl/core/alu.sv");
        assert_eq!(slang_root(&file, &base), Some(base.join("repo")));
        // a `.slang/` folder nearer than `.git` wins — it is the server's own config
        std::fs::create_dir_all(base.join("repo/rtl/.slang")).unwrap();
        assert_eq!(slang_root(&file, &base), Some(base.join("repo/rtl")));
        // no marker, file inside the working folder: the working folder
        std::fs::create_dir_all(base.join("loose/hdl")).unwrap();
        let loose = base.join("loose/hdl/blink.sv");
        assert_eq!(
            slang_root(&loose, &base.join("loose")),
            Some(base.join("loose"))
        );
        // no marker, file outside the working folder: its own folder, not
        // some unrelated tree
        assert_eq!(
            slang_root(&loose, &base.join("elsewhere")),
            Some(base.join("loose/hdl"))
        );
    }
}
