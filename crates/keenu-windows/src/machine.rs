// SPDX-License-Identifier: GPL-3.0-or-later

//! The system itself, for the assistant: where windows sit on the screen, sound, brightness, Wi-Fi and Bluetooth, the
//! dark look, Settings pages, a picture of the screen, locking and power, typing where the cursor is, the clipboard,
//! and what Windows says about itself. Nothing here runs unless the assistant's guard let it.

use std::ffi::c_void;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};

use keenu_platform::{
    Adjust, ClipboardContent, ClipboardProvider, Power, Report, Result, SystemCommand, holds,
};
use windows::Devices::Radios::{Radio, RadioAccessStatus, RadioKind, RadioState};
use windows::Win32::Foundation::{ERROR_SUCCESS, HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, GetMonitorInfoW, HGDIOBJ,
    MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow, ReleaseDC, SRCCOPY, SelectObject,
};
use windows::Win32::Media::Audio::{IMMDeviceEnumerator, MMDeviceEnumerator, eConsole, eRender};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::Win32::System::Power::SetSuspendState;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, REG_DWORD, RegSetKeyValueW};
use windows::Win32::System::Shutdown::LockWorkStation;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, SendInput, VIRTUAL_KEY, VK_RETURN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, HWND_BROADCAST, IsIconic, IsZoomed, PostMessageW, SM_CXVIRTUALSCREEN,
    SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SMTO_ABORTIFHUNG, SW_MAXIMIZE,
    SW_MINIMIZE, SW_RESTORE, SWP_NOACTIVATE, SWP_NOZORDER, SendMessageTimeoutW, SetWindowPos,
    ShowWindow, WM_CLOSE, WM_SETTINGCHANGE,
};
use windows_core::{BOOL, Interface, w};

use crate::control::{ComScope, app_of, bring_to_front, find_window, other_windows, title_of};
use crate::uia::{IAudioEndpointVolume, IMMDeviceActivate};

/// A rectangle on the screen, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Area {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Where `part` of `work` is: `left` is its left half, `top_left` its top-left quarter.
pub fn part_of(work: Area, part: &str) -> Option<Area> {
    let (half_w, half_h) = (work.w / 2, work.h / 2);
    let (right_x, bottom_y) = (work.x + half_w, work.y + half_h);
    Some(match part {
        "left" => Area { w: half_w, ..work },
        "right" => Area {
            x: right_x,
            w: work.w - half_w,
            ..work
        },
        "top" => Area { h: half_h, ..work },
        "bottom" => Area {
            y: bottom_y,
            h: work.h - half_h,
            ..work
        },
        "top_left" => Area {
            w: half_w,
            h: half_h,
            ..work
        },
        "top_right" => Area {
            x: right_x,
            w: work.w - half_w,
            h: half_h,
            ..work
        },
        "bottom_left" => Area {
            y: bottom_y,
            w: half_w,
            h: work.h - half_h,
            ..work
        },
        "bottom_right" => Area {
            x: right_x,
            y: bottom_y,
            w: work.w - half_w,
            h: work.h - half_h,
        },
        "center" => Area {
            x: work.x + work.w / 6,
            y: work.y + work.h / 8,
            w: work.w * 2 / 3,
            h: work.h * 3 / 4,
        },
        _ => return None,
    })
}

/// `count` tiles over `work` for `layout`, left to right and top to bottom; fewer when the layout holds fewer.
pub fn tiles(work: Area, layout: &str, count: usize) -> Vec<Area> {
    let (columns, rows) = match layout {
        "halves" => (2, 1),
        "rows" => (1, 2),
        "thirds" => (3, 1),
        "quarters" => (2, 2),
        _ => {
            let columns = (count.max(1) as f64).sqrt().ceil() as usize;
            (columns, count.max(1).div_ceil(columns))
        }
    };
    let count = count.min(columns * rows);
    (0..count)
        .map(|index| {
            let (column, row) = ((index % columns) as i32, (index / columns) as i32);
            let (columns, rows) = (columns as i32, rows as i32);
            let x = work.x + work.w * column / columns;
            let y = work.y + work.h * row / rows;
            Area {
                x,
                y,
                w: work.x + work.w * (column + 1) / columns - x,
                h: work.y + work.h * (row + 1) / rows - y,
            }
        })
        .collect()
}

/// The part of the display `window` is on that windows may use: without the taskbar.
fn work_area(window: HWND) -> Result<Area> {
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `info` is sized as the call expects.
    let found = unsafe {
        GetMonitorInfoW(
            MonitorFromWindow(window, MONITOR_DEFAULTTONEAREST),
            &mut info,
        )
    };
    if !found.as_bool() {
        return Err("Windows would not say where the display is.".into());
    }
    let RECT {
        left,
        top,
        right,
        bottom,
    } = info.rcWork;
    Ok(Area {
        x: left,
        y: top,
        w: right - left,
        h: bottom - top,
    })
}

fn place(window: HWND, area: Area) -> Result<()> {
    // SAFETY: acts only on a window Windows named.
    unsafe {
        // A maximized or minimized window does not move until it is a normal one again.
        if IsZoomed(window).as_bool() || IsIconic(window).as_bool() {
            let _ = ShowWindow(window, SW_RESTORE);
        }
        SetWindowPos(
            window,
            None,
            area.x,
            area.y,
            area.w,
            area.h,
            SWP_NOZORDER | SWP_NOACTIVATE,
        )
    }
    .map_err(|err| format!("Windows would not move “{}”: {err}", title_of(window)))
}

fn window_command(window: &str, command: &str) -> Result<String> {
    let found = find_window(window)?;
    let title = title_of(found);
    // SAFETY: each acts only on a window Windows named.
    unsafe {
        match command {
            "focus" => bring_to_front(found)?,
            "close" => PostMessageW(Some(found), WM_CLOSE, WPARAM(0), LPARAM(0))
                .map_err(|err| format!("Windows would not close it: {err}"))?,
            "minimize" => {
                let _ = ShowWindow(found, SW_MINIMIZE);
            }
            "maximize" => {
                let _ = ShowWindow(found, SW_MAXIMIZE);
            }
            "unmaximize" => {
                let _ = ShowWindow(found, SW_RESTORE);
            }
            part => {
                let area = part_of(work_area(found)?, part)
                    .ok_or_else(|| format!("“{part}” is not a part of the screen."))?;
                place(found, area)?;
                let _ = bring_to_front(found);
            }
        }
    }
    Ok(format!("{}: “{title}”.", command.replace('_', " ")))
}

fn arrange(layout: &str, apps: &[String]) -> Result<String> {
    let open = other_windows()?;
    let wanted = tiles(
        Area {
            x: 0,
            y: 0,
            w: 1,
            h: 1,
        },
        layout,
        9,
    )
    .len();
    let chosen: Vec<HWND> = match apps {
        [] => open.into_iter().take(wanted).collect(),
        // Every window of the one app named.
        [only] => open
            .into_iter()
            .filter(|w| holds(&app_of(*w), only) || holds(&title_of(*w), only))
            .take(wanted)
            .collect(),
        several => {
            let mut chosen = Vec::new();
            for app in several {
                let found = open.iter().find(|w| {
                    !chosen.contains(*w) && (holds(&app_of(**w), app) || holds(&title_of(**w), app))
                });
                match found {
                    Some(window) => chosen.push(*window),
                    None => return Err(format!("No window of “{app}” is open: open it first.")),
                }
            }
            chosen
        }
    };
    let Some(first) = chosen.first() else {
        return Err("There are no windows to arrange.".into());
    };
    let areas = tiles(work_area(*first)?, layout, chosen.len());
    for (window, area) in chosen.iter().zip(&areas) {
        place(*window, *area)?;
    }
    let titles: Vec<String> = chosen
        .iter()
        .take(areas.len())
        .map(|w| title_of(*w))
        .collect();
    Ok(format!("Arranged as {layout}: {}.", titles.join(", ")))
}

fn speakers() -> Result<IAudioEndpointVolume> {
    let failed = |err: windows_core::Error| {
        format!(
            "Windows has no sound output Keenu can reach: {}",
            err.message()
        )
    };
    // SAFETY: COM calls on objects this function holds, on a thread with COM.
    unsafe {
        let devices: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(failed)?;
        let device = devices
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .map_err(failed)?;
        let device: IMMDeviceActivate = device.cast().map_err(failed)?;
        let mut raw = std::ptr::null_mut();
        device
            .Activate(
                &IAudioEndpointVolume::IID,
                CLSCTX_ALL.0,
                std::ptr::null(),
                &mut raw,
            )
            .ok()
            .map_err(failed)?;
        if raw.is_null() {
            return Err("Windows has no sound output Keenu can reach.".into());
        }
        Ok(IAudioEndpointVolume::from_raw(raw))
    }
}

fn volume(adjust: Adjust) -> Result<String> {
    let _com = ComScope::enter();
    let speakers = speakers()?;
    let failed =
        |err: windows_core::Error| format!("Windows would not change the sound: {}", err.message());
    // SAFETY: COM calls on the object above.
    unsafe {
        let mut level = 0.0f32;
        let _ = speakers.GetMasterVolumeLevelScalar(&mut level);
        let set = |level: f32| {
            speakers
                .SetMasterVolumeLevelScalar(level.clamp(0.0, 1.0), std::ptr::null())
                .ok()
                .map_err(failed)?;
            speakers
                .SetMute(BOOL::from(false), std::ptr::null())
                .ok()
                .map_err(failed)?;
            Ok::<String, String>(format!("Volume at {:.0}%.", level.clamp(0.0, 1.0) * 100.0))
        };
        match adjust {
            Adjust::Set(percent) => set(f32::from(percent) / 100.0),
            Adjust::Up => set(level + 0.1),
            Adjust::Down => set(level - 0.1),
            Adjust::Mute | Adjust::Unmute => {
                let mute = adjust == Adjust::Mute;
                speakers
                    .SetMute(BOOL::from(mute), std::ptr::null())
                    .ok()
                    .map_err(failed)?;
                Ok(if mute { "Sound muted." } else { "Sound on." }.to_owned())
            }
        }
    }
}

/// Runs a script of this program's own with Windows PowerShell, hidden; what it printed.
fn powershell(script: &str) -> Result<String> {
    let script = format!(
        "[Console]::OutputEncoding=[Text.Encoding]::UTF8\n$ErrorActionPreference='Stop'\n{script}"
    );
    let encoded: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let output = std::process::Command::new(crate::system::system32(
        "WindowsPowerShell\\v1.0\\powershell.exe",
    ))
    .args([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-EncodedCommand",
    ])
    .arg(crate::voice::base64(&encoded))
    .stdin(std::process::Stdio::null())
    .creation_flags(0x0800_0000)
    .output()
    .map_err(|err| format!("Could not start Windows PowerShell: {err}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        let why = String::from_utf8_lossy(&output.stderr);
        Err(why
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("it failed")
            .trim()
            .to_owned())
    }
}

fn brightness(adjust: Adjust) -> Result<String> {
    let unsupported = |_| {
        "This display does not let Windows set its brightness: a laptop's own screen does, most other displays do not.".to_owned()
    };
    let now: i32 = powershell("(Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightness | Select-Object -First 1).CurrentBrightness")
        .map_err(unsupported)?
        .parse()
        .map_err(|_| unsupported(String::new()))?;
    let wanted = match adjust {
        Adjust::Set(percent) => i32::from(percent),
        Adjust::Up => now + 10,
        Adjust::Down => now - 10,
        Adjust::Mute | Adjust::Unmute => return Err("Brightness has no mute.".into()),
    }
    .clamp(0, 100);
    powershell(&format!(
        "Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightnessMethods | Invoke-CimMethod -MethodName WmiSetBrightness -Arguments @{{Timeout=0; Brightness=[byte]{wanted}}} | Out-Null"
    ))
    .map_err(unsupported)?;
    Ok(format!("Brightness at {wanted}%."))
}

fn radio(kind: RadioKind, on: bool, name: &str) -> Result<String> {
    let _com = ComScope::enter();
    let failed =
        |err: windows_core::Error| format!("Windows would not switch {name}: {}", err.message());
    let access = Radio::RequestAccessAsync()
        .and_then(|op| op.join())
        .map_err(failed)?;
    if access != RadioAccessStatus::Allowed {
        return Err(format!(
            "Windows did not let Keenu switch {name}. Allow it in Settings > Privacy & security > Radios, or switch it in Quick Settings."
        ));
    }
    let radios = Radio::GetRadiosAsync()
        .and_then(|op| op.join())
        .map_err(failed)?;
    let mut found = false;
    for index in 0..radios.Size().unwrap_or(0) {
        let Ok(radio) = radios.GetAt(index) else {
            continue;
        };
        if radio.Kind().ok() != Some(kind) {
            continue;
        }
        found = true;
        let state = if on { RadioState::On } else { RadioState::Off };
        let status = radio
            .SetStateAsync(state)
            .and_then(|op| op.join())
            .map_err(failed)?;
        if status != RadioAccessStatus::Allowed {
            return Err(format!(
                "Windows did not let Keenu switch {name}: switch it in Quick Settings."
            ));
        }
    }
    if !found {
        return Err(format!(
            "This computer has no {name} that Windows lets programs switch."
        ));
    }
    Ok(format!("{name} {}.", if on { "on" } else { "off" }))
}

fn dark_mode(on: bool) -> Result<String> {
    let light: u32 = u32::from(!on);
    for name in [w!("AppsUseLightTheme"), w!("SystemUsesLightTheme")] {
        // SAFETY: the value is a DWORD, and its size goes with it.
        let status = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
                name,
                REG_DWORD.0,
                Some(&light as *const u32 as *const c_void),
                4,
            )
        };
        if status != ERROR_SUCCESS {
            return Err("Windows would not change its colours.".into());
        }
    }
    // Apps that are open take it up when told.
    // SAFETY: a broadcast of a string this program owns, with a short timeout for windows that hang.
    unsafe {
        let _ = SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(w!("ImmersiveColorSet").as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            200,
            None,
        );
    }
    Ok(if on {
        "Dark mode on."
    } else {
        "Dark mode off."
    }
    .to_owned())
}

/// The Settings page for `panel`.
pub fn settings_page(panel: &str) -> &'static str {
    match panel {
        "wifi" | "wi_fi" => "network-wifi",
        "bluetooth" | "devices" => "bluetooth",
        "display" | "screen" => "display",
        "sound" | "audio" => "sound",
        "notifications" | "dnd" | "focus" => "notifications",
        "power" | "sleep" => "powersleep",
        "battery" => "batterysaver",
        "privacy" => "privacy",
        "apps" | "applications" => "appsfeatures",
        "update" | "windows_update" => "windowsupdate",
        "network" => "network-status",
        "personalization" | "background" | "colors" => "personalization",
        "mouse" | "touchpad" => "mousetouchpad",
        "keyboard" | "typing" => "typing",
        "printers" => "printers",
        _ => "",
    }
}

fn settings(panel: &str) -> Result<String> {
    let page = settings_page(panel);
    crate::control::shell_open(Path::new(&format!("ms-settings:{page}")))?;
    Ok(if page.is_empty() {
        "Opened Settings.".to_owned()
    } else {
        format!("Opened Settings at {panel}.")
    })
}

fn pictures_folder() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join("Pictures")
        .join("Screenshots")
}

fn screenshot() -> Result<String> {
    // SAFETY: every object made here is released before returning, and the pixel buffer is the size asked for.
    let (width, height, pixels) = unsafe {
        let (x, y) = (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
        );
        let (width, height) = (
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        );
        if width <= 0 || height <= 0 {
            return Err("Windows would not say how large the screen is.".into());
        }
        let screen = GetDC(None);
        let memory = CreateCompatibleDC(Some(screen));
        let bitmap = CreateCompatibleBitmap(screen, width, height);
        let previous = SelectObject(memory, HGDIOBJ(bitmap.0));
        let copied = BitBlt(memory, 0, 0, width, height, Some(screen), x, y, SRCCOPY);
        // GetDIBits wants the bitmap out of every device context.
        SelectObject(memory, previous);
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                // Negative: rows top to bottom.
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            bmiColors: [Default::default()],
        };
        let mut pixels = vec![0u8; width as usize * height as usize * 4];
        let rows = GetDIBits(
            memory,
            bitmap,
            0,
            height as u32,
            Some(pixels.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        );
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(memory);
        ReleaseDC(None, screen);
        if copied.is_err() || rows == 0 {
            return Err("Windows would not copy the screen.".into());
        }
        (width as u32, height as u32, pixels)
    };
    let mut pixels = pixels;
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        pixel[3] = 255;
    }
    let picture = image::RgbaImage::from_raw(width, height, pixels)
        .ok_or("Windows gave a picture of the wrong size.")?;
    let folder = pictures_folder();
    std::fs::create_dir_all(&folder)
        .map_err(|err| format!("Could not make {}: {err}", folder.display()))?;
    let file = folder.join(format!(
        "Keenu {}.png",
        chrono::Local::now().format("%Y-%m-%d %H.%M.%S")
    ));
    picture
        .save(&file)
        .map_err(|err| format!("Could not save the picture: {err}"))?;
    Ok(format!(
        "Saved a picture of the screen as {}.",
        file.display()
    ))
}

fn power(what: Power) -> Result<String> {
    let shutdown = |flag: &str| {
        std::process::Command::new(crate::system::system32("shutdown.exe"))
            .args([flag, "/t", "0"])
            .creation_flags(0x0800_0000)
            .status()
            .map_err(|err| format!("Could not start shutdown: {err}"))
            .and_then(|status| {
                status
                    .success()
                    .then_some(())
                    .ok_or_else(|| "Windows refused: an app may be holding it up.".to_owned())
            })
    };
    match what {
        Power::Sleep => {
            // SAFETY: asks Windows to sleep; it returns once the computer wakes.
            if unsafe { SetSuspendState(false, false, false) } {
                Ok("The computer slept, and is awake again.".into())
            } else {
                Err("Windows would not sleep now.".into())
            }
        }
        Power::Restart => shutdown("/r").map(|()| "Restarting.".to_owned()),
        Power::ShutDown => shutdown("/s").map(|()| "Shutting down.".to_owned()),
        Power::SignOut => std::process::Command::new(crate::system::system32("shutdown.exe"))
            .arg("/l")
            .creation_flags(0x0800_0000)
            .spawn()
            .map(|_| "Signing out.".to_owned())
            .map_err(|err| format!("Could not sign out: {err}")),
    }
}

fn unicode_key(unit: u16, up: bool) -> INPUT {
    let flags = if up {
        KEYEVENTF_UNICODE | KEYEVENTF_KEYUP
    } else {
        KEYEVENTF_UNICODE
    };
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(0),
                wScan: unit,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn enter_key(up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VK_RETURN,
                wScan: 0,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Types `text` in the app the user was in, not in Keenu's own island.
fn write(text: &str) -> Result<String> {
    let target = find_window("")?;
    bring_to_front(target)?;
    let mut inputs = Vec::new();
    for line in text.replace("\r\n", "\n").split_inclusive('\n') {
        for unit in line.trim_end_matches('\n').encode_utf16() {
            inputs.push(unicode_key(unit, false));
            inputs.push(unicode_key(unit, true));
        }
        if line.ends_with('\n') {
            inputs.push(enter_key(false));
            inputs.push(enter_key(true));
        }
    }
    // SAFETY: the slice and the size go together.
    let sent = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
    if sent as usize == inputs.len() {
        Ok(format!("Typed it in “{}”.", title_of(target)))
    } else {
        Err("Windows did not take the typing; something may be blocking input.".into())
    }
}

/// Carries out `command` on this computer.
pub fn run(command: &SystemCommand) -> Result<String> {
    match command {
        SystemCommand::Window { window, command } => window_command(window, command),
        SystemCommand::Arrange { layout, apps } => arrange(layout, apps),
        SystemCommand::Volume(adjust) => volume(*adjust),
        SystemCommand::Brightness(adjust) => brightness(*adjust),
        SystemCommand::Wifi(on) => radio(RadioKind::WiFi, *on, "Wi-Fi"),
        SystemCommand::Bluetooth(on) => radio(RadioKind::Bluetooth, *on, "Bluetooth"),
        SystemCommand::DarkMode(on) => dark_mode(*on),
        SystemCommand::Settings(panel) => settings(panel),
        SystemCommand::Screenshot => screenshot(),
        // SAFETY: locks this session, as Windows+L does.
        SystemCommand::Lock => unsafe { LockWorkStation() }
            .map(|()| "Locked.".to_owned())
            .map_err(|err| format!("Windows would not lock: {err}")),
        SystemCommand::Power(what) => power(*what),
        SystemCommand::Write(text) => write(text),
        SystemCommand::Copy(text) => crate::clipboard::Clipboard
            .write(&ClipboardContent::Text(text.clone()))
            .map(|()| "Copied.".to_owned()),
    }
}

const TOP_PROCESSES: &str = r#"
$all = Get-CimInstance Win32_PerfFormattedData_PerfProc_Process | Where-Object { $_.Name -ne '_Total' -and $_.Name -ne 'Idle' }
$cores = [Environment]::ProcessorCount
'Using the processor most (share of the whole processor):'
$all | Sort-Object PercentProcessorTime -Descending | Select-Object -First 8 | ForEach-Object { '- {0}: {1:N0}%, {2:N0} MB' -f $_.Name, ($_.PercentProcessorTime / $cores), ($_.WorkingSetPrivate / 1MB) }
'Using the most memory:'
$all | Sort-Object WorkingSetPrivate -Descending | Select-Object -First 8 | ForEach-Object { '- {0}: {1:N0} MB' -f $_.Name, ($_.WorkingSetPrivate / 1MB) }
"#;

const SYSTEM_INFO: &str = r#"
$os = Get-CimInstance Win32_OperatingSystem
'System: {0} {1}' -f $os.Caption, $os.Version
$up = (Get-Date) - $os.LastBootUpTime
'On for: {0} days {1} hours {2} minutes' -f $up.Days, $up.Hours, $up.Minutes
'Memory: {0:N1} GB free of {1:N1} GB' -f ($os.FreePhysicalMemory / 1MB), ($os.TotalVisibleMemorySize / 1MB)
Get-CimInstance Win32_LogicalDisk -Filter 'DriveType=3' | ForEach-Object { 'Disk {0} {1:N0} GB free of {2:N0} GB' -f $_.DeviceID, ($_.FreeSpace / 1GB), ($_.Size / 1GB) }
$cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
'Processor: {0}, {1}% busy' -f $cpu.Name.Trim(), $cpu.LoadPercentage
$battery = @(Get-CimInstance Win32_Battery)
if ($battery.Count -eq 0) { 'Battery: none' } else { $battery | ForEach-Object { 'Battery: {0}%, {1}' -f $_.EstimatedChargeRemaining, $(if ($_.BatteryStatus -eq 2) { 'plugged in' } else { 'on battery' }) } }
"#;

/// What Windows says, for the assistant.
pub fn report(report: Report) -> Result<String> {
    match report {
        Report::TopProcesses => powershell(TOP_PROCESSES),
        Report::SystemInfo => powershell(SYSTEM_INFO),
        Report::Clipboard => Ok(match crate::clipboard::Clipboard.read()? {
            Some(ClipboardContent::Text(text) | ClipboardContent::Rich { text, .. }) => text,
            Some(ClipboardContent::Files(files)) => {
                let names: Vec<String> = files.iter().map(|f| f.display().to_string()).collect();
                format!("Copied files: {}", names.join(", "))
            }
            Some(ClipboardContent::Image { width, height, .. }) => {
                format!("A copied picture, {width}×{height}.")
            }
            None => "Nothing, or something a password manager marked as private.".into(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Area = Area {
        x: 0,
        y: 0,
        w: 1920,
        h: 1040,
    };

    #[test]
    fn a_part_of_the_screen_is_where_people_mean() {
        assert_eq!(
            part_of(SCREEN, "left"),
            Some(Area {
                x: 0,
                y: 0,
                w: 960,
                h: 1040
            })
        );
        assert_eq!(
            part_of(SCREEN, "bottom_right"),
            Some(Area {
                x: 960,
                y: 520,
                w: 960,
                h: 520
            })
        );
        assert!(part_of(SCREEN, "upside").is_none());
    }

    #[test]
    fn tiles_cover_the_screen_without_gaps() {
        let quarters = tiles(SCREEN, "quarters", 4);
        assert_eq!(quarters.len(), 4);
        assert_eq!(
            quarters[3],
            Area {
                x: 960,
                y: 520,
                w: 960,
                h: 520
            }
        );
        // A third of 1920 leaves nothing over.
        let thirds = tiles(SCREEN, "thirds", 5);
        assert_eq!(thirds.len(), 3);
        assert_eq!(thirds[2].x + thirds[2].w, 1920);
        let grid = tiles(SCREEN, "grid", 5);
        assert_eq!(grid.len(), 5);
        assert_eq!(grid[0].w, 640);
    }

    #[test]
    fn settings_names_go_to_their_pages() {
        assert_eq!(settings_page("wifi"), "network-wifi");
        assert_eq!(settings_page("dnd"), "notifications");
        assert_eq!(settings_page("anything else"), "");
    }

    #[test]
    fn the_system_reports_itself() {
        let said = report(Report::SystemInfo).unwrap();
        assert!(
            said.contains("System:") && said.contains("Memory:"),
            "{said}"
        );
    }
}
