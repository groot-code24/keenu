# Keenu product spec

The single source of truth for how the island looks and behaves. Code that
disagrees with this document is a bug in one or the other; fix both together.

## 1. Who it is for and what they need

| As a user I want to… | So Keenu must… |
|---|---|
| see the time and what is running at a glance | show the clock in the closed island, with live activities beside it (timer ring, music) |
| control music without switching windows | open on hover with cover, scrubber, controls; sing along with synced lyrics |
| keep an eye on a call while I work in another window | show calls from any app or browser: their length in the closed island, a live preview of the call, mute and a way back |
| focus in blocks | run a Pomodoro timer that stays visible while closed and chimes at the end |
| know where my time goes | time the app in front, if I turn it on, and show it today, over the last seven days or the last 30 |
| not forget small tasks | write them down in one line, tick them off; tasks survive reboots and power cuts |
| move files between apps | park files on the island and drag them out later |
| know when something changes | show short, consistent alerts: charging, battery, losing the connection, earbuds, volume, brightness, Caps Lock |
| still get my notifications now that the clock is gone | show GNOME's notifications in the island, keep them in a list, and offer Do Not Disturb right there |
| know when I am being watched or recorded | show privacy dots for the camera, microphone and screen sharing, and a timer while the screen records |
| control sound without opening Quick Settings | change the volume and the output device from the music page |
| keep my laptop fast and cool | never poll; animate only while something moves |

Out of scope for v1: water reminders (removed), notification banners in the
island, and desktops other than GNOME.

## 2. Presentations

The island is always in exactly one presentation. `P` is the top bar height:
26 px with the compact top bar (the default), GNOME's 32 px otherwise. All
sizes are logical pixels.

| Presentation | Size (w × h) | Corner | Shoulder | Content |
|---|---|---|---|---|
| Compact | 144 × P; 232 with an activity | 10 | 7 | Clock; activity slots left/right |
| Compact, hanging | 164 × (P + 30); 256 with an activity | 16 | 10 | As above |
| Peek (alert) | fits its content, up to 380 wide × (P + 56); 56 high off a black bar; 12 px round the badge on every side | 28 | 13 | Badge · title/subtitle (a notification's words on up to two lines) · value, level bar or button, centred |
| Peek with buttons | fits its words up to 440, its buttons whole up to 560; buttons that don't fit go on to a line below; 16 px round one lit along its edges | 28 | 13 | Badge · title/subtitle · code · small buttons under the words, from the left |
| Expanded | 600 × (P + 164) on every page; wider when its tabs need room; 720 for lyrics | 32 | 22 | Header row (tabs) · page |

Every width is even, so the island sits centred on its display to the pixel.

The top bar is slimmed to a thin bezel (status icons and the tray stay in
it) and see-through like glass, with no colour of its own. The
island is a black notch hanging from the top edge of the screen with concave
shoulders. Settings → Appearance → Island can make it **Glass** instead: what is
behind it (windows and wallpaper) blurred (GNOME Shell's blur, radius 30) and
dimmed under an 80% layer of rgba(19, 19, 21), as GNOME's menus measure with
Blur my Shell, with a 1 px white edge at 10%; cut to the island's exact shape,
shoulders and corners included, so nothing shows outside it. On glass,
buttons are white at 8–16% instead of grey, an answer (Allow, Run, Join) and
a toggle that is on take GNOME's accent colour (the user's, from GNOME 47),
an OK stays white, and the assistant's and the alerts' light keeps to the
edge, each colour lit by its own lightness, so it never burns to white. Closed, it sits inside the bar
(32 px, against 26 px when it hangs below instead): maximised
windows start below the bar, so the resting island never covers an app and
no screen space is lost to it. Alerts and the open island reach below the
bar; they cover windows only for as long as they are shown. Their content
sits below the bar's height, and the open island's header row level with
the bar. In the settings the closed island can hang below the bar instead
("Compact, hanging").

Type is set in Inter (installed with Keenu), semibold for titles, nothing heavier than bold, and slightly tighter
tracking on large figures. The island's text fields keep their own look
when Blur my Shell restyles the shell's.

A slider takes the bar from glass to black; the island stays black. All
the way to black, bar and island read as one shape: the shoulders then curve
out of the bar's lower edge instead of the screen's top. The installer sets glass (and the slim
bar) on every install, turns off Blur my Shell's top bar blur (it darkens
the bar), and turns off
extensions that take the island's place. Keenu adds its colour to the bar's
own style and puts it back if another extension replaces that style,
leaving the other extension's rules in place.

### State details

Every state change shows in the icon, not only in colour, so it reads at a
glance (and for colour-blind users):

| Control | Off | On |
|---|---|---|
| Do Not Disturb | bell outline, grey chip | filled moon, indigo chip; moon in the closed island |
| Focus timer | ▶ Start pill | ⏸ Pause while it runs, ▶ Resume when paused; a round reset beside it once started |
| Lyrics | speech bubble outline | filled bubble, lit button |
| Completed tasks | arrow pointing right, folded | arrow pointing down, listed |
| Play / pause | play | pause |
| Task | empty circle | white circle with a black check; title struck through |
| Mute (call) | microphone, grey round button, "Mute" | crossed-out microphone, white button, "Muted"; a red crossed-out microphone in the closed island |

Controls that cannot act right now (Clear All with nothing to clear) stay in
place, dimmed, so the layout never jumps.

### Motion

- Size uses one spring for growing and shrinking: stiffness 155, damping 18
  (≈3.5% overshoot, peak at 0.37 s, settled by 0.57 s), close to
  `cubic-bezier(.34, 1.32, .5, 1)` over 0.62 s.
- Corner radius and shoulders use the same speed without overshoot, and are
  clamped every frame so corners never exceed the visible body: the outline
  stays clean at every size in between.
- Content cross-fades: outgoing 160 ms, incoming 320 ms after a 200 ms delay
  (the shape makes room first). No scaling.
- Pages slide 48 px with a fade, 260 ms ease-out.
- Rings in alerts sweep from empty to their value over 1 s, starting 350 ms in.
- With "reduce animations" on, everything jumps to its end state.

## 3. Interaction flow

| Input | Closed / alert | Open |
|---|---|---|
| Pointer rests 120 ms (Settings: 60–1000 ms, or click only) | Open on the default page | – |
| Pointer leaves | – | Close after 300 ms (not while typing or dragging a file) |
| Click | Open now (an alert opens on its page) | Normal button clicks |
| Middle-click | Play / pause | Play / pause |
| Right-click | On an alert: put it away; else bring the call, or the music player, to the front | Bring the music player to the front |
| Pointer on an alert | Holds it; it goes 1.5 s after the pointer leaves (one asking, a reminder or an event keeps its own time). One the pointer doesn't open (with a button) and an answer show a × on their icon to put them away | – |
| Two-finger swipe up / down on an alert | Put it away / open it | – |
| Super+V (GNOME's calendar shortcut) | Open the clipboard history, search ready, held open (Settings may leave it to GNOME's calendar) | Close |
| Super+I (Settings: any shortcut) | Ask the assistant; typing while it listens types the request instead | Speak again |
| Click a tab | – | Go to that page |
| Scroll, or swipe with two fingers | – | Lists and lyrics scroll; pages change only by their tabs |
| ← → Enter (search field) | – | Choose a clipboard card; Enter pastes it into the window in front |
| Esc | – | Clear the search, stop typing or renaming; then close when opened with Super+V |

**Default page on open**, first match wins: the page of the alert being
shown → the page picked by hand (a tab, or the Shelf after a file
was dragged onto the island), remembered for 5 minutes and forgotten when a
call starts → Call during a call → Notifications if some arrived unseen
(during Do Not Disturb) → Music if something plays → Focus if the timer
runs → Music.

Pages, in order: **Call** (only during a call) **· Music · Notifications ·
Focus · Reminders · Shelf · Calendar · Apps · Usage** (Apps only with its switch
on, Usage only with an AI plan found), one tab each in the header. Lists scroll
with the wheel or touchpad and never show a scrollbar.

## 4. Closed island

- Where it is: on the main display, in its top bar; or (Settings) on the display of the window in use,
  moving there once closed. A display without the top bar has the island hang from its top edge, with
  a header of its own when open, and GNOME's clock comes back to the main top bar meanwhile.
- Right slot, first that applies: a warning (Keenu's service stopped; a click offers to restart it) →
  the bot, 18 px, in the look chosen, blinking and glancing every few seconds (the assistant works on
  a request or a command runs with the island closed; a click shows it) →
  muted microphone on a call → a second activity → music bars → up to two privacy dots.
- Two activities at once: the first of recording, call, music and timer keeps the left slot and the
  centre; the next takes the right slot: the timer's time left (white while focusing,
  green on a break; beside the music bars while a song is first), a call's green phone and length, or
  the music bars. A second time is as large as the clock and on its line, with no ring. The island widens
  by as much as that is wider than a slot, on both sides, so the clock stays centred. Each side's sign
  keeps to its end of the island, all as far in.
- Left slot, first that applies: breathing red dot (screen recording) →
  green phone (a call) → focus timer ring → cover art (a song playing or paused) → white dot (unseen
  notifications).
- Do Not Disturb: a small moon before the clock (or the call's or recording's length), whatever the sides hold.
- While the screen records, the centre shows how long it has been recording
  instead of the clock; during a call (and no recording), how long the call
  has lasted, in green.
- Right slot: a red crossed-out microphone while a call is muted, else music
  bars while a song is loaded. They dance only while it
  plays and the closed island is on screen, and rest after a minute without
  input. Paused, they rest and dim, and the cover stays until the player
  closes.
- Far right: privacy dots: green camera, orange microphone,
  blue screen sharing (the island widens by 22 px while any is shown).
- Centre: the clock (follows GNOME's 12/24-hour setting).

## 5. Pages

Every page shares the header row inside the top bar. On
the left, a tab per page: icons, the open page's in a lit pill with its
name, and a red dot on Notifications while some are unseen. On the right a
red "● 0:42" pill while the screen records (click to stop), a green "📞 1:15"
pill during a call on any page but Call (click: the Call page), the privacy
dots, the music bars, the assistant's sparkle, `[timer ring] Tue 20 Oct 12:26` and the gear. The
tabs' icons, the gear and the pages' contents share the same 28 px edges. When the tabs and the
status together need more than the page's width, the island grows to fit them, by the same amount on
both sides so it stays centred. The Call tab, its phone in green, only shows during a call.

**Call**, with round controls. A call is an app holding the
microphone for over 2 s (sites open it briefly to ask for permission), in
any app or browser: WhatsApp, Telegram, Meet, Zoom, Discord… Level meters
and recorders (OBS, Audacity, Sound Recorder…) do not count. The app is
found from the microphone stream ("Google Chrome", "firefox", an app id),
then its window: one titled after a call service first ("Meet - …",
"WhatsApp"), else the app's latest. The call ends 4 s after the microphone
closes, so an app reopening it (switching devices) keeps the same call.

On the left, a live preview of the call's window (rounded 14 px, 128 px
high, as wide as the window's shape allows between 168 and 228 px, cropped
to fill): you and whoever you call, still moving while the window is
covered, minimised or on another workspace. GNOME Shell draws it from the
window's picture already on the graphics card (a `Clutter.Clone`): nothing
is captured, and it only exists while this page is open. Under the pointer
an ↗ shows; click it to go to the call. It shows the app's icon instead
while the preview is off (Settings → Calls) or no window is known, and
"Hidden while sharing" while the screen is shared. On the right: the app's
icon and the service or app name ("Google Meet", "WhatsApp"), "Voice call"
or "Video call" (while the camera is on) with the length in green, and two
round buttons with captions: **Mute** (GNOME's default microphone, for
every app; white and "Muted" while on) and **Open** (brings the call's
window to the front, switching workspace if needed, and closes the island).
When the call ends the tab goes and an open island moves to the next page.

**Music**, laid out as a now-playing card. On the left a 104 px cover
(click: bring the player forward) with the playing app's icon on its corner
(Spotify, Chrome, …; found by name when the player gives no desktop entry).
Beside it: title and artist — album (or the app's name) with the lyrics and
output buttons small at the right, the scrubber with elapsed / remaining
time, and previous · play/pause · next centred under them. An app's logo
sent in place of a cover is shown whole, without the badge. Built for Spotify and YouTube in
the browser; any MPRIS player works. With nothing playing: "Nothing
playing" and "Play something in Spotify or on YouTube".

**Volume and output** (Music page, the output button at the bottom right).
The volume with a mute button and percentage, then the outputs (speakers,
headphones, earbuds, HDMI) with a check on the current one; click one to
switch. The output button lights up and fills in while this is shown; it
returns to the player when the island closes.

**Lyrics** (Music page, toggled). The island widens to
720, its contents gliding to their new place. Left, in a column: a 64 px cover, title, artist and the music bars
(dancing while it plays), the scrubber, then the lyrics toggle (lit),
previous · play/pause · next and the output button. Right: synced lines;
the one being sung is white and full size, the others a little smaller,
those to come dimmed by distance and those sung dimmest; the view glides to
keep the current line a third of the way down, its edges fading. Pauses in
the singing (and a long intro) show as three breathing dots. Click a line
to jump there; scrolling by hand pauses following for 4 s. The island takes
a wash of the cover's main colour (by hue, so a two-colour cover gives one
of them, not the grey between). States, centred with an icon: "Looking for
lyrics" (breathing dots), "No lyrics for this song", "Instrumental", "Can't
reach the lyrics", "Lyrics are busy right now".

Between songs the island stays steady: when a song that was playing gives
way to nothing (browsers report no title and "Paused" for a moment between
videos), the song is shown playing for up to 1.5 s, so the cover, bars and
width do not drop out and back; a song paused by itself shows at once.

**Notifications**, as cards: a heading "Notifications" with
**Do Not Disturb** (a bell, or a moon on purple while on) and a **Clear**
pill on its right. Below, newest first, a card each, 6 px apart: the app's
icon (a symbolic one on a grey tile), the title in bold and its text in
grey on one line, how long ago on the right, and its buttons ("Reply",
"Mark as read") under them, whole, going on to another line when they
don't fit; under the pointer the card lights and × takes the time's place.
Cards past the edge fade out as the list scrolls. Click a card to open
it (the island closes). Opening the page marks them seen. Empty: a bell,
"No notifications · You're all caught up".

**Focus**: a 104 px ring on the left that fills clockwise as the phase
runs (white while focusing, green on a break), the time left inside and the
phase under it ("Focus", "Short break", "Long break"). In the middle two
stepper pills, **Work** and **Break**, each with its minutes and − / +
(Work 1–180, Break 1–60; each click counts, however fast). On the right a
**Start** pill (Pause while it runs, Resume when paused) with a round reset
beside it once the timer has started; the controls keep a fixed width, so
nothing moves as they change. No glows.

While a focus session runs (paused part way included), Do Not Disturb is on,
like a Focus mode; it goes off when the phase ends. A focus session waiting
for Start after a break doesn't count. Keenu only undoes what it did: if Do Not
Disturb was already on, it is left alone, and turned off mid-session it stays
off until that session ends, past pauses and the lock screen. It stays on
across the lock screen, and is turned off on the next start if the shell
stopped mid-session. ("Do Not Disturb" in Focus
Timer settings.)

**Shelf** (title "Shelf"): things to keep close, in two tabs on a
segmented control (switching crossfades).

- **Clipboard**: what was copied, pinned items first, then newest first, as
  cards (136 × 96): text
  (three lines; code in a monospace font), links (site and address), images
  (thumbnail with its size) and files (icon, name, "+2 more"). Each card shows
  the app it came from and how long ago. Click a card to copy it ("Copied"
  flashes on it). On hover: pin (pinned items stay at the front and are
  never cleared), a hand-off (text to Reminders; images and files to the Files
  tab) and ×. The bar holds type filters (text, links,
  images, files; the chosen one lit, the others dimmed), search (a round
  button that opens into a field, closing again when left empty) and
  Clear all (asks once more; pinned items are kept). Opened with Super+V, the search
  field has the keyboard: ← → choose a card (white outline), Enter pastes it
  into the window in front (Ctrl+Shift+V in terminals).
- **Files**: at most 6 files, all in view with no scrolling; a seventh is
  refused until one is removed. Each file is a tile of one size: a 52 px
  square preview, as Files shows the file (a picture cut to a square; a
  document's thumbnail, such as a PDF's first page, whole, made as Files makes
  it when there is none yet; else the theme's icon) with a
  round × always on its corner, and the name below, shortened in the
  middle. Click a tile to copy the file ("copied" shows on it): pictures
  as the image itself, so chat apps and AI chats paste an image, other
  files as a file; double-click to open it. The bar says what the tab is
  for ("No files on the shelf · Drop files here, then drag them into any app", in the island's
  words and the helper's alike), how full it is ("3
  of 5 · click to copy, or drag into any app"), or "Full · remove a file to
  add another". With `keenu-shelf` installed, the helper draws this tab in a
  see-through window placed exactly over it, tile for tile as the island
  draws it: drag a file out to any app (a chat, an email, a folder).
  Dragging copies; a drag that ends nowhere leaves the file on the shelf,
  and the island stays open until the drag ends. Dragging a file near the
  island (within 120 px) opens this tab by itself; the helper is started as
  the drag begins and shown as soon as the growing island covers the tab,
  so it takes the drop at once. While the file is on its way the tab lights
  up as one rounded area with an empty "Drop here" slot at the front, and
  the bar says "Drop to keep it on the shelf" (or "Full · remove a file
  first", the area edged red, and the drop is refused); a drag that goes
  off elsewhere closes the island again. When a file lands, light in its
  colour (its picture's main colour, or its type's) rises from the island's
  two bottom corners and fades after about 2 s, and the bar says "Added".
  Without the helper the island's own tiles do all but take drops, with an
  install hint when empty.

Clipboard history is event driven (GNOME tells the shell when the
clipboard changes; nothing polls) and kept by `keenud`: the 20 most recent
items plus up to 10 pinned ones ("Pins full · unpin one" past that), images
with a 320 × 200 thumbnail. The cards are only
drawn while the page is open, so copying costs nothing on screen. Copies are never kept
when a password manager marks them secret, from ignored apps (password
managers by default; Settings → Clipboard), or while history is off.

**Reminders** (a page of its own, the island's full width), in dark mode:
an orange "Reminders" with the number still open on the right
and a +; under it the reminders as rows of a thin ring and the title over a
hairline that runs from the text to the edge (a long title wraps over as
many lines as it needs, the ring by its first line), scrolling past what
fits, and a
last faint row "New Reminder" (Enter adds and stays ready, Esc stops; + puts
the cursor there). Clicking the title renames it in place (Enter or clicking
away saves, Esc cancels); × appears on hover. Ticking one fills its ring
with an orange dot and strikes the title through at once; it lingers 0.9 s
(a second tap undoes it), then fades and folds away, the rows below sliding
up. Closing the island in that moment keeps it done. Empty: "No Reminders",
or "All Done" once today's are ticked off. Done ones are cleared at
midnight.

**Calendar** (title "Calendar"), as a calendar widget. Left: the
month in red capitals with the year (click: back to today) and ‹ › to move
between months; the week's day letters (from the locale's first weekday,
weekends dimmer); the days, today in a red circle, the chosen day in a grey
one, a dot under days with events. Right: the chosen day ("Today, 20
October"; "Tomorrow", "Yesterday", or its weekday), a **Today** pill when
away from it, a field to add an event in one line (the hint says "e.g.
Dinner 8:00 pm"; also "Standup 9:00-9:15", "Gym at 7pm", "Dinner 1930" (four
digits last, not a year such as 2015); without a time it is all day), then the
day's events as grey rows with a dot of their calendar's colour: title, and
"All day" or the time on the right ("9:00 – 10:00 AM" on a 12-hour clock). As
a line is typed, the field shows, grey, before the chip, when it takes place as
it will be read ("8:00 PM", "All day"; nothing for a time alone). Finished events recede. Enter clears the
field at once and shows the event, faint, until its calendar lists it (or 30
seconds at most), so a slow server never invites a second Enter. With the
account's calendar out of reach (offline, its server down) the event is kept in
Keenu with a ring of that calendar's colour for its dot, the field says
"Offline · syncs once online" a moment, and `keenud` sends it there once in
reach (then the ring gives way to the calendar's own event). Events come
from the desktop's calendars (Evolution, and accounts added in Settings →
Online Accounts such as Google; in each calendar's own colour; to read, not to
click) and Keenu's own (orange; × on hover to remove), kept by `keenud` and
forgotten after 400 days. New events go to an account's calendar: a chip in
the add field (its colour, its name, ⌄) shows which; clicking it lists every
calendar that takes events (not a subscribed one, such as holidays) in the
field's place (the header says "New events go
to", with Done), a row each with its account in grey, then Keenu on this
computer, a tick on the one in use. Until one is picked, the first
account's calendar; with no account, Keenu's own. The assistant adds to the
same one. When the account can't be reached the event is kept in Keenu and
the field says "Saved in Keenu instead" a moment.

**Apps** (title "App Usage"): where the time went. Off until its switch in
Settings → Pages is on; off, nothing is timed and the page has no tab. On the
left the title, then how long the period's apps were in front ("2h 34m
today"); on the right a segmented **Today** / **7 days** / **30 days**
switch. Below, one row per app, most used first: its icon (a generic one for
an app without a desktop file), name, a thin white bar against the most used
app, and its time ("1h 12m", or "<1m" under a minute). Empty: "No usage yet
· Time in the app in front shows up here". Only the application in front is
timed, whether or not there is keyboard or pointer input; it stops when focus
moves to another application, when no window is in front, when the screen
locks, and after five minutes without input, unless a video or another
program holds the session awake; the next input goes on timing. The
extension keeps the stretches and sends them to `keenud` at most once a
minute, as the page opens, and when timing stops; nothing is asked while
the page is closed. A stretch past local midnight counts for each day it
spans. `keenud` keeps 30 days in `~/.local/share/keenu/app-usage.json`, and
only GNOME Shell may report, read or clear them. Settings → App Usage
clears them.

## 6. Alerts

One at a time. *Instant* alerts replace whatever is showing; the rest queue
(the same one updated replaces itself in place) and wait while the island is
open or hidden by a fullscreen window. Volume, brightness and Caps Lock are
dropped instead of waiting, since they are stale by then. More than three
notifications waiting come as one ("6 new notifications"); a notification
only counts as seen once it was on screen with someone at the computer.
Reminders and critical notifications are *sticky*: their time only runs while
someone is at the computer; any notification waits for them so too, as GNOME's
banner does. Right-clicking an alert puts it away without opening it.

| Alert | Badge | Title / subtitle | Right | Time |
|---|---|---|---|---|
| Charging | green ring + bolt | Charging / Charger connected | 76% | 2.6 s |
| Unplugged | ring + battery | On Battery / Charger disconnected | 76% | 2.6 s |
| Low battery (20, 10, 5%) | red ring | Low Battery / Plug in your charger soon | 10% | 8 s |
| Fully charged | green ring | Fully Charged | 100% | 2.6 s |
| Earbuds connected | ring + device icon | AirBuds Pro / Connected | 70% | 2.6 s |
| Connection lost | Wi-Fi offline icon | Offline / The network connection was lost | – | 2.6 s; taken back once online again |
| Volume, brightness (instant) | icon | Volume / device | level bar + %, gliding to each new level; pressing on at 0% or 100% stretches the island like a rubber band | 1.5 s |
| Caps Lock (instant) | ⇪ | Caps Lock on / off | – | 1.5 s |
| New song (off by default) | cover | title / artist | – | 2.6 s |
| Timer phase ends (+ chime) | timer | Focus session done… | Start break | 8 s |
| Screen recording started | red record icon | Recording screen | Stop | 2.6 s |
| Notification (GNOME's sound) | app icon, large | title / first line (only who it is from while the screen is shared) | its first button ("Mark as read") | 4 s; passing (a screenshot taken) 2.5 s; critical 20 s, instant and sticky |
| Reminder (+ chime) | bell | the reminder | – | 8 s, sticky |
| Assistant done in the background (resting the pointer holds it; it opens no page) | mic | the answer / Keenu Assistant (· Click to read more, when cut) | – | as long as it takes to read, at most 8 s |
| Keenu's service stopped | warning | Keenu's service isn't running | Restart | 8 s |
| Newer Keenu (once per version) | update | Keenu 0.x is out | How | 8 s |
| AI plan nearly used up | the plan's logo in a ring of its colour (Claude coral, ChatGPT green; red when nearly gone) | Claude / Weekly limit · resets Sat 09:30 | 86% | 8 s |
| Event about to start (Settings: 1–60 min before, 5 by default) | video camera when it has a meeting link, else calendar | Weekly sync in 5 min / 17:30 – 18:00 · Google Meet | Join (blue), with a Meet, Zoom, Teams or Webex link | sticky |
| Claude Code needs the user (off by default) | Claude's logo | Claude needs your OK / project · Allow Bash? (or "is waiting for you") | Show (its window) | sticky |
| Claude Code or Codex is done (off by default) | its logo | Codex is done / project · the gist of its last message | Show | 8 s |
| An AI's alerts could not be turned on | its logo | Couldn't turn on Codex alerts / why | – | 8 s |

Badges are grey unless the colour says something (green charging, red low
battery or failure, the plan's own colour, blue for an event); nothing amber.
Join is the one blue button, marking the action to take. Buttons on alerts,
notifications and the assistant are translucent grey pills with white text,
lighter under the pointer.

While the event alert is on, GNOME's own calendar reminder (Evolution's
`org.gnome.Evolution-alarm-notify`, which opens a window of its own when
clicked) of an event Keenu alerts of is taken away as it comes: one reminder
per event, Keenu's. It is matched by its calendar and event ids, for an event
with a time from 15 minutes gone to 30 minutes before Keenu's own alert.
Reminders Keenu doesn't give stay GNOME's: an all-day event, a task, one an
hour or a day ahead, a calendar GNOME doesn't show, and any while Do Not
Disturb is on (Keenu alerts of nothing then).

When a window is fullscreen the island hides, alerts wait (see above) and
GNOME's own volume pop-up is used. Charging, network, Bluetooth and Caps
Lock alerts each have their own switch, and wait out Do Not Disturb, except
a low battery. On a display other than the island's, GNOME's own volume
pop-up shows there.

**Events.** The reminder before an event comes from the same calendars as
the Calendar page (GNOME Shell's, which reads Evolution Data Server), opened
or not: Keenu asks for this month and tomorrow, and plans one alarm for the
next event with a time (all-day ones never alert). Its meeting link is read
from the event itself, through Evolution Data Server, on this computer:
Google's conference data first, then the URL, location and description. An
alarm the computer slept through is dropped once the event has started.

**AI agents.** Claude Code and Codex are connected as soon as Keenu finds
them (installed later too), once announced by an alert; Settings → AI Usage
→ AI Agents turns each off. Connecting adds Keenu's hooks to the agent's own
settings: `PermissionRequest` (timeout 600 s), `Notification` and `Stop` in
Claude Code's `~/.claude/settings.json` (or `$CLAUDE_CONFIG_DIR`, as
keenud's service sees it); `PermissionRequest` and `Stop` in Codex's
`~/.codex/hooks.json` (an older Codex: `notify` in its `config.toml`). The
user's own hooks, keys, comments and order are kept; turning the switch
off, or uninstalling, takes out Keenu's entries only.

A permission comes as an alert that stays until answered: the bot (46 px)
with an amber "!" badge, "‹project› needs permission", the agent's name,
the command or file in a code line, and Deny, Allow and (Claude Code)
Always, the last a white pill. Pressed, the badge turns into a green tick
and the bot hops ("Allowed", "Always allowed here"), or a red cross and a
shake ("Denied"), and the alert goes 0.85 s later. Hovering it never opens
the island. Put away unanswered, with the agent's window in front (then or
later), during Do Not Disturb, or with the island hidden, the agent asks in
its own terminal instead; answered there first, the alert goes. A finished
agent's alert has the bot with a green tick, the first sentence of what it
said, and Open terminal.

### Voice assistant

Super+I opens a pill below the top bar (280 px wide while it listens, 600 px,
as wide as the open island, once there is more to show): the bot on the left (36 px), the text beside
it, one round 32 px button at the end. The bot takes the shape (circle,
pebble, squircle, capsule, cloud, hexagon, drop, ghost) and colour chosen
in Settings → Assistant → Look, shaded with a soft highlight, a darker rim
and a glint. Its eyes have lids: they follow the pointer on a spring, glance
about and blink; they perk up and bounce with the voice, read along as you
type and as the answer shows, look up while it thinks, narrow while a
command runs, smile at an answer and droop on an error; it nods at Run and
shakes at Deny. Light rises from the panel's edge, brightest along the
bottom and gone by the top of the sides, in the bot's glow colours (or the
classic violet, blue and cyan), two highlights travelling round the rim; it
follows the voice, each key typed, and breathes while it works. No blobs of
colour.

| State | Text | End button |
|---|---|---|
| Listening | "How can I help?" (a follow-up: what it asked) | keyboard (type instead) |
| Typing | a pill field, Send inside it (dim until there is text) | – |
| Working | what was heard, the step now, done steps ticked below | Stop (a white square) |
| Answer | what was heard, the answer | keyboard (follow up) |
| Error | what went wrong, what to do, and Sign in or Set up when that fixes it | keyboard |

After 8 s on one step, "Still working · 0:12" shows under it. While the AI
writes its answer, the answer shows as it comes.

**The command card.** A command the AI wants run shows in the pill as a
small terminal: its purpose (bold) and a line on it, then `$ command` in
monospace with the program bold and `sudo` amber, then, left, the one thing
to know (Removes files or apps, red; Runs a script from the web, amber;
Needs your password, amber) and, right, Deny `esc` and Run `↩`. Running:
"Running · 1:12", the last two lines it printed, a light sweep under them,
and Stop (for a package manager, pressed once it says "May leave it half
installed" and turns into Stop anyway for 4 s). Ended, the card is one
line: the command and ✓ with how long, ✕ with the exit code, Denied or
Stopped. With the island closed, a waiting card comes as an alert with Deny
and Run.

**The conversation.** Scrolling up on the pill, or a click on the line above
the field, grows it (up to 420 px) into the conversation: "Conversation"
with + and a chevron to close; a time between messages more than 5 minutes
apart; your messages as right-aligned bubbles; answers on the left after a
small bot; commands that ran as one-line terminals with how they went. The
field stays at the bottom, with the mic in it until there are words, then
Send. Text can be selected and is copied as the selection settles; a click
on a command copies it. + shows "Started a new chat · Undo" for 5 s.

Closed while it works, the request and its command carry on: the bot shows
in the closed island and the answer comes as an alert. Resting the pointer on
it shows the work without taking the keyboard, and it goes back when the
pointer leaves; a click keeps it open, Esc putting it back.

### Notifications and Do Not Disturb

The island replaces GNOME's banners and its calendar drop-down while the
extension runs, so nothing shows twice (hovering opens Notifications; Super+V
opens the clipboard history instead of the drop-down). Rules
match GNOME's: low-urgency and already-seen notifications never pop up, and
apps whose banners are turned off in Settings stay quiet. Do Not Disturb is
GNOME's own setting, so it stays in sync with Quick Settings; while it is on,
only critical notifications pop up, and a line under the heading says so. Clicking a notification alert opens it;
hovering opens the Notifications page.

## 7. Data and reliability

- Reminders, shelf, clipboard history (images in `clips/`) and timer settings
  live in `~/.local/share/keenu/*.json`,
  written atomically and flushed to disk (file and folder), so they survive
  reboots and power cuts. A corrupt file is set aside, never lost silently.
- Lyrics and cover art are cached in `~/.cache/keenu`.
- `keenud` restarts on failure (systemd); the extension reconnects on its own.

## 8. Performance budget

- Idle, nothing playing: one wake-up per minute (the clock, GNOME's own WallClock, which also catches
  waking from suspend and time-zone changes). No polling.
- `keenud` sleeps until its next real deadline (a timer phase, a reminder, midnight) and re-checks the
  clock on waking from suspend; AI usage asks nothing while it is off, nor Claude while the screen is
  locked.
- Speech recognition runs only while a request is heard. The assistant's AI starts as the assistant
  opens and quits 3 minutes after the last request; both run at a lower priority, in a process group
  of their own that is killed whole; transcribing uses at most half the cores. A command the user ran
  is followed by reading its log 2.5 times a second, only while it runs. An agent's hook waiting for
  an answer sleeps on D-Bus. The files tray's helper quits 20 seconds after the tray closes.
- The bot in the closed island is drawn once and moves only while the assistant works.
- Animations run on the display's frame clock only while a spring moves.
- The open island ticks only the visible page (scrubber 4×/s, timer 1×/s).
- Calls are found from the shell's own audio events, never polled. The
  call's length ticks once a second only while it is on screen; the call
  preview reuses the window's picture and exists only while the Call page
  is open. Keenu never opens the camera or the microphone itself.
- `keenud` sleeps until a D-Bus signal arrives. The camera is watched with
  inotify, so the kernel reports opens and closes; nothing is polled.
- Moving parts that loop (music bars, the recording dot) run only while on
  screen; the music bars also rest after a minute without input.
