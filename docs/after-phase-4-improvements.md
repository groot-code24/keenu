# After phase 4: UI/UX and feature improvements

Status: proposal. Nothing in this file is built. During phases 0 to 4 ([assistant-plan.md](assistant-plan.md)) the
island's look, theme and layout stayed as they were, by request; this file lists what would change once those
phases are done, and why. Each item needs a yes before work starts.

Sources: Keenu as it stands on the `assistant-phases` branch, and [Isle](https://github.com/zickrian/isle), the
project Keenu is based on, read at its commit a29a7c4 (8 November 2026, after release 1.7.1). Keenu was taken from
Isle 1.5.0; Isle has had five releases since (1.5.1, 1.5.2, 1.5.3, 1.6.0, 1.7.0, 1.7.1).

## Where the two stand

| | Isle 1.7.1 (Linux, GNOME only) | Keenu on Linux | Keenu on Windows and macOS |
|---|---|---|---|
| Island | Black or Glass, notch or pill, normal or large | Isle 1.5.0's | Black, one size, drawn by Keenu itself |
| Pages | Music, Call, Timer, Reminders, Shelf, Calendar, Notifications, Assistant, AI usage, Apps, Devices, Settings | Isle 1.5.0's pages | Media, Calendar, Clipboard, Shelf, Voice, Agents, Usage, Settings |
| Assistant | Bot with a face, conversation, command cards with risk labels | Isle 1.5.0's, on keenud | Text and voice on the Voice page, approval card, action log |
| AI plan usage | Reads Claude and ChatGPT sign-in tokens to show limits | Removed in phase 0 | Removed in phase 0 |

The last row matters. Isle's AI usage page reads the sign-in tokens of Claude Code and Codex and calls their usage
endpoints. Keenu stopped doing that in phase 0 because Anthropic's terms do not allow third-party apps to handle
Claude sign-in tokens. That page is not ported back, whatever else is.

## 1. Bring over what Isle fixed and improved since 1.5.0

Isle's later releases are mostly in the GNOME extension, which is Keenu's Linux front end, so most of this ports
directly. The Windows and macOS island is Keenu's own drawing, so the same ideas need new code there.

| Isle change | Linux | Windows, macOS | Why |
|---|---|---|---|
| The closed island as wide as what it shows, the clock always centred, two activities as one small sign each side (1.5.1) | Port | Build | A fixed-width island looks empty with only the time, and crowded with a song and a timer |
| Same spacing on every page, one button size and one switch size, plain fills instead of outlined boxes (1.5.1) | Port | Build | Keenu's Settings rows added in phases 1 to 4 follow the old spacing; this is the moment to make them line up |
| Scroll or swipe over the tabs to change page (1.5.1) | Port | Build | Eight pages are slow to reach one click at a time |
| 12/24-hour choice, AM/PM beside the time (1.5.2) | Port | Build | Windows and macOS users expect the system's own format; today the island uses one |
| Sharp clipboard thumbnails at twice their pixels, small images shown at their own size (1.5.2) | Port | Check | Blurry on 200% displays, which most Mac screens and many Windows laptops are |
| The assistant's card always shown whole, with old steps giving way (1.5.2) | Port | Build | After many steps Keenu's approval card can be pushed down; nobody should have to scroll to find Deny |
| Auto-hide: always, with the top bar, or beside maximized windows (1.6.0, issues #19, #20, #22) | Port | Build | Asked for most in Isle's issues; on Windows the island sits over the title bars of maximized windows |
| Alerts over full-screen apps, off by default (1.6.0) | Port | Build | Keenu on Windows hides in full screen today, and misses a meeting reminder there |
| Notifications up to three lines, every one removable (1.6.0) | Port | Not there yet | |
| Black top bar beside windows (1.6.0, #26) | Port | Does not apply | GNOME only |
| Pill-shaped island, Large size, date beside the clock (1.7.0) | Port | Build later | Looks; lower priority than the rest |
| A shortcut for any page, Ctrl+Tab between pages, Ctrl+1 to 9 for a tab (1.7.0) | Port | Build | Tab already moves between controls; getting to a page still takes many presses; see section 3 |
| New clear glass with a thick edge (1.7.0) | Port | Build later | Windows 11 has Mica and Acrylic, macOS has vibrancy: use the system's own material, not a copy of Isle's shader |
| The island on every display (1.7.0) | Port | Build | Keenu shows on one display at a time, picked in Settings |
| Bluetooth battery alerts and a Devices page (1.7.0) | Port | Build later | Windows reports these through its Bluetooth APIs; macOS less reliably |
| Calls: only real call apps count (1.6.0) | Port | Not there yet | |
| opencode as an AI (1.6.0, 1.7.0) | Port for keenud | Add to routes | One more program people already sign in to themselves |
| Send files to and from a phone over the local network with a QR code and a 4-digit check (1.6.0, #28) | Port after a security review | Build after a security review | Useful, but it opens a port on the computer; Keenu's threat model needs a section for it first |
| Fixes: Super+V, shelf flicker, network mounts freezing the shelf, top bar moved to the bottom (1.7.0) | Port | Check the same cases | |

Isle's measured performance (no redraws or wakeups at rest) is a target for Windows and macOS as well, which
nobody has measured yet. Measuring comes before any of the new looks.

## 2. The assistant, made easier to follow

Phases 1 to 4 put the assistant on the existing Voice page and the existing approval card, so as not to change
the look. Isle shows what a dedicated design does better. Proposed:

1. **A face.** Isle's bot turns to the pointer, blinks, bounces with the voice and changes expression with what it
   is doing (listening, thinking, working, done, failed), with a glow round the island in its colour. Keenu's
   assistant is a line of text today. A small face makes the state readable at a glance from across the desk; it
   is optional and has a "plain" choice.
2. **What it heard, then the steps as a checklist.** Today the last three steps show as text. Proposed: the heard
   sentence on top, then each step with a tick when done or a cross when it failed, and the current one moving.
3. **Risk on the card, in plain words.** Isle labels a command card "Needs your password", "Runs a script from the
   web" or "Removes files or apps". Keenu's card shows the action and the app. Proposed labels for Keenu's own
   actions: "Sends something" (email, message), "Deletes", "Spends money", "Types into another app", "Can't be
   undone". They come from the same plain-code checks in `keenu-guard`, never from the AI.
4. **Key hints on the card.** "Deny (Esc)" and "Allow (Enter)" written on the buttons, as Isle does. Keenu already
   denies on Esc. Enter alone answers nothing today, and Allow is refused for its first moments so a card cannot
   be allowed before it is read. Proposed: Enter allows once that moment has passed, and its hint appears then.
5. **Allow for this task.** Every click and keystroke the assistant makes in another app asks first. For "fill in
   this form" that is a dozen cards. Proposed: a third button, "Allow for this task", that covers the same app and
   the same kind of action until the request ends, and never covers sending, deleting or paying. This is a
   security change, so it goes into the threat model first.
6. **The conversation.** Isle keeps the day's conversation, scrollable, with text that can be selected and copied,
   "+" for a new one and Undo for five seconds; it is deleted at midnight or after a quiet time the user picks.
   Keenu keeps the conversation only inside the running AI program and shows the last answer. Proposed: the same
   as Isle, stored encrypted like the memory, with the same deletion rules.
7. **Working in the background.** Isle lets the island close while the assistant works, with a small mic on the
   closed island and the answer as an alert. Keenu's worker carries on when the island closes, and a request
   for approval opens the card, but the closed island has no sign that it is working. Proposed: a small sign on
   the closed island while it works, and the answer as an alert.
8. **The voice level.** A light along the island's edge that rises with the voice while listening (Isle does
   this), so people can see the microphone hears them before they finish speaking.
9. **ChatGPT, labelled as OpenAI asks.** OpenAI's guidance for apps that use "Sign in with ChatGPT" asks for the
   button to read "Continue with ChatGPT", a first-use notice "You're using your ChatGPT plan", a "Using ChatGPT
   plan" mark while it answers, and a "Manage usage" link to the plan's usage settings. Phase 1 uses "Connect
   ChatGPT". These four are small and should come first, before anyone outside the project uses the branch.

## 3. Accessibility and first use

None of this is in Isle either; it matters more for Keenu because it runs on Windows and macOS, where people
expect system apps to work with a screen reader.

- **Screen readers.** The Windows and macOS island draws itself, so Narrator and VoiceOver see one unnamed
  window. Each button, row and field needs a name and role through UI Automation on Windows and the
  accessibility API on macOS: the same interfaces Keenu now uses to read other apps.
- **Keyboard only.** Tab and Shift+Tab already move between the open island's controls and Esc closes it. Still
  to do: Ctrl+Tab between pages (section 1), a focus ring that is easy to see on every page, and a check that
  every action added in phases 1 to 4 can be reached this way.
- **Text size** (Isle issue #9): follow the system's text scaling on Windows and macOS instead of fixed sizes.
- **Contrast.** Nobody has measured Keenu's grey secondary text against its black. Isle 1.7.0 measured its glass
  text at 9:1 or better. Measure Keenu's against 4.5:1 and fix what fails.
- **Reduced motion.** Follow the system setting: no spring or bounce when it is on.
- **First run.** One short sheet the first time the island opens: what it shows, that everything that records or
  connects is off, and where to turn things on. Today a new user finds the assistant only through Settings.
- **Errors that say what to do.** The assistant's messages already do ("Connect ChatGPT in Settings"); the rest of
  Settings should match, with a button that goes to the fix.

## 4. Features after phase 4

### Pages Windows and macOS lack

Isle's Linux pages that Keenu's own island has no equivalent for yet, in the order they are most useful:

1. **Timer** with work and break lengths that can be typed, its time on the closed island, and Do Not Disturb
   while focusing (Windows Focus, macOS Focus).
2. **Notifications** collected in a list with their buttons. Windows lets an app read notifications only with the
   user's permission (`UserNotificationListener`); macOS gives no public way to read other apps' notifications,
   so on macOS this page does not exist, and the docs should say so.
3. **Reminders** as their own page. They exist now (the assistant adds them), but only as alerts.
4. **Calls** with the green phone and call length, from the same media and microphone signals the privacy dots use.
5. **Apps**: time per app, off until turned on, the same as Usage on Linux.
6. **Devices**, after the Bluetooth alerts above.

### The assistant, the rest of the plan

What phases 1 to 4 did not reach, which belongs in the next round of work rather than in a look change:

- **Linux parity.** keenud's assistant still has only the AI programs, not the ChatGPT sign-in, an API, the
  memory or the briefing, which run on Windows and macOS. It also reads other apps only through its existing
  window commands, not through AT-SPI. Both are needed before Linux has the same assistant.
- **macOS listening.** macOS has spoken answers and the shortcut, but no speech recognition and no wake word;
  Apple's Speech framework does both on the device.
- **Talking over it.** Today speech stops when the user presses the shortcut or the microphone button. Stopping
  when the user starts talking needs echo cancellation, so the computer does not hear its own voice.
- **Real-time voice** (speech in and out without turning it into text in between) through OpenAI's or another
  provider's real-time API, for people who connect that provider.
- **whisper.cpp on Windows and macOS**, as on Linux, for people who want recognition that works offline and in
  more languages than Windows' built-in engine.
- **Gmail, Google Drive, Notion and Dropbox built in.** Keenu reaches them now only through an MCP server the user
  sets up. Built-in connectors need an OAuth client registered with each service under the project's name; that
  is the project owner's step, and Keenu does not invent client IDs.
- **Clipboard list and paste** (Isle issue #21): a vertical list under the pointer, with Enter pasting the chosen
  entry where the cursor is.
- **Shared sessions with the AI programs.** Claude Code, Codex and Grok each keep their own history; showing
  which project and conversation the assistant is in would make "continue that" work across days.

## 5. Order and size

These are estimates for one developer, before anyone has measured. They follow the plan's own rule: each step ends
with a review before the next starts.

| Order | Work | Estimate |
|---|---|---|
| 1 | ChatGPT labels and first-use notice (2.9); card always whole; key hints | 2 to 3 days |
| 2 | Measure performance and contrast on Windows and macOS; fix what fails | 1 week |
| 3 | Keyboard use, screen reader names, text size, reduced motion (section 3) | 2 to 3 weeks |
| 4 | Port Isle 1.5.1 to 1.7.1 to the GNOME extension (section 1, Linux column) | 2 to 4 weeks |
| 5 | Assistant: checklist of steps, risk labels, background sign, conversation (2.2, 2.3, 2.6, 2.7) | 3 to 4 weeks |
| 6 | "Allow for this task", after its threat-model section (2.5) | 1 to 2 weeks |
| 7 | Island width, auto-hide, every display, alerts over full screen on Windows and macOS | 3 to 4 weeks |
| 8 | Timer, Reminders, Notifications (Windows) pages | 3 to 4 weeks |
| 9 | Linux assistant parity; macOS listening | 1 to 2 months |
| 10 | The face, glass, pill and Large looks | 2 to 3 weeks |
| 11 | Built-in connectors, once OAuth clients exist | 2 to 4 weeks each |

## 6. What not to do

- Do not read AI sign-in tokens to show plan usage, as Isle's AI usage page does.
- Do not copy Isle's look onto Windows and macOS pixel for pixel: use each system's materials, fonts and controls
  where they exist, and keep the island's shape and behaviour the same across systems.
- Do not let "Allow for this task" or any other shortcut skip the card for sending, deleting or paying.
- Do not open a network port (phone sharing) without a written threat model, a visible sign while it is open and
  an automatic stop.
- Do not claim any of this works on a system until it has been tried there by a person, not only in CI.
