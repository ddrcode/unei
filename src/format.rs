//! External formatting on save — treefmt as the single integration point
//! (docs/decisions.md). The editor never formats text itself: after a
//! successful write, if a treefmt config governs the file, treefmt runs on
//! it and the buffer reloads when the file changed. No config → no
//! formatting, silently.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::config::OPTIONS;

pub enum Outcome {
    /// No treefmt config governs this file (or formatting is disabled).
    NoConfig,
    /// treefmt ran; the file is unchanged.
    Unchanged,
    /// treefmt rewrote the file; `text` is the new content.
    Reformatted { text: String },
    /// treefmt failed or timed out (the written file may be untouched).
    Failed(String),
}

/// Finds the governing config by walking up from the file's directory.
fn find_config(file: &Path) -> Option<(PathBuf, PathBuf)> {
    let mut dir = file.parent()?;
    loop {
        for name in ["treefmt.toml", ".treefmt.toml"] {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some((candidate, dir.to_path_buf()));
            }
        }
        dir = dir.parent()?;
    }
}

/// Formats a just-written file. Blocking, bounded by
/// `OPTIONS.format_timeout_ms` (the process is killed on overrun).
pub fn format_file(file: &Path) -> Outcome {
    if !OPTIONS.format_on_save {
        return Outcome::NoConfig;
    }
    let Some((config, root)) = find_config(file) else {
        return Outcome::NoConfig;
    };
    let before = match std::fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) => return Outcome::Failed(format!("cannot reread file: {e}")),
    };

    let child = Command::new("treefmt")
        .arg("--config-file")
        .arg(&config)
        .arg("--tree-root")
        .arg(&root)
        .arg(file)
        .current_dir(&root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return Outcome::Failed(format!("treefmt not runnable: {e}")),
    };

    let deadline = Instant::now() + Duration::from_millis(OPTIONS.format_timeout_ms);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Outcome::Failed(format!(
                        "treefmt timed out after {}ms",
                        OPTIONS.format_timeout_ms
                    ));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return Outcome::Failed(format!("treefmt wait failed: {e}")),
        }
    };

    if !status.success() {
        let stderr = child
            .stderr
            .take()
            .and_then(|mut s| {
                use std::io::Read;
                let mut buf = String::new();
                s.read_to_string(&mut buf).ok().map(|_| buf)
            })
            .unwrap_or_default();
        let tail: String = stderr
            .lines()
            .last()
            .unwrap_or("")
            .chars()
            .take(120)
            .collect();
        return Outcome::Failed(format!("treefmt exited with {status}: {tail}"));
    }

    match std::fs::read_to_string(file) {
        Ok(after) if after != before => Outcome::Reformatted { text: after },
        Ok(_) => Outcome::Unchanged,
        Err(e) => Outcome::Failed(format!("cannot reread file: {e}")),
    }
}
