// SPDX-License-Identifier: GPL-3.0-or-later

//! Secrets in the operating system's credential store: Windows Credential Manager, the macOS Keychain or
//! the Secret Service on Linux. Where there is none, saving fails: a secret is never written to a file.

use std::fmt;

/// Why a secret could not be read or saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialError {
    /// This system has no credential store Keenu can use; the text says what to install.
    Unavailable(String),
    /// The store refused or failed. Never holds the secret.
    Failed(String),
}

impl fmt::Display for CredentialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable(why) | Self::Failed(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for CredentialError {}

/// Somewhere to keep secrets, each under a key such as `calendar/<account id>`.
pub trait CredentialStore: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<String>, CredentialError>;
    fn set(&self, key: &str, secret: &str) -> Result<(), CredentialError>;
    /// Deleting what is not there succeeds.
    fn delete(&self, key: &str) -> Result<(), CredentialError>;
}

/// This operating system's store.
pub fn system_store() -> Box<dyn CredentialStore> {
    Box::new(platform::Store)
}

/// A store in memory, for tests.
#[derive(Debug, Default)]
pub struct MemoryStore(std::sync::Mutex<std::collections::BTreeMap<String, String>>);

impl CredentialStore for MemoryStore {
    fn get(&self, key: &str) -> Result<Option<String>, CredentialError> {
        Ok(self.0.lock().unwrap().get(key).cloned())
    }
    fn set(&self, key: &str, secret: &str) -> Result<(), CredentialError> {
        self.0.lock().unwrap().insert(key.into(), secret.into());
        Ok(())
    }
    fn delete(&self, key: &str) -> Result<(), CredentialError> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
}

#[cfg(windows)]
mod platform {
    use std::ptr::null_mut;

    use windows_sys::Win32::Foundation::{ERROR_NOT_FOUND, FILETIME, GetLastError};
    use windows_sys::Win32::Security::Credentials::{
        CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredFree,
        CredReadW, CredWriteW,
    };

    use super::{CredentialError, CredentialStore};

    /// Credential Manager holds at most 2560 bytes per entry, less than a Microsoft refresh token can
    /// be: longer secrets are split, `<target>#1`, `<target>#2`, after the first.
    const CHUNK: usize = 2048;

    pub struct Store;

    fn target(key: &str, part: usize) -> Vec<u16> {
        let name = if part == 0 {
            format!("Keenu/{key}")
        } else {
            format!("Keenu/{key}#{part}")
        };
        name.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn read_part(key: &str, part: usize) -> Result<Option<Vec<u8>>, CredentialError> {
        let name = target(key, part);
        let mut found: *mut CREDENTIALW = null_mut();
        // SAFETY: `name` ends in a NUL; `found` is freed below, once, when the call succeeded.
        unsafe {
            if CredReadW(name.as_ptr(), CRED_TYPE_GENERIC, 0, &mut found) == 0 {
                return match GetLastError() {
                    ERROR_NOT_FOUND => Ok(None),
                    code => Err(CredentialError::Failed(format!(
                        "Credential Manager could not read it (error {code})"
                    ))),
                };
            }
            let credential = &*found;
            let blob = if credential.CredentialBlobSize == 0 {
                Vec::new()
            } else {
                std::slice::from_raw_parts(
                    credential.CredentialBlob,
                    credential.CredentialBlobSize as usize,
                )
                .to_vec()
            };
            CredFree(found.cast());
            Ok(Some(blob))
        }
    }

    fn write_part(key: &str, part: usize, bytes: &[u8]) -> Result<(), CredentialError> {
        let mut name = target(key, part);
        let mut user: Vec<u16> = "Keenu".encode_utf16().chain(std::iter::once(0)).collect();
        let credential = CREDENTIALW {
            Flags: 0,
            Type: CRED_TYPE_GENERIC,
            TargetName: name.as_mut_ptr(),
            Comment: null_mut(),
            LastWritten: FILETIME {
                dwLowDateTime: 0,
                dwHighDateTime: 0,
            },
            CredentialBlobSize: bytes.len() as u32,
            CredentialBlob: bytes.as_ptr().cast_mut(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            AttributeCount: 0,
            Attributes: null_mut(),
            TargetAlias: null_mut(),
            UserName: user.as_mut_ptr(),
        };
        // SAFETY: every pointer in `credential` outlives the call, which copies what it needs.
        if unsafe { CredWriteW(&credential, 0) } == 0 {
            let code = unsafe { GetLastError() };
            return Err(CredentialError::Failed(format!(
                "Credential Manager could not save it (error {code})"
            )));
        }
        Ok(())
    }

    fn delete_part(key: &str, part: usize) -> Result<bool, CredentialError> {
        let name = target(key, part);
        // SAFETY: `name` ends in a NUL.
        if unsafe { CredDeleteW(name.as_ptr(), CRED_TYPE_GENERIC, 0) } == 0 {
            let code = unsafe { GetLastError() };
            if code == ERROR_NOT_FOUND {
                return Ok(false);
            }
            return Err(CredentialError::Failed(format!(
                "Credential Manager could not delete it (error {code})"
            )));
        }
        Ok(true)
    }

    impl CredentialStore for Store {
        fn get(&self, key: &str) -> Result<Option<String>, CredentialError> {
            let Some(mut all) = read_part(key, 0)? else {
                return Ok(None);
            };
            let mut part = 1;
            while let Some(more) = read_part(key, part)? {
                all.extend(more);
                part += 1;
            }
            String::from_utf8(all)
                .map(Some)
                .map_err(|_| CredentialError::Failed("the saved secret is damaged".into()))
        }

        fn set(&self, key: &str, secret: &str) -> Result<(), CredentialError> {
            self.delete(key)?;
            let bytes = secret.as_bytes();
            if bytes.is_empty() {
                return write_part(key, 0, bytes);
            }
            for (part, chunk) in bytes.chunks(CHUNK).enumerate() {
                write_part(key, part, chunk)?;
            }
            Ok(())
        }

        fn delete(&self, key: &str) -> Result<(), CredentialError> {
            let mut part = 0;
            while delete_part(key, part)? {
                part += 1;
            }
            Ok(())
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use security_framework::passwords::{
        delete_generic_password, get_generic_password, set_generic_password,
    };

    use super::{CredentialError, CredentialStore};

    const SERVICE: &str = "Keenu";
    /// `errSecItemNotFound`.
    const NOT_FOUND: i32 = -25300;

    pub struct Store;

    impl CredentialStore for Store {
        fn get(&self, key: &str) -> Result<Option<String>, CredentialError> {
            match get_generic_password(SERVICE, key) {
                Ok(bytes) => String::from_utf8(bytes)
                    .map(Some)
                    .map_err(|_| CredentialError::Failed("the saved secret is damaged".into())),
                Err(err) if err.code() == NOT_FOUND => Ok(None),
                Err(err) => Err(CredentialError::Failed(format!(
                    "the Keychain could not read it (error {})",
                    err.code()
                ))),
            }
        }

        fn set(&self, key: &str, secret: &str) -> Result<(), CredentialError> {
            set_generic_password(SERVICE, key, secret.as_bytes()).map_err(|err| {
                CredentialError::Failed(format!(
                    "the Keychain could not save it (error {})",
                    err.code()
                ))
            })
        }

        fn delete(&self, key: &str) -> Result<(), CredentialError> {
            match delete_generic_password(SERVICE, key) {
                Ok(()) => Ok(()),
                Err(err) if err.code() == NOT_FOUND => Ok(()),
                Err(err) => Err(CredentialError::Failed(format!(
                    "the Keychain could not delete it (error {})",
                    err.code()
                ))),
            }
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod platform {
    use std::io::Write;
    use std::process::{Command, Stdio};

    use super::{CredentialError, CredentialStore};

    /// `secret-tool` from libsecret talks to the desktop's Secret Service (GNOME Keyring, KWallet). The
    /// secret goes over its standard input, never its command line, where other programs could see it.
    const TOOL: &str = "secret-tool";

    pub struct Store;

    fn unavailable(err: std::io::Error) -> CredentialError {
        if err.kind() == std::io::ErrorKind::NotFound {
            CredentialError::Unavailable(
                "No Secret Service tool found: install libsecret-tools (Debian, Ubuntu) or libsecret \
                 (Fedora). Keenu will not keep this secret in a plain file."
                    .into(),
            )
        } else {
            CredentialError::Failed(format!("could not run {TOOL}: {err}"))
        }
    }

    impl CredentialStore for Store {
        fn get(&self, key: &str) -> Result<Option<String>, CredentialError> {
            let output = Command::new(TOOL)
                .args(["lookup", "application", "keenu", "key", key])
                .stdin(Stdio::null())
                .output()
                .map_err(unavailable)?;
            if !output.status.success() || output.stdout.is_empty() {
                return Ok(None);
            }
            String::from_utf8(output.stdout)
                .map(Some)
                .map_err(|_| CredentialError::Failed("the saved secret is damaged".into()))
        }

        fn set(&self, key: &str, secret: &str) -> Result<(), CredentialError> {
            let label = format!("Keenu: {key}");
            let mut child = Command::new(TOOL)
                .args([
                    "store",
                    "--label",
                    &label,
                    "application",
                    "keenu",
                    "key",
                    key,
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(unavailable)?;
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(secret.as_bytes()).map_err(|err| {
                    CredentialError::Failed(format!("could not pass it on: {err}"))
                })?;
            }
            let status = child
                .wait()
                .map_err(|err| CredentialError::Failed(format!("{TOOL} failed: {err}")))?;
            if status.success() {
                Ok(())
            } else {
                Err(CredentialError::Unavailable(
                    "The Secret Service refused it: is a keyring running and unlocked in this \
                     session? Keenu will not keep this secret in a plain file."
                        .into(),
                ))
            }
        }

        fn delete(&self, key: &str) -> Result<(), CredentialError> {
            Command::new(TOOL)
                .args(["clear", "application", "keenu", "key", key])
                .stdin(Stdio::null())
                .output()
                .map_err(unavailable)?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_memory_store_behaves_like_a_store() {
        let store = MemoryStore::default();
        assert_eq!(store.get("a"), Ok(None));
        store.set("a", "secret").unwrap();
        assert_eq!(store.get("a").unwrap().as_deref(), Some("secret"));
        store.delete("a").unwrap();
        store.delete("a").unwrap();
        assert_eq!(store.get("a"), Ok(None));
    }

    /// Against the real Windows Credential Manager, under a key used only by this test.
    #[cfg(windows)]
    #[test]
    fn windows_credential_manager_round_trips_secrets_of_any_length() {
        let store = system_store();
        let key = format!("test/{}", std::process::id());
        for secret in ["short", &"x".repeat(2048), &"long-token-".repeat(900)] {
            store.set(&key, secret).unwrap();
            assert_eq!(store.get(&key).unwrap().as_deref(), Some(secret));
        }
        // A shorter secret leaves no old parts behind.
        store.set(&key, "short again").unwrap();
        assert_eq!(store.get(&key).unwrap().as_deref(), Some("short again"));
        store.delete(&key).unwrap();
        assert_eq!(store.get(&key), Ok(None));
        store.delete(&key).unwrap();
    }
}
