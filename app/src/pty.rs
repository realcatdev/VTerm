use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;

use anyhow::{Context, Result};
use portable_pty::{CommandBuilder, PtyPair, PtySize, native_pty_system};

pub struct PtySession {
    _pair: PtyPair,
    _child: Box<dyn portable_pty::Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    receiver: Receiver<String>,
    shell_label: String,
    cwd_label: String,
}

impl PtySession {
    pub fn new(cols: u16, rows: u16) -> Result<Self> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("failed to allocate PTY")?;

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
        let mut cmd = CommandBuilder::new_default_prog();
        let cwd: PathBuf = std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/"));
        let cwd_label = cwd
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("~")
            .to_string();
        cmd.cwd(&cwd);
        cmd.env("SHELL", &shell);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", "VTerm");
        cmd.env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"));
        cmd.env("PROMPT_EOL_MARK", "");
        if let Ok(lang) = std::env::var("LANG") {
            cmd.env("LANG", lang);
        }
        if let Ok(lc_ctype) = std::env::var("LC_CTYPE") {
            cmd.env("LC_CTYPE", lc_ctype);
        }
        let child = pair
            .slave
            .spawn_command(cmd)
            .context("failed to spawn shell in PTY")?;

        let mut reader = pair
            .master
            .try_clone_reader()
            .context("failed to clone PTY reader")?;
        let writer = pair
            .master
            .take_writer()
            .context("failed to take PTY writer")?;

        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut buf = [0u8; 8192];
            while let Ok(size) = reader.read(&mut buf) {
                if size == 0 {
                    break;
                }
                let chunk = String::from_utf8_lossy(&buf[..size]).to_string();
                if tx.send(chunk).is_err() {
                    break;
                }
            }
        });

        Ok(Self {
            _pair: pair,
            _child: child,
            writer,
            receiver: rx,
            shell_label: shell
                .rsplit('/')
                .next()
                .filter(|name| !name.is_empty())
                .unwrap_or("shell")
                .to_string(),
            cwd_label,
        })
    }

    pub fn shell_label(&self) -> &str {
        &self.shell_label
    }

    pub fn cwd_label(&self) -> &str {
        &self.cwd_label
    }

    pub fn try_read(&self) -> Vec<String> {
        let mut chunks = Vec::new();
        while let Ok(chunk) = self.receiver.try_recv() {
            chunks.push(chunk);
        }
        chunks
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) -> Result<()> {
        self.writer
            .write_all(bytes)
            .context("failed to write PTY bytes")?;
        self.writer.flush().context("failed to flush PTY writer")
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self._pair
            .master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("failed to resize PTY")
    }
}
