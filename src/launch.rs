//! CLI argument resolution: working folder + buffers (ticket #3 rules).

use std::path::PathBuf;

pub struct Launch {
    /// The working folder (picker root).
    pub root: PathBuf,
    /// Files to open as buffers, in order.
    pub files: Vec<PathBuf>,
    /// Start with the file picker open (launched onto a directory).
    pub open_picker: bool,
}

/// - no args: root = cwd, one empty buffer
/// - first arg is a dir: it becomes the root and the picker opens; other dir
///   args are ignored, file args still become buffers
/// - first arg is a file: its parent becomes the root; every file arg becomes
///   a buffer, dir args are ignored
pub fn resolve(args: &[String], cwd: PathBuf) -> Launch {
    let Some(first) = args.first() else {
        return Launch {
            root: cwd,
            files: Vec::new(),
            open_picker: false,
        };
    };
    let first_path = PathBuf::from(first);
    let first_is_dir = first_path.is_dir();

    let files: Vec<PathBuf> = args
        .iter()
        .map(PathBuf::from)
        .filter(|p| !p.is_dir())
        .collect();

    let root = if first_is_dir {
        first_path
    } else {
        first_path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(PathBuf::from)
            .unwrap_or(cwd)
    };

    Launch {
        root,
        open_picker: first_is_dir && files.is_empty(),
        files,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tailored-launch-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("a.txt"), "a").unwrap();
        fs::write(dir.join("sub/b.txt"), "b").unwrap();
        dir
    }

    fn s(p: &std::path::Path) -> String {
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn no_args_uses_cwd() {
        let l = resolve(&[], PathBuf::from("/work"));
        assert_eq!(l.root, PathBuf::from("/work"));
        assert!(l.files.is_empty() && !l.open_picker);
    }

    #[test]
    fn dir_arg_becomes_root_and_opens_picker() {
        let d = tmp("dir");
        let l = resolve(&[s(&d)], PathBuf::from("/work"));
        assert_eq!(l.root, d);
        assert!(l.files.is_empty());
        assert!(l.open_picker);
    }

    #[test]
    fn file_arg_roots_at_parent() {
        let d = tmp("file");
        let f = d.join("a.txt");
        let l = resolve(&[s(&f)], PathBuf::from("/work"));
        assert_eq!(l.root, d);
        assert_eq!(l.files, vec![f]);
        assert!(!l.open_picker);
    }

    #[test]
    fn bare_filename_roots_at_cwd() {
        let l = resolve(&["notes.md".into()], PathBuf::from("/work"));
        assert_eq!(l.root, PathBuf::from("/work"));
        assert_eq!(l.files, vec![PathBuf::from("notes.md")]);
    }

    #[test]
    fn dir_first_ignores_other_dirs_keeps_files() {
        let d = tmp("mixed");
        let l = resolve(
            &[s(&d), s(&d.join("sub")), s(&d.join("a.txt"))],
            PathBuf::from("/work"),
        );
        assert_eq!(l.root, d);
        assert_eq!(l.files, vec![d.join("a.txt")]);
        assert!(!l.open_picker); // a file was given, no picker
    }

    #[test]
    fn file_first_ignores_dirs() {
        let d = tmp("filefirst");
        let l = resolve(
            &[
                s(&d.join("a.txt")),
                s(&d.join("sub")),
                s(&d.join("sub/b.txt")),
            ],
            PathBuf::from("/work"),
        );
        assert_eq!(l.root, d);
        assert_eq!(l.files, vec![d.join("a.txt"), d.join("sub/b.txt")]);
    }

    #[test]
    fn nonexistent_path_is_a_new_file() {
        let l = resolve(&["brand/new.md".into()], PathBuf::from("/work"));
        assert_eq!(l.root, PathBuf::from("brand"));
        assert_eq!(l.files, vec![PathBuf::from("brand/new.md")]);
    }
}
