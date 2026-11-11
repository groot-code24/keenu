# Privacy

Keenu has no server and no telemetry. It keeps what it needs on your computer and only reaches the network for
things you turned on. Everything below that watches, listens or goes online **starts off**.

## What it may keep

| What | Starts | Where it stays | What is kept | How long |
|---|---|---|---|---|
| Clipboard history | Off | This computer | What you copy: text, rich text, images, file paths. Not what an app marks as private. | 30 days by default, up to the last 100 unpinned; pinned items until you remove them |
| App usage | Off | This computer | An app's identifier, and when it came to the front and left. **Never** window titles, documents, keystrokes, screenshots or browsing history. | 30 days by default |
| File shelf | Empty until you drop something | This computer | A reference to the file and its name, size and date. The file itself is not copied. | Until you remove it |
| Calendar | Until you connect an account | This computer and your calendar service | A copy of your events, so they show while offline | Until you remove the account |
| Tasks and reminders | Empty | This computer | What you wrote | Until you delete them |
| Settings | | This computer | Your choices, and addresses of servers you configured. **Never** passwords, tokens or API keys. | Until you reset them |
| Log | | This computer | Errors and what the program was doing, with secrets removed. Never what you copy, say or are asked about. | Replaced as it grows, about 2 MB at most |
| Assistant memory (Windows, macOS) | Off | This computer, encrypted, its key in the credential store | Only what you tell it to remember or import: never anything it decides to keep by itself | Until you forget it; **Forget everything** deletes the file |
| Assistant action log | When the assistant first acts | This computer | Each action it took or was refused, with secrets removed. Never the text of what it read | Replaced as it grows, about 2 MB at most |
| Assistant briefing and tasks | Empty | This computer | When the briefing runs, and what you asked to have done | Until you remove them |
| Assistant connectors | None | This computer | Each connector's name, address and access. Its token goes to the credential store | Until you remove it |

Passwords, sign-in tokens and API keys go only to your system's credential store: Windows Credential Manager, the
macOS Keychain or the Secret Service on Linux. Where there is none, Keenu refuses to save them rather than writing
them to a file.

## What may leave your computer

Nothing leaves unless you turned on the feature that sends it:

| What | Goes to | What is sent | Starts |
|---|---|---|---|
| Online lyrics | [LRCLIB](https://lrclib.net) | The song's title, artist, album and length | Off |
| Calendar | The server you connected: yours, Google or Microsoft | Your sign-in; your events when you add or change one | Not connected |
| Voice with your endpoint | The address you entered | Your recording while you record, and what was heard | Local by default |
| Update check | The release manifest's address | A plain request, once a day | Off |
| Cover art (Linux) | The address the music player names | A plain download | When music plays |
| The assistant (Windows, macOS) | The AI you connected: ChatGPT with your own sign-in, an API you set up, a model on this computer, or Claude Code, Codex or Grok as installed and signed in by you | What you ask; the memories that match it, if memory is on; and what it reads to do the task: a window's controls and their text (never a password field's), a file's name or a small file's text | Off |
| Assistant web search | xAI, with your own key | The search | Not set up |
| Assistant connectors | The MCP server you added | What the AI asks that connector, with the access you gave it | None added |

On Linux, the optional AI assistant, its plan-usage page and meeting details have their own table in
[SECURITY.md](SECURITY.md#what-leaves-your-computer).

## The microphone

The microphone is opened only while you are recording, on your action, and the island says so the whole time. It
is closed the moment you stop or cancel, and it cannot start while voice is off. Audio is not kept after it is
turned into words.

"Hey Keenu" (Windows only) is off until you turn it on, and then needs the assistant and voice on as well. While
it is on, Windows' own speech engine listens on this computer for that phrase alone, and the island shows the
microphone as in use. Nothing it hears leaves the computer or is kept.

## Coding agents

When you connect Claude Code or Codex, their permission requests show on the island. The command and folder are
displayed and not stored. Keenu's answer goes only to the agent that asked.

## Your control

In **Settings**: pause or turn off any of the above; export what is kept to one file; delete it all; reset settings.
From a terminal: `keenu --export-data <file>`, `keenu --usage-clear`, `keenu --clipboard-history-clear`.

Uninstalling leaves your data, in case you come back. To remove it too, use **Delete my data** first, or delete the
folder: `%LOCALAPPDATA%\Keenu`, `~/Library/Application Support/Keenu` or `~/.local/share/keenu`.

See also [data retention](docs/data-retention.md) and [permissions](docs/permissions.md).
