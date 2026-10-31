# SPDX-License-Identifier: GPL-3.0-or-later
# Drives a real keenu.exe on a real Windows desktop and checks what it does. Every folder it writes to is a
# temporary one (KEENU_DATA_DIR, CLAUDE_CONFIG_DIR, CODEX_HOME), so your own data and agent settings are not
# touched. It does use the shared clipboard for a moment, and puts your text back afterwards; anything else
# on the clipboard (an image, say) is lost. It shows one notification, and shows the island with permission
# requests for a project called demo. Clicks on it are ignored during the test.
#
#   .\scripts\windows\smoke-test.ps1 -Exe .\target\release\keenu.exe
param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [switch]$SkipClipboard,
    [switch]$SkipNotification
)
$ErrorActionPreference = 'Stop'
$Exe = (Resolve-Path $Exe).Path
if ((Get-Process keenu -ErrorAction SilentlyContinue | Where-Object { $_.Path -ne $Exe })) {
    throw 'Another Keenu is running: close it first, as only one can run in a session.'
}

$root = Join-Path ([IO.Path]::GetTempPath()) ("keenu-smoke-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force $root | Out-Null
$env:KEENU_DATA_DIR = Join-Path $root 'data'
$env:CLAUDE_CONFIG_DIR = Join-Path $root 'claude'
$env:CODEX_HOME = Join-Path $root 'codex'
# Someone at the computer may click what appears; these checks need to know what they pressed themselves.
$env:KEENU_TEST_IGNORE_POINTER = '1'
$script:failures = 0
$script:passes = 0
$script:skips = 0

function Pass($name) { $script:passes++; Write-Host "PASS  $name" }
function Fail($name, $why) { $script:failures++; Write-Host "FAIL  $name : $why" }
function Skip($name, $why) { $script:skips++; Write-Host "SKIP  $name : $why" }
function Check($name, [bool]$condition, $why = '') { if ($condition) { Pass $name } else { Fail $name $why } }

# keenu.exe is a desktop program, so a command is run and waited for with its output captured to files.
function Invoke-Keenu {
    param([string[]]$Arguments, [string]$StandardInput)
    $out = Join-Path $root ([Guid]::NewGuid().ToString('N') + '.out')
    $err = "$out.err"
    $parameters = @{ FilePath = $Exe; ArgumentList = $Arguments; Wait = $true; PassThru = $true; NoNewWindow = $true
        RedirectStandardOutput = $out; RedirectStandardError = $err }
    if ($PSBoundParameters.ContainsKey('StandardInput')) {
        $in = "$out.in"
        [IO.File]::WriteAllText($in, $StandardInput, (New-Object Text.UTF8Encoding($false)))
        $parameters.RedirectStandardInput = $in
    }
    $process = Start-Process @parameters
    [pscustomobject]@{
        Code = $process.ExitCode
        Out = [string](Get-Content $out -Raw -ErrorAction SilentlyContinue)
        Err = [string](Get-Content $err -Raw -ErrorAction SilentlyContinue)
    }
}

Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class KeenuSmoke {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int left, top, right, bottom; }
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowW(string cls, string title);
  [DllImport("user32.dll")] public static extern IntPtr SendMessageW(IntPtr hwnd, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr hwnd, uint msg, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern uint RegisterWindowMessageW(string name);
  public static IntPtr MouseAt(int x, int y) { return new IntPtr((y << 16) | (x & 0xFFFF)); }
}
'@
function Find-Island { [KeenuSmoke]::FindWindowW('KeenuIslandWindow', 'Keenu') }

function Save-Settings($change) {
    $file = Join-Path $env:KEENU_DATA_DIR 'settings.json'
    New-Item -ItemType Directory -Force $env:KEENU_DATA_DIR | Out-Null
    $settings = if (Test-Path $file) { Get-Content $file -Raw | ConvertFrom-Json } else { [pscustomobject]@{} }
    & $change $settings
    [IO.File]::WriteAllText($file, ($settings | ConvertTo-Json -Depth 8), (New-Object Text.UTF8Encoding($false)))
}

function Start-Island {
    $process = Start-Process -FilePath $Exe -PassThru
    for ($i = 0; $i -lt 50; $i++) {
        Start-Sleep -Milliseconds 100
        $window = Find-Island
        if ($window -ne [IntPtr]::Zero) { return @{ Process = $process; Window = $window } }
        if ($process.HasExited) { break }
    }
    throw "The island did not open (process exited: $($process.HasExited))."
}
function Stop-Island($island) {
    [void][KeenuSmoke]::PostMessageW($island.Window, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) # WM_CLOSE
    if (-not $island.Process.WaitForExit(5000)) { $island.Process.Kill(); return $false }
    $true
}

$clipboardBefore = $null
if (-not $SkipClipboard) { try { $clipboardBefore = Get-Clipboard -Raw } catch { $clipboardBefore = $null } }

try {
    # --- Commands
    $version = Invoke-Keenu @('--version')
    Check 'version prints and exits 0' ($version.Code -eq 0 -and $version.Out -match '^\d+\.\d+\.\d+') "code $($version.Code): $($version.Out)"
    $bad = Invoke-Keenu @('--no-such-command')
    Check 'an unknown command fails with a non-zero code' ($bad.Code -ne 0)
    $caps = Invoke-Keenu @('--capabilities')
    Check 'capabilities report each integration' ($caps.Out -match 'Media:' -and $caps.Out -match 'Google Calendar:' -and $caps.Out -match 'Updates:')
    $mon = Invoke-Keenu @('--monitors')
    Check 'displays are listed' ($mon.Code -eq 0 -and $mon.Out -match 'primary') $mon.Out

    # --- Local data
    $todo = Invoke-Keenu @('--todo-add', 'Smoke test task')
    Check 'a task is created' ($todo.Out -match 'Created task (\d+)') $todo.Err
    $taskId = $Matches[1]
    $list = Invoke-Keenu @('--todo-list')
    Check 'the task is still there in a new process' ($list.Out -match 'Smoke test task')
    $done = Invoke-Keenu @('--todo-done', $taskId)
    Check 'the task can be ticked off' ($done.Code -eq 0)
    $reminder = Invoke-Keenu @('--reminder-add', '2099-01-01T09:00:00+05:30', 'Smoke test reminder')
    Check 'a reminder is created' ($reminder.Out -match 'Created reminder')
    $badDate = Invoke-Keenu @('--reminder-add', 'tomorrow', 'x')
    Check 'a bad date is refused' ($badDate.Code -ne 0)

    $ics = Join-Path $root 'in.ics'
    @"
BEGIN:VCALENDAR
VERSION:2.0
BEGIN:VEVENT
UID:smoke-1
DTSTART:20991031T040000Z
DTEND:20991031T043000Z
SUMMARY:Smoke event
RRULE:FREQ=DAILY;COUNT=2
END:VEVENT
END:VCALENDAR
"@ | Set-Content -Encoding ascii $ics
    $import = Invoke-Keenu @('--calendar-import', $ics)
    Check 'an ICS file is imported' ($import.Out -match 'Imported 1') $import.Err
    $again = Invoke-Keenu @('--calendar-import', $ics)
    Check 'importing again adds no duplicate' ($again.Out -match 'Imported 0')
    $export = Join-Path $root 'out.ics'
    [void](Invoke-Keenu @('--calendar-export', $export))
    Check 'an ICS file is exported' ((Get-Content $export -Raw) -match 'Smoke event')

    $lrc = Join-Path $root 'song.lrc'
    "[00:01.00]First line`n[00:03.25]Second line" | Set-Content -Encoding utf8 $lrc
    $lyric = Invoke-Keenu @('--lyrics', $lrc, '2000')
    Check 'a lyrics file is followed by time' ($lyric.Out -match 'First line') "$($lyric.Out) $($lyric.Err)"

    # --- Privacy: everything starts off and is reversible
    $settingsFile = Join-Path $env:KEENU_DATA_DIR 'settings.json'
    $diag = Invoke-Keenu @('--diagnostics')
    Check 'diagnostics report privacy switches off by default' ($diag.Out -match 'Clipboard history: false' -and $diag.Out -match 'Usage recording: false' -and $diag.Out -match 'Voice: false' -and $diag.Out -match 'Online lyrics: false')
    [void](Invoke-Keenu @('--usage-enable'))
    Check 'usage can be turned on' ((Get-Content $settingsFile -Raw | ConvertFrom-Json).usage.enabled -eq $true)
    [void](Invoke-Keenu @('--usage-disable'))
    Check 'and off again' ((Get-Content $settingsFile -Raw | ConvertFrom-Json).usage.enabled -eq $false)

    # --- Agent hooks in temporary agent folders
    $install = Invoke-Keenu @('--agent-install')
    Check 'hooks install' ($install.Code -eq 0) $install.Err
    $claude = Get-Content (Join-Path $env:CLAUDE_CONFIG_DIR 'settings.json') -Raw | ConvertFrom-Json
    Check 'Claude Code has a permission hook with its ten minutes' ($claude.hooks.PermissionRequest[0].hooks[0].timeout -eq 600)
    $codex = Get-Content (Join-Path $env:CODEX_HOME 'hooks.json') -Raw | ConvertFrom-Json
    Check 'Codex has a permission hook' ($codex.hooks.PermissionRequest[0].hooks[0].command -match 'agent-hook codex')
    Check 'installing tells the user about trusting it in Codex' ($install.Out -match '/hooks')
    $status = Invoke-Keenu @('--agent-status')
    Check 'status says installed' ($status.Out -match 'Claude Code: installed' -and $status.Out -match 'Codex: installed') $status.Out
    $brokenDir = Join-Path $root 'broken'
    New-Item -ItemType Directory -Force $brokenDir | Out-Null
    Set-Content -Encoding ascii (Join-Path $brokenDir 'settings.json') '{ not json'
    $env:CLAUDE_CONFIG_DIR, $savedClaude = $brokenDir, $env:CLAUDE_CONFIG_DIR
    $refused = Invoke-Keenu @('--agent-install')
    Check 'settings that are not JSON are refused and left alone' ($refused.Code -ne 0 -and (Get-Content (Join-Path $brokenDir 'settings.json') -Raw) -match 'not json') $refused.Err
    $env:CLAUDE_CONFIG_DIR = $savedClaude

    $permission = '{"hook_event_name":"PermissionRequest","session_id":"s","cwd":"C:\\code\\demo","tool_name":"Bash","tool_input":{"command":"npm test"}}'
    $noApp = Invoke-Keenu @('--agent-hook', 'claude') -StandardInput $permission
    Check 'with no island the hook prints nothing, so the terminal asks' ($noApp.Code -eq 0 -and [string]::IsNullOrWhiteSpace($noApp.Out)) $noApp.Out

    # --- The island
    Save-Settings { param($s) $s | Add-Member -Force onboarded $true }
    $island = Start-Island
    Check 'the island opens' ($island.Window -ne [IntPtr]::Zero)
    Start-Sleep -Milliseconds 500
    Check 'the window is visible' ([KeenuSmoke]::IsWindowVisible($island.Window))
    $rect = New-Object KeenuSmoke+RECT
    [void][KeenuSmoke]::GetWindowRect($island.Window, [ref]$rect)
    Check 'the closed island is its small size' (($rect.right - $rect.left) -lt 500 -and ($rect.bottom - $rect.top) -lt 100) "$($rect.right - $rect.left)x$($rect.bottom - $rect.top)"
    Start-Sleep -Milliseconds 300
    $second = Start-Process -FilePath $Exe -PassThru
    Check 'a second copy leaves at once' ($second.WaitForExit(5000))
    Start-Sleep -Milliseconds 400
    Check 'and the first is still running' (-not $island.Process.HasExited)
    [void][KeenuSmoke]::GetWindowRect($island.Window, [ref]$rect)
    Check 'the second copy opened the first' (($rect.right - $rect.left) -gt 500) "$($rect.right - $rect.left)"

    # Everything Windows may send when the desk changes around it.
    $messages = @{ WM_DISPLAYCHANGE = 0x007E; WM_SETTINGCHANGE = 0x001A; WM_POWERBROADCAST = 0x0218 }
    foreach ($name in $messages.Keys) { [void][KeenuSmoke]::SendMessageW($island.Window, $messages[$name], [IntPtr]18, [IntPtr]::Zero) }
    $taskbar = [KeenuSmoke]::RegisterWindowMessageW('TaskbarCreated')
    [void][KeenuSmoke]::SendMessageW($island.Window, $taskbar, [IntPtr]::Zero, [IntPtr]::Zero)
    [void][KeenuSmoke]::SendMessageW($island.Window, 0x02B1, [IntPtr]7, [IntPtr]::Zero) # session locked
    [void][KeenuSmoke]::SendMessageW($island.Window, 0x02B1, [IntPtr]8, [IntPtr]::Zero) # and unlocked
    Start-Sleep -Milliseconds 300
    Check 'survives display, settings, resume, Explorer and lock messages' (-not $island.Process.HasExited)

    # A real permission request across the real pipe, answered by real keys on the island. Keys, not synthetic
    # clicks: Windows rescales the coordinates of mouse messages sent from a process that is not DPI aware.
    function Press-Key($virtualKey) {
        [void][KeenuSmoke]::SendMessageW((Find-Island), 0x0100, [IntPtr]$virtualKey, [IntPtr]::Zero) # WM_KEYDOWN
    }
    function Ask-And-Press($tabs) {
        $in = Join-Path $root ([Guid]::NewGuid().ToString('N'))
        [IO.File]::WriteAllText($in, $permission, (New-Object Text.UTF8Encoding($false)))
        $out = "$in.out"
        $hook = Start-Process -FilePath $Exe -ArgumentList '--agent-hook', 'claude' -RedirectStandardInput $in -RedirectStandardOutput $out -NoNewWindow -PassThru
        # Long enough for the request to reach the island and for its Allow to count, on a busy computer too: Allow
        # ignores presses for its first 700 ms on screen, and this test cannot see when that began.
        Start-Sleep -Milliseconds 3000
        $asked = -not $hook.HasExited
        for ($i = 0; $i -lt $tabs; $i++) { Press-Key 0x09 }
        Press-Key 0x0D
        $finished = $hook.WaitForExit(8000)
        if (-not $finished) { $hook.Kill() }
        [pscustomobject]@{ Asked = $asked; Finished = $finished; Out = [string](Get-Content $out -Raw) }
    }
    # Tab order on the approval page, from keenu-ui: Ask in terminal, Deny, Allow.
    $deny = Ask-And-Press 2
    Check 'a request waits for an answer' $deny.Asked
    Check 'choosing Deny prints the documented deny' ($deny.Out -match '"behavior":"deny"' -and $deny.Out -match '"message"') "$($deny.Out)"
    $terminal = Ask-And-Press 1
    Check 'choosing Ask in terminal prints nothing' ($terminal.Finished -and [string]::IsNullOrWhiteSpace($terminal.Out)) "$($terminal.Out)"
    $allow = Ask-And-Press 3
    Check 'choosing Allow prints the documented allow' ($allow.Out -match '"behavior":"allow"' -and $allow.Out -match '"hookEventName":"PermissionRequest"') "finished: $($allow.Finished), printed: $($allow.Out)"
    Check 'and the allow carries no field Codex would reject' ($allow.Out -notmatch 'updatedInput|updatedPermissions|interrupt|message')
    $escaped = Ask-And-Press 0
    Check 'Enter with nothing chosen answers nothing' (-not $escaped.Finished) "$($escaped.Out)"

    $wrongToken = Join-Path $env:KEENU_DATA_DIR 'agent-token'
    $goodToken = Get-Content $wrongToken -Raw
    Set-Content -NoNewline -Encoding ascii $wrongToken 'not-the-token'
    $forged = Invoke-Keenu @('--agent-hook', 'claude') -StandardInput $permission
    Check 'a request with the wrong token is ignored' ([string]::IsNullOrWhiteSpace($forged.Out)) $forged.Out
    Set-Content -NoNewline -Encoding ascii $wrongToken $goodToken
    $garbage = Invoke-Keenu @('--agent-hook', 'claude') -StandardInput 'not json at all'
    Check 'garbage from an agent is ignored' ($garbage.Code -eq 0 -and [string]::IsNullOrWhiteSpace($garbage.Out))

    # --- Clipboard, with the real clipboard
    if ($SkipClipboard) {
        Skip 'clipboard history' 'asked to skip'
    } else {
        Set-Clipboard -Value 'copied while history is off'
        Start-Sleep -Milliseconds 500
        $history = Invoke-Keenu @('--clipboard-history-list')
        Check 'nothing is kept while history is off' ($history.Out -notmatch 'copied while')
        Check 'the island survives' (Stop-Island $island)
        Save-Settings { param($s) $s | Add-Member -Force clipboard ([pscustomobject]@{ enabled = $true }) }
        $island = Start-Island
        Start-Sleep -Milliseconds 400
        # As a native program copies, with the data there at once. .NET's own copy promises the data for later and then
        # blocks without answering, which leaves it and the island waiting for each other.
        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
public static class KeenuTestClipboard {
    [DllImport("user32", SetLastError = true)] static extern bool OpenClipboard(IntPtr owner);
    [DllImport("user32")] static extern bool CloseClipboard();
    [DllImport("user32")] static extern bool EmptyClipboard();
    [DllImport("user32", SetLastError = true)] static extern IntPtr SetClipboardData(uint format, IntPtr data);
    [DllImport("user32", CharSet = CharSet.Unicode)] static extern uint RegisterClipboardFormatW(string name);
    [DllImport("kernel32")] static extern IntPtr GlobalAlloc(uint flags, UIntPtr size);
    [DllImport("kernel32")] static extern IntPtr GlobalLock(IntPtr memory);
    [DllImport("kernel32")] static extern bool GlobalUnlock(IntPtr memory);
    static void Put(uint format, byte[] bytes) {
        IntPtr memory = GlobalAlloc(2, (UIntPtr)bytes.Length);
        Marshal.Copy(bytes, 0, GlobalLock(memory), bytes.Length);
        GlobalUnlock(memory);
        if (SetClipboardData(format, memory) == IntPtr.Zero) throw new InvalidOperationException("SetClipboardData failed");
    }
    public static void Copy(string text, string marker, byte[] value) {
        int tries = 0;
        while (!OpenClipboard(IntPtr.Zero)) {
            if (++tries > 200) throw new InvalidOperationException("Something is holding the clipboard");
            Thread.Sleep(25);
        }
        try {
            EmptyClipboard();
            Put(13, Encoding.Unicode.GetBytes(text + "\0"));
            if (!string.IsNullOrEmpty(marker)) Put(RegisterClipboardFormatW(marker), value);
        } finally { CloseClipboard(); }
    }
}
'@
        [KeenuTestClipboard]::Copy('copied with history on', $null, $null)
        Start-Sleep -Milliseconds 600
        [KeenuTestClipboard]::Copy('hunter2-private', 'ExcludeClipboardContentFromMonitorProcessing', [byte[]](1, 0, 0, 0))
        Start-Sleep -Milliseconds 600
        [KeenuTestClipboard]::Copy('hunter3-not-in-history', 'CanIncludeInClipboardHistory', [byte[]](0, 0, 0, 0))
        Start-Sleep -Milliseconds 600
        Check 'the island survives the clipboard' (Stop-Island $island)
        $history = Invoke-Keenu @('--clipboard-history-list')
        Check 'a copy is kept with history on' ($history.Out -match 'copied with history on') $history.Out
        Check 'what a password manager marks private is not kept' ($history.Out -notmatch 'hunter2') $history.Out
        Check 'what an app marks as not for history is not kept' ($history.Out -notmatch 'hunter3') $history.Out
        $copy = Invoke-Keenu @('--clipboard-history-copy', '1')
        Start-Sleep -Milliseconds 200
        Check 'an entry goes back on the clipboard' ($copy.Code -eq 0 -and (Get-Clipboard -Raw) -match 'copied with history on') "$($copy.Err)"
        [void](Invoke-Keenu @('--clipboard-history-clear'))
        $history = Invoke-Keenu @('--clipboard-history-list')
        Check 'clearing empties it' ([string]::IsNullOrWhiteSpace($history.Out))
        $island = Start-Island
    }

    # --- A dropped file is kept by reference. WM_DROPFILES needs a shell object, so the shelf command stands in.
    $listShelf = Invoke-Keenu @('--shelf-list')
    Check 'the shelf lists without error' ($listShelf.Code -eq 0)

    # --- Notifications
    if ($SkipNotification) {
        Skip 'notifications' 'asked to skip'
    } else {
        $note = Invoke-Keenu @('--notify', 'Keenu test', 'This came from the smoke test.')
        Check 'a notification is accepted by Windows' ($note.Code -eq 0) $note.Err
    }

    # --- Closing
    Check 'the island closes cleanly when asked' (Stop-Island $island)
    Check 'the agent token is only readable through the data folder' (Test-Path (Join-Path $env:KEENU_DATA_DIR 'agent-token'))
    $exported = Join-Path $root 'export.json'
    $exportResult = Invoke-Keenu @('--export-data', $exported)
    Check 'data can be exported' ($exportResult.Code -eq 0 -and (Test-Path $exported)) $exportResult.Err
    $uninstall = Invoke-Keenu @('--agent-uninstall')
    Check 'hooks come out again' ($uninstall.Code -eq 0 -and -not ((Get-Content (Join-Path $env:CLAUDE_CONFIG_DIR 'settings.json') -Raw) -match 'agent-hook'))
} finally {
    if (-not $SkipClipboard -and $null -ne $clipboardBefore) { Set-Clipboard -Value $clipboardBefore }
    Get-Process keenu -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $Exe } | Stop-Process -Force -ErrorAction SilentlyContinue
    Remove-Item Env:KEENU_DATA_DIR, Env:CLAUDE_CONFIG_DIR, Env:CODEX_HOME, Env:KEENU_TEST_IGNORE_POINTER -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force $root -ErrorAction SilentlyContinue
}

Write-Host ''
Write-Host "$script:passes passed, $script:failures failed, $script:skips skipped"
if ($script:failures -gt 0) { exit 1 }
