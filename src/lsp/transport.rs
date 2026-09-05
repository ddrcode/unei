//! JSON-RPC over stdio with LSP Content-Length framing. A reader thread
//! parses server messages into a channel; writes go straight to the child's
//! stdin from the main thread.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin};
use std::sync::mpsc::{Receiver, Sender, channel};

use serde_json::Value;

pub struct Transport {
    pub child: Child,
    stdin: ChildStdin,
    pub incoming: Receiver<Value>,
}

impl Transport {
    /// Takes over a spawned child's stdio and starts the reader thread.
    pub fn new(mut child: Child) -> std::io::Result<Self> {
        let stdin = child.stdin.take().expect("child stdin piped");
        let stdout = child.stdout.take().expect("child stdout piped");
        let (tx, incoming) = channel();
        std::thread::spawn(move || read_loop(stdout, tx));
        Ok(Self {
            child,
            stdin,
            incoming,
        })
    }

    pub fn send(&mut self, msg: &Value) -> std::io::Result<()> {
        let body = serde_json::to_vec(msg).expect("serializable message");
        write!(self.stdin, "Content-Length: {}\r\n\r\n", body.len())?;
        self.stdin.write_all(&body)?;
        self.stdin.flush()
    }

    pub fn shutdown(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn read_loop(stdout: impl Read, tx: Sender<Value>) {
    let mut reader = BufReader::new(stdout);
    loop {
        // headers
        let mut content_length: Option<usize> = None;
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => return, // EOF: server exited
                Ok(_) => {}
                Err(_) => return,
            }
            let line = line.trim_end();
            if line.is_empty() {
                break;
            }
            if let Some(rest) = line
                .strip_prefix("Content-Length:")
                .or_else(|| line.strip_prefix("content-length:"))
            {
                content_length = rest.trim().parse().ok();
            }
        }
        let Some(len) = content_length else { return };
        let mut body = vec![0u8; len];
        if reader.read_exact(&mut body).is_err() {
            return;
        }
        match serde_json::from_slice::<Value>(&body) {
            Ok(msg) => {
                if tx.send(msg).is_err() {
                    return; // editor side dropped the channel
                }
            }
            Err(_) => continue, // skip malformed message
        }
    }
}
