# Security model

How Keenu is built to be safe, and where each protection is. [The threat model](threat-model.md) says what it defends
against; [SECURITY.md](../SECURITY.md) says how to report a problem.

## Principles

1. **Off until asked.** Everything that watches, listens or reaches out starts off. `Settings::default` is the only
   place that decides this, and a test holds it to that.
2. **Failure never grants.** A timeout, a closed program or an unreadable request leaves things as they would be
   without Keenu.
3. **Nothing from outside is run.** Hook payloads, calendar data and manifests are data. They are shown or stored, and
   no field in them becomes a command.
4. **What is on screen is what is true.** The UI is rebuilt from the real state: the microphone indicator comes from
   the recording itself, and a capability that cannot work shows why.

## Secrets

| | |
|---|---|
| Where | `keenu_security::credentials`: Windows Credential Manager, the macOS Keychain, the Secret Service on Linux |
| If there is no store | Saving fails with what to install. There is no fallback to a file. |
| Long values | Windows' limit is 2560 bytes per entry, under a Microsoft refresh token's size, so longer values are split |
| On Linux | `secret-tool` takes the secret on standard input, never on its command line, where other programs could see it |
| In memory | `Tokens` has a `Debug` that prints nothing |
| In logs | Every log line goes through `keenu_core::redact`: bearer tokens, `password=`, API-key shapes and URL passwords |
| In errors | An OAuth or API error reports the status and error code, never the answer's text, which can echo what was sent |
| In settings | There is no field for one. A test saves a key through the UI and checks it is not in `settings.json` |

## Coding-agent approvals

The hook protocol is the vendors' own: [Claude Code](https://code.claude.com/docs/en/hooks) and
[Codex](https://learn.chatgpt.com/docs/hooks), checked 2026-10-31. Printing nothing means "no decision", and the
agent then asks in its terminal.

| Risk | Protection | Where |
|---|---|---|
| Another user reaches the pipe | The name holds this user's SID; its access list allows only that SID; remote clients are refused | `ipc.rs` |
| Another program holds the name first | `FILE_FLAG_FIRST_PIPE_INSTANCE`: the real server refuses to start rather than share it | `ipc.rs` |
| A false server reads the request | The client connects at identification level, so a server cannot act as the user | `ipc.rs` |
| A forged request from a program of this user's | A random token, fresh each start, in a file only this user can read. This raises the bar; it is no boundary against this user's own programs, see the threat model | `ipc.rs`, `host.rs` |
| Replay, or a second answer | Ids are remembered, and an answer takes its request out of the queue first | `replay.rs`, `app.rs` |
| A command disguised on screen | Control characters and those that reorder or hide text become a visible `�`; cleaned in the hook and again on arrival | `protocol.rs`, `app.rs` |
| Allowing what cannot be seen | A command too long to show is marked, with a warning | `protocol.rs`, `view.rs` |
| A click meant for something else | **Allow ignores clicks for its first 700 ms**: the prompt opens by itself, wherever the pointer is. Deny needs no wait | `app.rs` |
| A key meant for something else | Keyboard focus resets for each new request, and Enter with nothing focused does nothing; the prompt does not take focus | `app.rs` |
| Fields the agent would reject | Only documented fields are sent, and `message` only with a denial | `protocol.rs` |
| Payload as a command | Nothing in it is ever run. Tool names must match a strict pattern. | `protocol.rs` |
| A permanent rule added by accident | Keenu never sends `updatedPermissions` | `protocol.rs` |
| Damaging the agent's settings | Only its own entries change; invalid JSON is refused; first-time backup, atomic write, put back on failure | `agents/` |

## Updates and downloads

Three independent checks, all before anything is run:

1. **The manifest's Ed25519 signature**, against a public key compiled into the program. `keenu_security::integrity`
   uses `verify_strict`, which refuses weak keys.
2. **The file's SHA-256**, from that signed manifest. A download gets its real name only after it matches; otherwise it
   is deleted. It may not be larger than published.
3. **The platform's signature.** On Windows, `WinVerifyTrust` with revocation checks, then the signer's thumbprint
   against one fixed in the build (`KEENU_SIGNING_THUMBPRINT`). It is checked again just before installing.

A build without a key and thumbprint **cannot update**, and says so. Installing goes through Windows Installer, which
rolls back on failure. Addresses must be HTTPS. Plain HTTP is allowed only to `127.0.0.1`, for rehearsing a release.
Automatic installing is opt-in on top of opt-in checking.

`install.sh` also checks the manifest's signature, where OpenSSL 3 is available. `install.ps1` does not: PowerShell 5.1
has no Ed25519. It relies on HTTPS, the SHA-256, and Authenticode when the file claims to be signed.

## Input handling

| Input | Handling |
|---|---|
| Paths from drops and saved files | Must be absolute, with no `..` (`validate_path`), and are only ever opened through the shell |
| Lyrics file names | From song metadata, cleaned of separators and dots, so a title cannot leave the lyrics folder |
| Clipboard images | Width, height and buffer checked against the header before any read; size limits |
| Calendar text | iCalendar and XML are parsed, never interpreted. No XML DTD support, so no external entities |
| Server addresses in answers | Followed only if secure; Graph's `nextLink` only on its own host |
| Toast text | XML-escaped; control characters removed |
| The page's manifest | Written as text, never as markup; only `https://` becomes a link |
| CSV export | Fields beginning like a formula are defused |
| `keenu.exe`'s own helpers | `msiexec`, `cmd` and PowerShell by full path under `%SystemRoot%`, so `PATH` cannot substitute them |

## Files

Data is under the user's own folder. On Linux and macOS, files are created `0600` and folders `0700`. On Windows,
`%LOCALAPPDATA%` already excludes other users. Saves are atomic. A corrupt file is moved aside as `.corrupt`, and one
that cannot be read turns off saving rather than overwriting what was never seen.

## Clipboard

History respects `ExcludeClipboardContentFromMonitorProcessing` and `CanIncludeInClipboardHistory`, which password
managers set. This is tested against the real Windows clipboard. Reading happens only while history is on, and an
automated test confirms the clipboard is not read at all otherwise.

## Dependencies and CI

`cargo deny` checks advisories, licences and sources; `npm audit` and gitleaks run in
[security.yml](../.github/workflows/security.yml). Workflow permissions are read-only by default; only the final
publish step can write. Release artifacts carry build provenance attestations.

## What is not protected

Stated plainly:

- **Your own programs.** Another program running as you can read your clipboard history and usage files, and can
  answer the agent token. On a desktop, the user account is the boundary.
- **Data at rest** is not encrypted beyond the secrets above. Use full-disk encryption.
- **The first install** trusts HTTPS for the script itself, as any `curl | sh` does. Read it first if that concerns you.
- **Unsigned builds.** Until a certificate is configured, Windows and macOS will warn people, rightly, and updates are
  off.
