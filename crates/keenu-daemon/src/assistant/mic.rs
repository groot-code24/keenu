// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

//! The microphone, through PipeWire's `pw-record`: 16 kHz mono 16-bit chunks.

use std::process::Stdio;

use keenu_core::voice::CHUNK_MS;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

pub const RATE: u32 = 16_000;
const CHUNK_SAMPLES: usize = (RATE * CHUNK_MS / 1000) as usize;

/// A running `pw-record`, read chunk by chunk.
pub struct Mic {
    _child: Child,
    chunks: mpsc::Receiver<Vec<i16>>,
    reader: JoinHandle<()>,
}

impl Mic {
    /// Starts recording; the recorder is killed when this is dropped.
    pub fn start() -> std::io::Result<Self> {
        let mut child = Command::new("pw-record")
            // Tagged as Keenu's own, so the island does not take it for a call.
            .args(["--rate", "16000", "--channels", "1", "--format", "s16"])
            .args([
                "-P",
                "{ application.id = \"io.github.zickrian.isle\" }",
                "-",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let mut stdout = child.stdout.take().expect("piped stdout");
        // Read on a task of its own, so waiting for a chunk can be interrupted without losing audio.
        let (sender, chunks) = mpsc::channel(64);
        let reader = tokio::spawn(async move {
            let mut bytes = vec![0u8; CHUNK_SAMPLES * 2];
            while stdout.read_exact(&mut bytes).await.is_ok() {
                let chunk = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|&pair| i16::from_le_bytes(pair))
                    .collect();
                if sender.send(chunk).await.is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            _child: child,
            chunks,
            reader,
        })
    }

    /// The next chunk; `None` once the recorder stopped.
    pub async fn chunk(&mut self) -> Option<Vec<i16>> {
        self.chunks.recv().await
    }
}

impl Drop for Mic {
    /// Stops the reader; `kill_on_drop` takes care of the recorder.
    fn drop(&mut self) {
        self.reader.abort();
    }
}
