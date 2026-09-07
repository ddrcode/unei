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

/// Kills the formatter and everything it spawned. treefmt runs in its own
/// process group (see `format_file`), so on Unix the group as a whole is
/// signalled; killing only the direct child would let a still-running
/// formatter write the file after we've moved on.
fn kill_group(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        // SAFETY: plain syscall on a pid we own; the group was created by us
        unsafe {
            libc::killpg(child.id() as libc::pid_t, libc::SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
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
/// `OPTIONS.format_timeout_ms`: on overrun the whole formatter process
/// group is killed — treefmt *and* the formatters it spawned — so a
/// straggler can't rewrite the file after a later save (#82 §7).
pub fn format_file(file: &Path) -> Outcome {
    if !OPTIONS.format_on_save {
        return Outcome::NoConfig;
    }
    // absolute paths throughout: treefmt runs with its working directory set
    // to the tree root, so a relative launch path (`unei project/src/a.rs`)
    // would be resolved from the wrong place (#82 §10)
    let file: PathBuf = if file.is_absolute() {
        file.to_path_buf()
    } else {
        match std::env::current_dir() {
            Ok(cwd) => cwd.join(file),
            Err(e) => return Outcome::Failed(format!("no working directory: {e}")),
        }
    };
    let file = file.as_path();
    let Some((config, root)) = find_config(file) else {
        return Outcome::NoConfig;
    };
    let before = match std::fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) => return Outcome::Failed(format!("cannot reread file: {e}")),
    };

    let mut cmd = Command::new("treefmt");
    cmd.arg("--config-file")
        .arg(&config)
        .arg("--tree-root")
        .arg(&root)
        .arg(file)
        .current_dir(&root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0); // its own group, so a timeout reaches its children
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return Outcome::Failed(format!("treefmt not runnable: {e}")),
    };
    // drain stderr as it comes: a chatty formatter must not stall on a full
    // pipe, and the read must not add to the deadline after exit
    let stderr_pipe = child.stderr.take();
    let drain = std::thread::spawn(move || {
        use std::io::Read;
        let mut buf = String::new();
        if let Some(mut s) = stderr_pipe {
            let _ = s.read_to_string(&mut buf);
        }
        buf
    });

    let deadline = Instant::now() + Duration::from_millis(OPTIONS.format_timeout_ms);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() >= deadline {
                    kill_group(&mut child);
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
    let stderr = drain.join().unwrap_or_default();

    if !status.success() {
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
