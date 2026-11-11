# Plan: Keenu as a full personal assistant and agent

**Status: a proposal. None of this is built, except where "Keenu today" says so.** Written 2026-10-31.

**Update, 2026-11-11:** a first version of phases 0 to 4 is on the `assistant-phases` branch, not released. It
is a prototype, built and tested by CI with fake AI services, not finished phases: no real account, microphone or
Mac user has tried it, and the "finished when" tests in [section 9](#9-build-order) have not been run. What it
covers on each system is in [what works where](platform-support.md#the-assistant-branch-assistant-phases-not-released).
Left out so far: Linux's ChatGPT and API routes, memory and briefing, and reading other apps' controls on Linux;
listening on macOS; stopping speech by talking over it; real-time voice; built-in Gmail, Drive, Notion and Dropbox
connectors (they need OAuth clients registered under the project's name). Memory, the schedule and connectors are
modules of `keenu-assistant`, not crates of their own as section 7 proposed. What would change after phase 4 is in
[after-phase-4-improvements.md](after-phase-4-improvements.md).

The goal is for Keenu to do what Jarvis, Grok and Muse do, on the user's own computer, with the AI account they already
have. This plan lists every feature found in those products, how Keenu would get each one, and the order to build them.

## 1. Summary

- **Most of it can be built.** The Linux edition already has the core: an agent loop, four AI sign-ins, an approval card
  and speech recognition on the computer. The first job is to bring that to Windows and macOS.
- **"Every feature" has limits.** A few features belong to those companies and cannot be copied: Meta's model, the inside
  of Instagram, X's own systems. Some cost real money to run, and a few should not be copied at all. Section 3 sorts each one.
- **The biggest decision is cloud.** Muse and Grok's agent reportedly run on a computer in the cloud that works while yours
  is off. Keenu can do that only with servers you pay for, and it cannot use people's Claude subscriptions there. This
  plan keeps Keenu on the user's own computer and leaves cloud as an optional last phase.
- **The design stays as it is.** No new theme, colours or fonts. Three features cannot fit that rule and need a decision
  from you: the phone app, video avatars and animated companions.

Build phases 0 to 3 first. They give a working assistant on all three systems, and show whether the rest is worth building.

## 2. What the products are

| Product | What it is | How sure this is |
|---|---|---|
| **Jarvis** | The AI from the Iron Man films. A list of behaviours to aim for, not a real product | Fiction |
| **Grok** | xAI's assistant: voice, research, images and video, scheduled tasks, and `@grok` replies on X | xAI's own site refused automated reading (403). Features come from its API docs, its Google Play listing and secondary articles. **Check them before building** |
| **Grok Bot** | Reported as an always-on agent on a cloud computer | Secondary sources only, with conflicting dates |
| **Muse** | Meta's personal agent, launched 2026-09-08, with its own computer in the cloud | Meta's announcement, except where marked |

Each feature below is marked by source: **M** Meta's announcement, **X** xAI's docs, **S** secondary sources only, **F** fiction.

"Jarvis", "Grok" and "Muse" are other companies' names. Use them inside this plan only, never in Keenu's interface or
advertising. The same goes for named characters such as Ani.

## 3. Every feature, and how Keenu gets it

**Route** says what each one needs:

- **Local:** runs on the user's computer, with their own AI account.
- **Key:** needs the user's own paid API key or a service account.
- **Partner:** needs an agreement with, or client IDs from, another company.
- **Cloud:** needs servers that you run and pay for.
- **No:** left out, for the reason given.

### Assistant and voice

| Feature | From | Keenu today | Route | Phase |
|---|---|---|---|---|
| Text chat with an agent | M, S | Linux | Local | 1 |
| Press-to-talk voice | S, F | Linux | Local | 2 |
| Replies read aloud | S, F | Windows and macOS have system speech, unused by an agent | Local | 2 |
| Talking over it to interrupt | F | No | Local | 2 |
| Wake word, "Hey Keenu" | F | No | Local, off by default | 2 |
| Real-time natural voice | X, S | No | Key: xAI lists $0.08 a minute; other providers to compare | 2 |
| A personality and tone the user sets | F, S | No | Local | 3 |
| Memory that learns preferences | M | Today's conversation only | Local, viewable and deletable | 3 |
| Bringing context from other AI apps | S | No | Local: import from the user's export file | 3 |
| Projects or topic folders | S | No | Local | 3 |
| Morning briefing, and speaking up unasked | F | Reminders only | Local, off by default | 3 |
| Knowing what the user is working on | F | Foreground app, opt-in | Local, opt-in | 3 |

### Doing things on the computer

| Feature | From | Keenu today | Route | Phase |
|---|---|---|---|---|
| Open, switch and arrange apps and windows | F | Linux | Local | 1 |
| Drive any app: click, type, use menus | S, F | No | Local, within the limits in section 5 | 1 |
| Files: find, read, open | S, F | Linux | Local | 1 |
| Mac's Mail, Messages, Calendar, Notes | S | No | Local, macOS | 4 |
| Install and fix software, with approval | | Linux | Local | 1 |
| Seeing the screen | S, F | No | Local capture, sent to the AI only if the user turns it on | 1 |
| Coding agent | S | Approvals for Claude Code and Codex | Local, through those tools | Exists |
| Smart home: lights, heating | F | No | Partner: Home Assistant first | 7 |

### Research and connected services

| Feature | From | Keenu today | Route | Phase |
|---|---|---|---|---|
| Web search with citations | S, X | Linux, through the AI | Local | 1 |
| Deep research reports | S | No | Local, as a long agent task | 4 |
| Live search of X posts | S, X | No | Key: xAI's X Search, $5 per 1,000 posts | 4 |
| Connectors: Google Workspace, Dropbox, Notion and more | M, S | CalDAV; Google and Microsoft calendar code, no client IDs yet | Partner: each service needs client IDs | 4 |
| Choosing exactly what each connector may do | M | No | Local | 4 |
| Adding connectors made by others | S | No | Local, through MCP, with review of each one | 4 |
| Turning a saved reel into a grocery list | M | No | Local for a shared link; **No** for reading inside Instagram | 6 |

### Acting on the web

| Feature | From | Keenu today | Route | Phase |
|---|---|---|---|---|
| Its own browser, separate from the user's | M | No | Local | 5 |
| Filling in forms | M | No | Local | 5 |
| Making bookings | S | No | Local, stopping for approval | 5 |
| Negotiating on the user's behalf | M | No | Local. Always says it is an AI, and the user approves the final deal | 5 |
| Shopping up to the checkout | M | No | Local, and the user presses pay | 5 |
| Paying with a one-time card | M | No | Partner: Stripe; availability unconfirmed | 5b |
| Using saved logins without seeing them | M: coming | No | Partner: the 1Password CLI; the AI never sees passwords | 5 |

### Creating

| Feature | From | Keenu today | Route | Phase |
|---|---|---|---|---|
| Understanding images | X | No | Local where the AI's route allows it | 6 |
| Generating and editing images | X, S | No | Key: xAI lists $0.02 to $0.05 an image; others to compare | 6 |
| Generating video from text or a photo | X, S | No | Key: xAI lists $0.02 to $0.08 a second | 6 |
| Explaining a video or post | S | No | Local | 6 |

### Working without the user

| Feature | From | Keenu today | Route | Phase |
|---|---|---|---|---|
| Carrying on after the window closes | M | Linux | Local | 1 |
| Scheduled tasks | S | Reminders only | Local while the computer is on. Key recommended: see section 6 | 3 |
| Telling the user when it needs them | M | Linux | Local | 1 |
| Working while the computer is off | M, S | No | Cloud | 8 |
| Its own computer in the cloud | M, S | No | Cloud | 8 |

### Reaching it from elsewhere

| Feature | From | Keenu today | Route | Phase |
|---|---|---|---|---|
| Chatting from a messaging app | M | No | Telegram first; WhatsApp's business terms need checking first | 7 |
| A bot people tag on X, like `@grok` | S | No | Key: X API, paid. A separate public service | 7b |
| Web app | M | Website only | Cloud | 8 |
| Phone app | M, S | No | A new app, needing a decision: see section 8 | 7 |

### Safety and privacy

| Feature | From | Keenu today | Route | Phase |
|---|---|---|---|---|
| Checks before sensitive actions | M | Approval card | Local | 0, 1 |
| A complete log of what it did and plans to do | M | Conversation only | Local | 0 |
| An independent guard over what reaches the internet | M | No | Local | 0, 5 |
| Never seeing passwords or card details | M | Hides secrets in output; keeps away from key folders | Local | 0, 5 |
| Removing access at any time | M | No | Local | 4 |
| Not used for advertising or training | M | No server, no telemetry | Already so, on the computer. AI providers' own terms apply | Exists |
| Encryption with a key only the user holds | M: coming | Credential store for secrets | Local. Cloud would need it from the start | 3, 8 |

### Not copied

| Feature | From | Why not |
|---|---|---|
| Meta's Muse Spark model and xAI's Grok models as Keenu's own | M, X | They are theirs. Grok can be used through the user's account or key. Keenu picks a route, not a model of its own |
| Reading inside Instagram, Facebook or WhatsApp | M | Only Meta has that access |
| Being built into AI glasses | M | Needs the maker's agreement |
| Named companions such as Ani, and sexual or romantic characters | S | Their characters, and a serious safety and legal risk. Grok's own image tools were [blocked in Indonesia and restricted](https://asianews.network/indonesia-blocks-elon-musks-grok-over-ai-generated-sexualised-images) over this |
| Generating real people's likenesses or sexual images | S | Same reason. Generation goes through a provider's own safety checks, and Keenu refuses these requests too |
| Paying without the user's approval | | Muse also checks first. A mistake here costs real money |

## 4. How someone will use it

1. Run the install command, and turn on the assistant in Settings.
2. Connect an AI: **Sign in with ChatGPT**, their own **Claude Code** or **Grok** sign-in, or an API key.
3. Press the shortcut and speak or type: "Plan my Friday: check my calendar, book a table for four near the office, and
   draft the invite."
4. The island shows each step. Looking runs by itself; anything that sends, spends or deletes shows a card with the exact
   action, and **Allow** or **Deny**.
5. They can close the island and carry on. It tells them when it is finished or needs an answer, and the log shows everything.

### AI accounts: what is allowed

| Account | Route | Rule |
|---|---|---|
| ChatGPT Plus or Pro | Official Sign in with ChatGPT, through the Responses API | [Open for open-source and local apps](https://developers.openai.com/cookbook/articles/sign-in-with-chatgpt). Hosted or paid apps must [apply first](https://developers.openai.com/siwc/token-sharing-open-source) |
| Claude | The user's own unmodified Claude Code | [Other apps may not offer Claude login or handle its tokens](https://code.claude.com/docs/en/legal-and-compliance). Signing in to the unmodified program is allowed, but scripted use is not addressed, and [`--bare` will become the default for `-p`](https://code.claude.com/docs/en/headless), which skips subscription login. **This route may stop working** |
| Grok | The Grok CLI | xAI's terms not yet checked |
| Any provider | The user's API key, in the system's credential store | Always allowed, and billed by use |
| Local models | Ollama or similar, on the user's hardware | Private and offline, but weaker. Added in phase 3 |

## 5. What it cannot do, even when finished

- **Every app:** apps with accessibility information work well. Games and custom-drawn apps need screenshots and are less
  reliable.
- **Windows:** normal programs cannot control admin windows or UAC prompts. Keenu will not work around that.
- **macOS:** the user must grant Accessibility and Screen Recording. Without an Apple Developer ID, those permissions may
  need granting again after each update.
- **Linux on Wayland:** windows can be managed through GNOME, but clicks and keystrokes into other apps are restricted.
- **Other desktops:** on Linux, the island still needs GNOME.
- **While the computer is off:** nothing runs, until phase 8.

## 6. Risks

| Risk | What to do |
|---|---|
| **A web page or email tells the agent to do something.** The main risk of any agent | Everything from outside is data, never instructions. Actions are checked by code, not by the AI. Test against a collection of known attacks at every release |
| **It does the wrong thing with real accounts** | Approval levels, the log, a Stop that always works, and undo where possible |
| **Providers change their rules or block access** | Several routes, always including API keys. Check their terms before each phase. Never handle Claude tokens |
| **Heavy use on a subscription.** Anthropic's limits [assume ordinary individual use](https://code.claude.com/docs/en/legal-and-compliance) | Show usage. Scheduled and long tasks should use an API key where the provider's terms require it |
| **Connectors need many client IDs**, and some need a security review by the provider | Start with a few. Let people add their own |
| **Legal exposure** from payments, bookings, generated media and messages sent as the user | Take legal advice before phases 5b, 6 and 7b. Publish clear terms |
| **Size.** This is a large product for a small team | Stop and review after each phase. Cut freely |

## 7. Changes to the project

| Where | Change | Phase |
|---|---|---|
| `crates/keenu-daemon/src/usage.rs`, `assistant/models.rs`, `assistant/sign_in.rs` | **Stop reading Claude's token** and using it to call Anthropic's API, and review the same for Codex and Grok. This is in the published Linux build | 0 |
| New `crates/keenu-assistant` | Moved from `keenu-daemon/src/assistant` (about 13,000 lines): agent loop, AI routes, chat | 0 |
| New `crates/keenu-guard` | Approval levels, checks on what leaves the computer, the action log | 0 |
| `crates/keenu-platform` | New interfaces: `AppControlProvider`, `HotkeyProvider`, `ScreenProvider`, `SchedulerProvider` | 1 |
| `crates/keenu-windows` | UI Automation, Job Objects, global hotkey, screen capture | 1 |
| `crates/keenu-macos` | Accessibility API, Apple Events, hotkey, recording, and the missing clipboard, approvals and single instance | 1 |
| `crates/keenu-daemon`, `gnome-extension` | Use the shared crates; add AT-SPI to read apps | 1 |
| `crates/keenu-integrations` | ChatGPT sign-in, the Responses API, API key routes | 1 |
| `crates/keenu-whisper` | Build for Windows and macOS; on-device wake word | 2 |
| New `crates/keenu-memory` | Encrypted memory, personality, projects, import | 3 |
| New `crates/keenu-scheduler` | Scheduled tasks and briefings | 3 |
| New `crates/keenu-connectors` | MCP client, connector permissions, built-in connectors | 4 |
| New `crates/keenu-browser` | A separate browser profile driven by the agent | 5 |
| New `crates/keenu-media` | Image and video routes, with safety checks | 6 |
| New `crates/keenu-relay` | Pairing with messaging apps and the phone, sender allow-list | 7 |
| New `services/` and `apps/mobile/` | Only if phases 7 and 8 go ahead | 7, 8 |
| `crates/keenu-app`, `crates/keenu-ui` | Connect all of it to the existing pages, without new design | All |
| `docs/`, `tests/`, `.github/workflows/` | Threat model, privacy, acceptance matrix, evaluation and attack tests, real-app tests in CI | All |

The existing `eval.rs` asks the real AI many requests and checks each answer. Extend it to run for every route.

New dependencies must fit Keenu's GPL licence, and pass `cargo deny`.

## 8. Design rule

**Unchanged:** the island, pages, navigation, theme, colours, fonts, icons and animations.

**How new features fit:**

- **Conversation and voice:** the existing Voice page, with a text field in the existing input style.
- **Steps and approvals:** existing text styles, and the Agents page's Allow and Deny card.
- **Log, memory, connectors, schedules:** rows in the existing list style, on existing pages and in Settings.
- **Images:** the shelf's existing thumbnails.

**Cannot fit, and need your decision:**

| Feature | Why | Recommendation |
|---|---|---|
| Phone app | A new app has to have new screens | Build it in phase 7, with the same theme |
| Video avatar (Muse Realtime Avatar, reported) | Needs a video surface and costly rendering | Leave out |
| Animated companions | New visuals, and the risks above | Leave out. Offer tone and voice settings instead |

## 9. Build order

Time estimates are rough guesses for one experienced developer working with AI help. They have not been tested, and
real-app testing often takes longer. Each phase ends with passing CI, an updated acceptance matrix, and a release you approve.

| Phase | What | Estimate | Finished when |
|---|---|---|---|
| **0. Foundation** | Remove token reading. Shared crate. Guard and action log. Linux works exactly as before | 3–4 weeks | Linux assistant tests pass unchanged, and no code reads Claude's token |
| **1. Agent everywhere** | Text requests, all AI routes, app control, screen capture, on Windows, then macOS, then Linux | 2–3 months | The same 50 real tasks pass on each system |
| **2. Voice** | Hotkey, on-device speech, spoken replies, interruption, wake word, optional real-time voice | 1–2 months | Tested by people with real microphones |
| **3. Memory and routine** | Memory, personality, projects, import, scheduled tasks, briefings, local models | 1–2 months | A week of daily use by real people |

**Stop here and review.** This is a complete assistant. Check who uses it and what they ask for before going on.

| Phase | What | Estimate | Finished when |
|---|---|---|---|
| **4. Connectors and research** | MCP, Gmail, Calendar, Drive, Notion, Dropbox, deep research, X search | 1–2 months | Access can be limited and removed for each; provider reviews passed |
| **5. Web agent** | Own browser, forms, bookings, negotiation, checkout up to the user's approval, 1Password | 2 months | Attack tests pass; nothing is bought or sent without approval |
| **5b. Payments** | One-time cards, if a partner agrees | Unknown | Legal review and a partner agreement |
| **6. Creation** | Image understanding, image and video generation | 1 month | Safety tests pass; costs are shown before spending |
| **7. Reach** | Phone app, Telegram, then WhatsApp if allowed, smart home | 2–3 months | Pairing and sender checks pass an outside security review |
| **7b. Public X bot** | A public reply bot | 1 month, then ongoing moderation | Your decision: it is a separate public service |
| **8. Cloud (optional)** | A computer per user, web app, always on | 3+ months, then ongoing | Outside security audit, with a budget for running costs |

That adds up to roughly 5 to 8 months to the review after phase 3, 11 to 16 months through phase 7, and 15 to 20 months
with the X bot and cloud as well.

**About phase 8:** a cloud computer for each user has real running costs and carries users' data, so it needs paid plans,
a privacy policy and support. It also cannot use Claude subscriptions, as Anthropic forbids routing requests through
them on users' behalf, and OpenAI requires hosted apps to apply first. Users would need API keys, or you would pay for the AI.

## 10. Decisions needed from you

| Decision | Recommendation |
|---|---|
| Accept the risk of the Claude Code route? | Yes, labelled "uses your own Claude Code", with API keys as a backup |
| Which system first? | Windows: the only real desktop available for testing |
| Cloud computer, phase 8? | Not now. Decide after phase 3 |
| Phone app, phase 7? | Yes, after the review |
| Avatars and companions? | No |
| Public bot on X, phase 7b? | No, unless it becomes a real goal: it brings moderation work |
| Budget | Needed: an Apple Developer ID, a Windows code-signing certificate, test devices, API credits for testing |

## 11. Accounts and credentials only you can provide

- Apple Developer ID and a Windows code-signing certificate
- OAuth client IDs: Google, Microsoft, and each connector
- A check of whether Keenu's use fits Sign in with ChatGPT's registration steps
- X API and Telegram bot accounts, for phase 7
- A partner agreement for payments, for phase 5b
- Legal advice, before phases 5b, 6 and 7b

## Sources

Read on 2026-10-31. Details may have changed.

- Meta: [Introducing Muse](https://about.fb.com/news/2026/09/introducing-muse-personal-ai-agent/)
- xAI: [API overview](https://docs.x.ai/docs/overview), [web search](https://docs.x.ai/developers/tools/web-search), [pricing](https://docs.x.ai/developers/pricing)
- Anthropic: [legal and compliance](https://code.claude.com/docs/en/legal-and-compliance), [headless mode](https://code.claude.com/docs/en/headless)
- OpenAI: [Sign in with ChatGPT](https://developers.openai.com/cookbook/articles/sign-in-with-chatgpt), [ChatGPT plan usage](https://developers.openai.com/siwc/token-sharing-open-source)
- Secondary, not confirmed by the companies: [Muse guide (Codersera)](https://codersera.com/blog/meta-muse-ai-agent-app-guide-2026/amp/), [Muse review (eesel)](https://www.eesel.ai/blog/meta-muse-agent-review), [Grok on Google Play](https://play.google.com/store/apps/details?id=ai.x.grok&hl=en_US), [Grok guide](https://aibusinessweekly.net/p/how-to-use-grok), [Grok 4.6 article](https://www.basenor.com/blogs/news/grok-4-6-is-here-video-explanations-and-agentic-ai-explained)
