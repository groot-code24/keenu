# Setting up calendars

On Windows and macOS, Keenu connects to calendars itself. On Linux it uses the accounts in GNOME's Settings → Online
Accounts instead, and nothing here applies.

## CalDAV

This works with Nextcloud, Fastmail, iCloud (with an app-specific password), Radicale and other CalDAV servers.

On the **Calendar** page choose **CalDAV**, then enter:

1. **The server's address**, which must start with `https://`. Your server's documentation will have it, such as
   `https://cloud.example.org/remote.php/dav/`. A plain `http://` address is refused because your password goes with
   every request, unless it is on this computer.
2. **Your user name.**
3. **Your password.** Keenu checks it against the server, then keeps it in your system's credential store and never
   in a file.

## Google Calendar and Microsoft 365

These need an OAuth client ID, which identifies the application to Google or Microsoft. **Keenu does not come with
one**: whoever builds or publishes it must register their own, and an official build's will be listed in its release
notes. Without one, the buttons say so.

Sign-in uses the Authorization Code flow with PKCE and a loopback redirect, in your browser. Keenu never sees your
password, and keeps only the tokens, in your system's credential store.

### Google

1. In the [Google Cloud console](https://console.cloud.google.com/), create a project and enable the **Google
   Calendar API**.
2. Configure the OAuth consent screen. While it is in testing, add yourself as a test user.
3. Under **Credentials**, create an **OAuth client ID** of type **Desktop app**.
4. Give it to Keenu in the environment, or compile it in with the same names at build time:

   ```powershell
   $env:KEENU_GOOGLE_CLIENT_ID = '...apps.googleusercontent.com'
   $env:KEENU_GOOGLE_CLIENT_SECRET = '...'
   ```

Google calls the second value a secret, but for a desktop app it is not one: it cannot be kept secret in a program
people can download.

Scope requested: `https://www.googleapis.com/auth/calendar`.

### Microsoft 365 and Outlook.com

1. In the [Microsoft Entra admin center](https://entra.microsoft.com/), go to **App registrations** → **New
   registration**.
2. Under supported account types, choose personal and organizational accounts, unless you only need one.
3. Under **Authentication**, add the platform **Mobile and desktop applications** with the redirect URI
   `http://localhost`, and turn on **Allow public client flows**.
4. Give it to Keenu:

   ```powershell
   $env:KEENU_MICROSOFT_CLIENT_ID = '...'
   ```

Scopes requested: `offline_access User.Read Calendars.ReadWrite`.

> Connecting to real Google and Microsoft accounts has not been tested yet, because that needs these registrations.
> The requests have been tested against mock servers. See the
> [acceptance matrix](feature-acceptance-matrix.md).

## Mail for the assistant

The assistant can use a Gmail or Outlook account: search and read mail, keep drafts, and send, which asks you each
time. It uses the same client IDs as the calendars, with its own sign-in: Settings → Assistant → **Mail** →
**Gmail** or **Outlook**. Nothing is signed in for mail until you press one.

- **Gmail**: enable the **Gmail API** in the same Google Cloud project. Scope requested:
  `https://www.googleapis.com/auth/gmail.modify` (read, label, draft and send; never delete for good). Google counts
  it as restricted: for your own use, keep the consent screen in testing with yourself as a test user. Google then
  asks you to sign in again every seven days.
- **Outlook**: in the same app registration, under **API permissions**, add the delegated Microsoft Graph
  permissions `Mail.ReadWrite` and `Mail.Send`. Scopes requested: `offline_access User.Read Mail.ReadWrite
  Mail.Send`.

Removing a mail account deletes its sign-in from this computer. To withdraw the access at Google's or Microsoft's end
as well, use your account's security page.

> Mail has been tested against the message formats only, never with a real account.

## What it does

- Shows the next two weeks on the Calendar page, from a copy kept on your computer, so it works offline.
- Syncs every 15 minutes while open and when you press **Sync**, fetching only what changed where the server can
  say: WebDAV sync on CalDAV, `syncToken` on Google, `delta` on Microsoft.
- **Add** creates an event in the first calendar you can write to, from a line such as `Lunch 1:00 pm`. The trash
  icon deletes one. Offline, these wait and go out with the next sync.
- If someone changed an event first, your edit is not sent over theirs: the server's version stands, and the island
  says so.
- Repeating events are expanded, keeping their local time across daylight-saving changes. A rule Keenu cannot
  expand (`BYSETPOS`, say) shows only its first time, rather than a wrong series.

## Removing an account

Removing a Google account also tells Google to withdraw the access. Removing any account deletes its credentials and
its copy of your events from this computer. If you withdraw access from the provider's side first, the account's
status changes to "not synced" and asks you to sign in again.
