// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 zickrian

//! The helpers the assistant starts (AI CLIs, speech recognition): each runs at a lower priority and with what it
//! starts tied to it, so the desktop stays smooth and nothing outlives them.

use std::ops::{Deref, DerefMut};

use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::{Child, Command};

/// How much they give way to the desktop (see nice(1)).
#[cfg(unix)]
const NICENESS: libc::c_int = 10;
/// No console window, and below normal priority.
#[cfg(windows)]
const CREATION_FLAGS: u32 = 0x0800_0000 | 0x0000_4000;
/// A line of an AI CLI's output longer than this (a whole file it read, say) is skipped, never kept.
const MAX_LINE: usize = 8 * 1024 * 1024;

/// A command for the CLI at `program`. npm's `.cmd` launcher is passed over for the script behind it, as `cmd.exe` cuts
/// a command line at 8 KB, and the assistant's instructions are longer.
pub fn command(program: &std::path::Path) -> Command {
    if let Some((node, script)) = npm_script(program) {
        let mut command = Command::new(node);
        command.arg(script);
        return command;
    }
    Command::new(program)
}

/// The Node and script behind npm's launcher at `program`, on a system that has them.
fn npm_script(program: &std::path::Path) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    if !program
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("cmd"))
    {
        return None;
    }
    let dir = program.parent()?;
    let script = script_in(&std::fs::read_to_string(program).ok()?)?;
    let node = dir.join("node.exe");
    let node = if node.is_file() {
        node
    } else {
        std::path::PathBuf::from("node")
    };
    Some((node, dir.join(script)))
}

/// What npm's launcher runs: `"%dp0%\\node_modules\\…\\cli.js"`, relative to the launcher's folder.
fn script_in(launcher: &str) -> Option<String> {
    let start = launcher.find("%dp0%\\")? + "%dp0%\\".len();
    launcher[start..]
        .split('"')
        .find_map(|rest| rest.ends_with(".js").then(|| rest.to_owned()))
}

/// A started helper; dropped, it goes and takes what it started along.
pub struct Group(Child);

/// Starts `command` at a lower priority, with no window. Its own folder leads its PATH.
#[cfg(windows)]
pub fn spawn(command: &mut Command) -> std::io::Result<Group> {
    let program = std::path::PathBuf::from(command.as_std().get_program());
    if let Some(path) = path_for(&program) {
        command.env("PATH", path);
    }
    let child = command
        .creation_flags(CREATION_FLAGS)
        .kill_on_drop(true)
        .spawn()?;
    Ok(Group(child))
}

/// Starts `command` in a process group of its own, at a lower priority. Its own folder leads its PATH, so
/// a CLI that starts with `#!/usr/bin/env node` finds the Node installed beside it (nvm, volta…).
#[cfg(unix)]
pub fn spawn(command: &mut Command) -> std::io::Result<Group> {
    let program = std::path::PathBuf::from(command.as_std().get_program());
    let path = path_for(&program);
    // It never gains privileges (no setuid sudo or pkexec), unless it runs from a snap, whose launcher
    // needs them. macOS has no such switch: there it relies on the AI program's own rules.
    #[cfg(target_os = "linux")]
    if !from_snap(&program, path.as_deref()) {
        // SAFETY: prctl is async-signal-safe and only reads its integer arguments.
        unsafe {
            command.pre_exec(|| {
                if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) == 0 {
                    Ok(())
                } else {
                    Err(std::io::Error::last_os_error())
                }
            });
        }
    }
    if let Some(path) = path {
        command.env("PATH", path);
    }
    let child = command.process_group(0).kill_on_drop(true).spawn()?;
    if let Some(pid) = child.id() {
        // SAFETY: setpriority only reads its plain integer arguments.
        unsafe { libc::setpriority(libc::PRIO_PGRP, pid, NICENESS) };
    }
    Ok(Group(child))
}

/// Whether `program`, or the Node it would run on, is a snap's.
#[cfg(target_os = "linux")]
fn from_snap(program: &std::path::Path, path: Option<&std::ffi::OsStr>) -> bool {
    let snap = |path: &std::path::Path| {
        std::fs::canonicalize(path)
            .is_ok_and(|real| real.starts_with("/snap") || real.ends_with("usr/bin/snap"))
    };
    let path = path.map_or_else(
        || std::env::var_os("PATH").unwrap_or_default(),
        std::ffi::OsStr::to_os_string,
    );
    let node = std::env::split_paths(&path)
        .map(|dir| dir.join("node"))
        .find(|node| node.is_file());
    snap(program)
        || program.starts_with("/snap")
        || node.is_some_and(|node| node.starts_with("/snap") || snap(&node))
}

/// The PATH for `program`: its own folder first, then keenud's.
pub fn path_for(program: &std::path::Path) -> Option<std::ffi::OsString> {
    let dir = program.parent().filter(|dir| !dir.as_os_str().is_empty())?;
    let path = std::env::var_os("PATH").unwrap_or_default();
    let dirs = std::iter::once(dir.to_path_buf()).chain(std::env::split_paths(&path));
    std::env::join_paths(dirs).ok()
}

impl Drop for Group {
    /// Kills the helper and everything it started.
    fn drop(&mut self) {
        // Its group, led by it: the node workers or servers an AI CLI starts go with it.
        #[cfg(unix)]
        if let Some(pid) = self.0.id().and_then(|pid| libc::pid_t::try_from(pid).ok()) {
            // SAFETY: killpg only reads its plain integer arguments.
            unsafe { libc::killpg(pid, libc::SIGKILL) };
        }
        // Its tree: Windows has no process groups to signal. By full path, never one found on the PATH.
        #[cfg(windows)]
        if let Some(pid) = self.0.id() {
            let system = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
            let taskkill = std::path::Path::new(&system).join("System32\\taskkill.exe");
            let _ = std::process::Command::new(taskkill)
                .args(["/T", "/F", "/PID", &pid.to_string()])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    }
}

impl Deref for Group {
    type Target = Child;

    /// The child process, for its pipes and exit status.
    fn deref(&self) -> &Child {
        &self.0
    }
}

impl DerefMut for Group {
    /// The child process, to take its pipes or wait on it.
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.0
    }
}

/// An AI CLI's JSON events, a line at a time, skipping those that cannot matter unparsed.
pub struct Events<R> {
    reader: BufReader<R>,
    line: Vec<u8>,
    limit: usize,
}

impl<R: AsyncRead + Unpin> Events<R> {
    /// Reads events from a CLI's stdout, skipping lines over 8 MiB.
    pub fn new(output: R) -> Self {
        Self::with_limit(output, MAX_LINE)
    }

    /// As `new`, with a smaller line limit for tests.
    fn with_limit(output: R, limit: usize) -> Self {
        Self {
            reader: BufReader::new(output),
            line: Vec::new(),
            limit,
        }
    }

    /// The next event holding one of `words` (each with its quotes, e.g. `"result"`: a key or a
    /// value), parsed; `None` once the output ends.
    pub async fn next(&mut self, words: &[&str]) -> std::io::Result<Option<serde_json::Value>> {
        loop {
            let Some(line) = self.next_line().await? else {
                return Ok(None);
            };
            if !words.iter().any(|word| line.contains(word)) {
                continue;
            }
            if let Ok(event) = serde_json::from_str(line) {
                return Ok(Some(event));
            }
        }
    }

    /// The next line, empty when it was too long to keep; `None` at the end.
    async fn next_line(&mut self) -> std::io::Result<Option<&str>> {
        self.line.clear();
        let mut skipped = false;
        let mut read_any = false;
        loop {
            let buffer = self.reader.fill_buf().await?;
            if buffer.is_empty() {
                if !read_any {
                    return Ok(None);
                }
                break;
            }
            read_any = true;
            let (part, ends) = match buffer.iter().position(|&byte| byte == b'\n') {
                Some(at) => (&buffer[..at], true),
                None => (buffer, false),
            };
            if skipped || self.line.len() + part.len() > self.limit {
                skipped = true;
                self.line.clear();
            } else {
                self.line.extend_from_slice(part);
            }
            let used = part.len() + usize::from(ends);
            self.reader.consume(used);
            if ends {
                break;
            }
        }
        if self.line.last() == Some(&b'\r') {
            self.line.pop();
        }
        Ok(Some(std::str::from_utf8(&self.line).unwrap_or_default()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_script_behind_npms_launcher() {
        let launcher = "@ECHO off\r\nSET dp0=%~dp0\r\nendLocal & goto #_undefined_# 2>NUL || title %COMSPEC% & \"%_prog%\"  \"%dp0%\\node_modules\\@anthropic-ai\\claude-code\\cli.js\" %*\r\n";
        assert_eq!(
            script_in(launcher).as_deref(),
            Some("node_modules\\@anthropic-ai\\claude-code\\cli.js")
        );
        assert_eq!(script_in("@ECHO off"), None);
    }

    #[tokio::test]
    async fn skips_lines_that_cannot_matter() {
        let output: &[u8] = b"{\"type\":\"user\",\"content\":\"x\"}\nnot json \"result\"\n\
            {\"type\":\"result\",\"n\":1}\r\n{\"type\":\"result\",\"n\":2}";
        let mut events = Events::new(output);
        let words = ["\"result\""];
        assert_eq!(events.next(&words).await.unwrap().unwrap()["n"], 1);
        // The last line may end without a newline.
        assert_eq!(events.next(&words).await.unwrap().unwrap()["n"], 2);
        assert!(events.next(&words).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn skips_lines_too_long_to_keep() {
        let long = format!("{{\"type\":\"result\",\"text\":\"{}\"}}\n", "x".repeat(100));
        let output = format!("{long}{{\"type\":\"result\",\"n\":3}}\n");
        let mut events = Events::with_limit(output.as_bytes(), 64);
        let event = events.next(&["\"result\""]).await.unwrap().unwrap();
        assert_eq!(event["n"], 3);
    }

    #[tokio::test]
    async fn takes_its_whole_group_along() {
        // A shell that leaves a grandchild running, as an AI CLI's node workers would.
        let mut command = Command::new("sh");
        command.args(["-c", "sleep 30 & echo $!; wait"]);
        command.stdout(std::process::Stdio::piped());
        let mut group = spawn(&mut command).unwrap();
        let stdout = group.stdout.take().unwrap();
        let mut lines = tokio::io::AsyncBufReadExt::lines(tokio::io::BufReader::new(stdout));
        let grandchild: i32 = lines.next_line().await.unwrap().unwrap().parse().unwrap();
        drop(group);
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        // Gone, or a zombie left for whoever adopted it to reap.
        let state = std::fs::read_to_string(format!("/proc/{grandchild}/stat")).unwrap_or_default();
        let alive = state
            .rsplit(')')
            .next()
            .is_some_and(|rest| !rest.trim_start().starts_with('Z') && !rest.is_empty());
        assert!(!alive, "the grandchild outlived its group");
    }
}
