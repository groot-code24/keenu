// SPDX-License-Identifier: GPL-3.0-or-later

//! Between an agent's hook and the running Keenu: a named pipe only this user can open, a token only
//! this user can read, and one line of JSON each way. Nothing received here is ever run.

use std::io;
use std::time::{Duration, Instant};

use keenu_integrations::agents::protocol::{Answer, ApprovalRequest, Message};

use windows::Win32::Foundation::{
    CloseHandle, ERROR_BROKEN_PIPE, ERROR_FILE_NOT_FOUND, ERROR_IO_PENDING, ERROR_MORE_DATA,
    ERROR_NO_DATA, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, GENERIC_READ, GENERIC_WRITE, HANDLE,
    HLOCAL, INVALID_HANDLE_VALUE, LocalFree,
};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    TokenUser,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, FILE_GENERIC_READ,
    FILE_GENERIC_WRITE, FILE_SHARE_NONE, FlushFileBuffers, OPEN_EXISTING, PIPE_ACCESS_DUPLEX,
    ReadFile, SECURITY_IDENTIFICATION, WriteFile,
};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
    PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT, PeekNamedPipe,
    WaitNamedPipeW,
};
use windows::Win32::System::Threading::{CreateEventW, GetCurrentProcess, OpenProcessToken};
use windows_core::{HSTRING, PCWSTR, PWSTR};

/// The most a message may be: a request's bounded fields, in JSON.
const MAX_MESSAGE: usize = 256 * 1024;

/// This user's SID as text: names the pipe and locks it.
fn current_user_sid() -> io::Result<String> {
    // SAFETY: the handle and buffers are local and sized by the calls that fill them.
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)?;
        let mut size = 0;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut size);
        let mut buffer = vec![0u8; size as usize];
        let result = GetTokenInformation(
            token,
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            size,
            &mut size,
        );
        let _ = CloseHandle(token);
        result?;
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        let mut text = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut text)?;
        let sid = text.to_string().map_err(io::Error::other);
        let _ = LocalFree(Some(HLOCAL(text.0.cast())));
        sid
    }
}

/// The pipe for this user's Keenu. Its name is per user so that one user's hook never reaches another's.
pub fn pipe_name() -> io::Result<String> {
    Ok(format!(r"\\.\pipe\keenu-agents-{}", current_user_sid()?))
}

/// Why waiting on a client stopped.
enum Wait {
    Done(usize),
    /// The client went: answered in its terminal, or killed.
    Gone,
    TimedOut,
}

/// One end of a pipe, overlapped so waits can time out and notice the other end leaving.
struct Pipe {
    handle: HANDLE,
    event: HANDLE,
}

impl Drop for Pipe {
    fn drop(&mut self) {
        // SAFETY: both are ours and closed once.
        unsafe {
            let _ = CloseHandle(self.handle);
            let _ = CloseHandle(self.event);
        }
    }
}

// SAFETY: pipe and event handles may be used from any thread.
unsafe impl Send for Pipe {}

impl Pipe {
    fn from_handle(handle: HANDLE) -> io::Result<Self> {
        // SAFETY: a plain manual-reset event.
        let event = unsafe { CreateEventW(None, true, false, None)? };
        Ok(Self { handle, event })
    }

    fn wait(&self, overlapped: &mut OVERLAPPED, until: Instant) -> Wait {
        loop {
            let mut transferred = 0;
            // SAFETY: `overlapped` outlives the wait; the operation is cancelled before leaving on timeout.
            let result =
                unsafe { GetOverlappedResult(self.handle, overlapped, &mut transferred, false) };
            match result {
                Ok(()) => return Wait::Done(transferred as usize),
                Err(err) if err.code() == ERROR_BROKEN_PIPE.to_hresult() => return Wait::Gone,
                Err(err) if err.code() == ERROR_NO_DATA.to_hresult() => return Wait::Gone,
                Err(err) if err.code() == ERROR_MORE_DATA.to_hresult() => {
                    return Wait::Done(transferred as usize);
                }
                Err(err)
                    if err.code()
                        != windows::Win32::Foundation::ERROR_IO_INCOMPLETE.to_hresult() =>
                {
                    return Wait::Gone;
                }
                Err(_) => {}
            }
            if Instant::now() >= until {
                // SAFETY: cancels only this operation, then waits for it to end.
                unsafe {
                    let _ = CancelIoEx(self.handle, Some(overlapped));
                    let _ = GetOverlappedResult(self.handle, overlapped, &mut transferred, true);
                }
                return Wait::TimedOut;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn read_some(&self, buffer: &mut [u8], until: Instant) -> Wait {
        let mut overlapped = OVERLAPPED {
            hEvent: self.event,
            ..OVERLAPPED::default()
        };
        // SAFETY: `buffer` and `overlapped` stay in place until `wait` has ended the operation.
        unsafe {
            if let Err(err) = ReadFile(self.handle, Some(buffer), None, Some(&mut overlapped))
                && err.code() != ERROR_IO_PENDING.to_hresult()
            {
                return Wait::Gone;
            }
        }
        self.wait(&mut overlapped, until)
    }

    fn read_line(&self, until: Instant) -> Option<Vec<u8>> {
        let mut line = Vec::new();
        let mut buffer = [0u8; 8192];
        loop {
            let Wait::Done(read) = self.read_some(&mut buffer, until) else {
                return None;
            };
            if read == 0 {
                return None;
            }
            line.extend_from_slice(&buffer[..read]);
            if line.len() > MAX_MESSAGE {
                return None;
            }
            if let Some(end) = line.iter().position(|&byte| byte == b'\n') {
                line.truncate(end);
                return Some(line);
            }
        }
    }

    fn write_all(&self, bytes: &[u8], until: Instant) -> bool {
        let mut overlapped = OVERLAPPED {
            hEvent: self.event,
            ..OVERLAPPED::default()
        };
        // SAFETY: as in `read_some`.
        unsafe {
            if let Err(err) = WriteFile(self.handle, Some(bytes), None, Some(&mut overlapped))
                && err.code() != ERROR_IO_PENDING.to_hresult()
            {
                return false;
            }
        }
        matches!(self.wait(&mut overlapped, until), Wait::Done(_))
    }

    /// Whether the other end has gone, without taking anything from the pipe.
    fn client_gone(&self) -> bool {
        // SAFETY: only asks about the pipe.
        unsafe { PeekNamedPipe(self.handle, None, 0, None, None, None).is_err() }
    }
}

/// Makes a server instance only this user can open.
fn create_server_instance(name: &str, first: bool) -> io::Result<Pipe> {
    let sddl = HSTRING::from(format!("D:P(A;;GA;;;{})", current_user_sid()?));
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: `descriptor` is freed below after the pipe has copied it.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: false.into(),
        };
        let mut flags = PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED;
        // The first refuses to start if someone else holds the name already.
        if first {
            flags |= FILE_FLAG_FIRST_PIPE_INSTANCE;
        }
        let handle = CreateNamedPipeW(
            &HSTRING::from(name),
            flags,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            64 * 1024,
            64 * 1024,
            0,
            Some(&attributes),
        );
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        Pipe::from_handle(handle)
    }
}

/// A request received, waiting for its answer.
pub struct Incoming {
    pub request: ApprovalRequest,
    pipe: Pipe,
}

impl Incoming {
    /// Whether the hook has gone away, because the agent got its answer elsewhere.
    pub fn abandoned(&self) -> bool {
        self.pipe.client_gone()
    }

    /// Sends the answer back.
    pub fn answer(self, answer: &Answer) {
        let mut line = serde_json::to_vec(answer).unwrap_or_default();
        line.push(b'\n');
        let until = Instant::now() + Duration::from_secs(2);
        if self.pipe.write_all(&line, until) {
            // SAFETY: the pipe is open; lets the hook read before it is closed.
            unsafe {
                let _ = FlushFileBuffers(self.pipe.handle);
                let _ = DisconnectNamedPipe(self.pipe.handle);
            }
        }
    }
}

/// Serves hooks forever, calling `on_request` with each well-formed request with the right token.
pub fn serve(
    token: String,
    on_request: impl Fn(Incoming) + Send + Sync + 'static,
) -> io::Result<()> {
    let name = pipe_name()?;
    let mut server = create_server_instance(&name, true)?;
    let on_request = std::sync::Arc::new(on_request);
    std::thread::Builder::new()
        .name("keenu-agent-ipc".into())
        .spawn(move || {
            loop {
                let mut overlapped = OVERLAPPED {
                    hEvent: server.event,
                    ..OVERLAPPED::default()
                };
                // SAFETY: `overlapped` lives until the wait returns.
                let connected = unsafe { ConnectNamedPipe(server.handle, Some(&mut overlapped)) };
                let ready = match connected {
                    Ok(()) => true,
                    Err(err) if err.code() == ERROR_PIPE_CONNECTED.to_hresult() => true,
                    Err(err) if err.code() == ERROR_IO_PENDING.to_hresult() => {
                        let far = Instant::now() + Duration::from_secs(60 * 60 * 24 * 365);
                        matches!(server.wait(&mut overlapped, far), Wait::Done(_))
                    }
                    Err(_) => false,
                };
                let next = create_server_instance(&name, false);
                let Ok(next) = next else {
                    log::warn!("the agent pipe could not make another instance");
                    std::thread::sleep(Duration::from_secs(1));
                    continue;
                };
                let current = std::mem::replace(&mut server, next);
                if ready {
                    let token = token.clone();
                    let on_request = on_request.clone();
                    // A slow or hostile client holds up only itself.
                    std::thread::spawn(move || handle_client(current, &token, &*on_request));
                }
            }
        })?;
    Ok(())
}

fn handle_client(pipe: Pipe, token: &str, on_request: &dyn Fn(Incoming)) {
    let until = Instant::now() + Duration::from_secs(5);
    let Some(line) = pipe.read_line(until) else {
        return;
    };
    let Ok(message) = serde_json::from_slice::<Message>(&line) else {
        return;
    };
    if !keenu_security::constant_time_eq(message.token.as_bytes(), token.as_bytes()) {
        log::warn!("an agent request came with the wrong token: ignored");
        return;
    }
    on_request(Incoming {
        request: message.request,
        pipe,
    });
}

/// The hook's side: sends `message` and waits for the answer. Anything but an answer in time is
/// `NoDecision`, so the agent asks in its own terminal.
pub fn request(message: &Message, timeout: Duration) -> Answer {
    try_request(message, timeout).unwrap_or(Answer::NoDecision)
}

fn try_request(message: &Message, timeout: Duration) -> Option<Answer> {
    let name = HSTRING::from(pipe_name().ok()?);
    let until = Instant::now() + timeout;
    let handle = loop {
        // SAFETY: opens a named pipe; identification level, so a false server cannot act as this user.
        let opened = unsafe {
            CreateFileW(
                &name,
                (GENERIC_READ | GENERIC_WRITE).0 | FILE_GENERIC_READ.0 | FILE_GENERIC_WRITE.0,
                FILE_SHARE_NONE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED | SECURITY_IDENTIFICATION,
                None,
            )
        };
        match opened {
            Ok(handle) => break handle,
            Err(err) if err.code() == ERROR_FILE_NOT_FOUND.to_hresult() => return None,
            Err(err) if err.code() == ERROR_PIPE_BUSY.to_hresult() => {
                // SAFETY: waits briefly for a free instance.
                let _ = unsafe { WaitNamedPipeW(&name, 2000) };
                if Instant::now() >= until {
                    return None;
                }
            }
            Err(_) => return None,
        }
    };
    let pipe = Pipe::from_handle(handle).ok()?;
    let mut line = serde_json::to_vec(message).ok()?;
    line.push(b'\n');
    if !pipe.write_all(&line, Instant::now() + Duration::from_secs(5)) {
        return None;
    }
    let reply = pipe.read_line(until)?;
    serde_json::from_slice(&reply).ok()
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use keenu_integrations::agents::Agent;

    use super::*;

    fn sample(id: &str) -> ApprovalRequest {
        ApprovalRequest {
            id: id.into(),
            agent: Agent::Claude,
            tool: "Bash".into(),
            detail: "npm test".into(),
            truncated: false,
            cwd: "C:\\code".into(),
            project: "code".into(),
            description: String::new(),
            suggested_permissions: Vec::new(),
        }
    }

    /// Against real named pipes. They share one server because the pipe name is per user.
    #[test]
    fn requests_and_answers_cross_a_real_pipe() {
        let (sender, receiver) = mpsc::channel();
        let sender = std::sync::Mutex::new(sender);
        serve("right-token".into(), move |incoming| {
            sender.lock().unwrap().send(incoming).unwrap();
        })
        .unwrap();

        // An answer gets back to the hook.
        let client = std::thread::spawn(|| {
            request(
                &Message {
                    token: "right-token".into(),
                    request: sample("r1"),
                },
                Duration::from_secs(10),
            )
        });
        let incoming: Incoming = receiver.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(incoming.request.id, "r1");
        assert!(!incoming.abandoned());
        incoming.answer(&Answer::Deny);
        assert_eq!(client.join().unwrap(), Answer::Deny);

        // The wrong token is ignored: the hook gets no decision, so the terminal asks.
        let answer = request(
            &Message {
                token: "wrong".into(),
                request: sample("r2"),
            },
            Duration::from_secs(2),
        );
        assert_eq!(answer, Answer::NoDecision);
        assert!(receiver.recv_timeout(Duration::from_millis(200)).is_err());

        // No answer in time is no decision.
        let answer = request(
            &Message {
                token: "right-token".into(),
                request: sample("r3"),
            },
            Duration::from_millis(500),
        );
        assert_eq!(answer, Answer::NoDecision);
        let incoming = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        // That hook has left, which the server can tell.
        std::thread::sleep(Duration::from_millis(100));
        assert!(incoming.abandoned());
    }
}
