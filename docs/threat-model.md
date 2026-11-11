# Threat model

## What is worth protecting

| Asset | Why |
|---|---|
| The agent approval decision | An allow lets an agent run a command as you |
| The update path | Controlling it means running code on every install |
| Passwords, OAuth tokens, API keys | Access to your calendar or paid services |
| Clipboard history | Can hold anything you copied |
| Calendar, usage and shelf data | Personal |
| Agents' settings files | Keenu edits files that belong to other programs |

## Who might attack

| Attacker | Can | In scope? |
|---|---|---|
| A network attacker | See and change traffic | **Yes** |
| A compromised mirror or update host | Serve anything at the download addresses | **Yes** |
| Another user on the computer | Run programs, as themselves | **Yes** |
| A malicious calendar server, music app or lyrics source | Send any data | **Yes** |
| A prompt-injected or hostile coding agent | Ask for anything through its hook; put anything in a payload | **Yes** |
| A web page in your browser | Reach loopback ports | **Yes**, for the OAuth redirect |
| A program already running as you | Almost anything you can do | **No**: see below |
| An administrator, or malware with admin rights | Everything | **No** |
| Physical access to an unlocked computer | Everything you can do | **No** |

## Threats

**S**poofing, **T**ampering, **R**epudiation, **I**nformation disclosure, **D**enial of service, **E**levation.

| # | Threat | Type | Mitigation | Remaining risk |
|---|---|---|---|---|
| 1 | Update host serves a malicious installer | T, E | Signed manifest, SHA-256 from it, and Authenticode with a pinned signer | The signing keys themselves: keep them in secrets, and rotate by shipping a new build |
| 2 | Attacker replays an old, signed manifest to prevent updates or downgrade | T | Only newer versions are ever taken, so no downgrade. A freeze is possible | Manifests carry no expiry, so a held-back user sees "up to date". Accepted for now |
| 3 | Tampered download through `install.ps1` or `install.sh` | T | SHA-256 from the manifest over HTTPS; Ed25519 on the manifest in `install.sh` with OpenSSL 3 | `install.ps1` trusts HTTPS for the manifest: a compromised host can change manifest and file together, unless the file is signed |
| 4 | Agent payload makes a dangerous command look harmless | S | Control and bidirectional characters made visible; long commands marked; working folder shown | A long, harmless-looking command is still the user's to read |
| 5 | Hostile agent floods requests | D | Answers queue one at a time; each times out to "no decision" | Annoying: the fix is closing the agent |
| 6 | A popup catches a stray click on Allow | E | Allow ignores clicks for its first 700 ms | A deliberate quick click after that. **Found in testing**: a person clicked prompts that appeared during a test run |
| 7 | Replayed request reuses an allow | E | Ids remembered; each hook makes a new one; an allow is never reused | |
| 8 | Another user connects to the pipe | E, I | Access list naming only this SID; no remote clients | |
| 9 | Another program claims the pipe's name first | S, I | The real server refuses to start, and the agent falls back to its terminal; the client is at identification level | A same-user program could still impersonate: out of scope |
| 10 | A web page sends a forged OAuth redirect | S | Random `state` compared in constant time; a wrong one is refused and the wait goes on | |
| 11 | Stolen authorization code | S, I | PKCE: the code is useless without the verifier, which never leaves this computer | |
| 12 | A token ends up in a log or bug report | I | Redactor on every line and in diagnostics; errors hold codes, not answers | Redaction is pattern-based, so an unusual format could pass: no code logs tokens on purpose |
| 13 | A malicious CalDAV server sends hostile XML or iCalendar | T, D | No DTDs or external entities; a bounded read; damaged items skipped; a bound on repeat expansion | |
| 14 | A malicious server redirects credentials elsewhere | I | Insecure addresses refused; Graph links only on its own host | CalDAV follows hrefs on any HTTPS host named by the server you chose |
| 15 | Hostile music metadata reaches a file path | T | Title and artist cleaned before forming a file name | |
| 16 | A crafted clipboard bitmap reads out of bounds | I, D | All sizes checked before reading | |
| 17 | Password manager's copied secret lands in history | I | Windows' two privacy formats honoured, and tested for real | An app that does not set them is treated as ordinary. On by your choice |
| 18 | The island shows a false state | S | Rebuilt from real state; the microphone indicator from the device thread | |
| 19 | PATH makes Keenu run a fake `msiexec` or PowerShell | E | Full paths under `%SystemRoot%`. **Found in testing** | |
| 20 | Editing an agent's settings loses the user's data | T | Refuses invalid JSON; keeps other keys in order; atomic; backup; restored on failure | |
| 21 | Editing a calendar event loses attendees or alarms | T | The server's object is fetched and only known fields replaced. **Found in testing** | |
| 22 | Migration overwrites or loses data | T | Copies; never overwrites; old folder untouched; logged | |
| 23 | Text in a window, file, web result or connector tells the assistant to do something (prompt injection) | E, I | Everything read is handed to the AI marked as data, not instructions; the AI can only propose actions, and Keenu's own code grades each one: looking runs, anything that changes something asks on the island card, and a fixed list is refused outright | A user who approves without reading. The card shows the action in Keenu's words, not the AI's |
| 24 | The AI clicks or types in the wrong window | T | A window is picked by its id, or by a title only one window has; "more than one could be" is an error; each click and keystroke asks first and names the window | A window that changes between the card and the click |
| 25 | The assistant reads or fills a password field | I | Refused in platform code by what the system says the field is (UI Automation's IsPassword, macOS's AXSecureTextField), not by its name; its value is never asked for. Tested on a real password box by the Windows smoke test | An app that draws its own password field without telling the system |
| 26 | A scheduled task acts while nobody is there | E | A request with no one at the computer can only look: anything that would ask is refused and reported | |
| 27 | A forged ChatGPT sign-in redirect, or a stolen code | S, I | As 10 and 11: random state and nonce, PKCE, a loopback address on 127.0.0.1 only, one sign-in at a time | |
| 28 | An API key, ChatGPT token or connector token leaks | I | Kept only in the credential store; never in settings, the action log or the conversation; an API address must be HTTPS unless it is on this computer | A key the user pastes into a request: Keenu cannot tell it from text |
| 29 | Someone with the user's files reads the assistant's memory | I | Encrypted with ChaCha20-Poly1305; the key is in the credential store, not beside the file | Anyone signed in as the user can open the credential store too |
| 30 | A connector does more than it was allowed | E | Each connector starts off; read access offers only its tools that only read; writing asks every time | The server's own description of a tool decides whether it only reads |
| 31 | The microphone listens without the user knowing | I | "Hey Keenu" is off by default, needs the assistant and voice on, and shows a red dot on the closed island and a line on the Voice page while it listens; Windows' engine runs on this computer | |
| 32 | Every action is logged, or nothing is | R | An action is not taken if the log cannot be written; the log has secrets removed and stays on this computer | |

## Out of scope, and why

**A program already running as you** can read your files, log your keys, and answer the pipe with the token. Desktop
operating systems do not separate your own programs from each other, and nothing Keenu could do would change that. The
token and access list stop other users and accidents; they do not stop your own account.

**Supply chain of dependencies** is reduced, not eliminated: `cargo deny` checks advisories, sources are limited to
crates.io, and `Cargo.lock` is committed.

## Findings during development

Testing against real systems, not mocks, found:

| Found by | Problem | Fix |
|---|---|---|
| Real clicks during a test | A prompt can appear under the pointer and take a stray click | Allow's arming delay; focus reset |
| Running on a real machine | `powershell.exe` was not on `PATH` at all, and lookup by name is hijackable | Full paths |
| Real server (Radicale) | Deleted events were not recognised, because `@` came back as `%40` | Canonical paths |
| Repeating that test | Requests were lost at random on a connection the server was about to close, which could lose an edit. The test's stand-in server, closing at once, could not show it | Connections are not reused. The stand-in now closes late, and the test failed before the fix |
| `cargo deny` | `quick-xml` 0.37 had advisories for slow parsing and unbounded memory (RUSTSEC-2026-0194, -0195). Keenu uses a plain `Reader` and no attributes, so neither path was reached | Upgraded to 0.42 |
| Reading the parser | A server's deeply nested XML could overflow the stack, which ends the program instead of failing a sync | 64 levels at most |
| Reading the code after that | Updating a CalDAV event replaced its UID, attendees and alarms | Fetch and patch |
| Real shutdown | Closing hung and never ran shutdown, so unsaved state was lost | Host released before default handling |
| Real install | `install.ps1 -Uninstall` said "removed" with Keenu still there | Finds the MSI's product, and checks the result |
| Renaming, reading before changing | The extension's command allow-list would have refused every approved command | Both sides renamed together, with tests |
| The audit | The Windows edition checked for updates in the upstream author's repository | The repository must be set explicitly, or nothing is looked up |
| Reading what the rename touched | The extension's Update button ran a script fetched from `github.com/YOUR-GITHUB-OWNER`: a placeholder anyone could register, then running their code on users' computers | The real repository is now in the source, and a release refuses to publish a package with a placeholder |
| A clean GitHub runner | Start at login failed, and could not be turned on, on a profile with no Run key | Creates the key when turning it on; no key means off |
| Reading the published program's own report | A workflow's empty setting made a build with no certificate say updates were available. It still refused them: an empty pin matches no signer | An empty setting counts as not set. **Fixed after 2.0.0-alpha.1** |
