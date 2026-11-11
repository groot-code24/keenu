// SPDX-License-Identifier: GPL-3.0-or-later

//! Using other apps: Win32 for windows and keys, UI Automation for what is in them. Password fields are refused here,
//! by what Windows says they are and not what they are called. A window running as administrator cannot be reached by
//! a normal program, which Windows enforces and Keenu does not try to get around.

use std::os::windows::process::CommandExt;

use keenu_platform::{
    AppControlProvider, Availability, KeyCombo, Result, SECRET_FIELD, UiElement, WindowInfo, holds,
    pick_element,
};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, TRUE, WPARAM};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP,
    MAPVK_VK_TO_VSC, MapVirtualKeyW, SendInput, VIRTUAL_KEY, VK_BACK, VK_CONTROL, VK_DELETE,
    VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_HOME, VK_LEFT, VK_LMENU, VK_LWIN, VK_NEXT, VK_PRIOR,
    VK_RETURN, VK_RIGHT, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, IsIconic, IsWindowVisible, PostMessageW, SW_RESTORE,
    SetForegroundWindow, ShowWindow, WM_CLOSE,
};
use windows_core::{BOOL, PWSTR};

use crate::uia::{
    CUI_AUTOMATION, IUIAutomation, IUIAutomationElement, IUIAutomationInvokePattern,
    IUIAutomationSelectionItemPattern, IUIAutomationTogglePattern, IUIAutomationValuePattern,
    TREE_SCOPE_DESCENDANTS, UIA_INVOKE_PATTERN, UIA_SELECTION_ITEM_PATTERN, UIA_TOGGLE_PATTERN,
    UIA_VALUE_PATTERN,
};

pub struct AppControl;

/// COM for this thread, for as long as it is held: callers' threads may not have it yet.
pub(crate) struct ComScope(bool);

impl ComScope {
    pub(crate) fn enter() -> Self {
        // SAFETY: paired with `CoUninitialize` in `drop`, on this thread, only when this call counted.
        Self(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok())
    }
}

impl Drop for ComScope {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: this thread's `CoInitializeEx` succeeded.
            unsafe { CoUninitialize() };
        }
    }
}

fn automation() -> Result<IUIAutomation> {
    // SAFETY: a plain COM instantiation on a thread with COM.
    unsafe { CoCreateInstance(&CUI_AUTOMATION, None, CLSCTX_INPROC_SERVER) }
        .map_err(|err| format!("Windows' UI Automation is not available: {err}"))
}

pub(crate) fn title_of(window: HWND) -> String {
    // SAFETY: the buffer's length goes with it.
    unsafe {
        let length = GetWindowTextLengthW(window);
        if length <= 0 {
            return String::new();
        }
        let mut buffer = vec![0u16; length as usize + 1];
        let copied = GetWindowTextW(window, &mut buffer);
        String::from_utf16_lossy(&buffer[..copied.max(0) as usize])
    }
}

/// The program behind a window, without its folder or `.exe`.
pub(crate) fn app_of(window: HWND) -> String {
    // SAFETY: the handle is closed below, and the buffer's length goes with it.
    unsafe {
        let mut id = 0;
        GetWindowThreadProcessId(window, Some(&mut id));
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, id) else {
            return String::new();
        };
        let mut buffer = [0u16; 1024];
        let mut size = buffer.len() as u32;
        let result = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(process);
        if result.is_err() {
            return String::new();
        }
        let full = String::from_utf16_lossy(&buffer[..size as usize]);
        let file = full.rsplit('\\').next().unwrap_or_default();
        file.strip_suffix(".exe")
            .or_else(|| file.strip_suffix(".EXE"))
            .unwrap_or(file)
            .to_owned()
    }
}

extern "system" fn collect(window: HWND, list: LPARAM) -> BOOL {
    // SAFETY: `list` is the `Vec` in `top_windows`, which outlives `EnumWindows`.
    let list = unsafe { &mut *(list.0 as *mut Vec<HWND>) };
    // SAFETY: only asks about a window Windows just named.
    if unsafe { IsWindowVisible(window) }.as_bool() && !title_of(window).is_empty() {
        list.push(window);
    }
    TRUE
}

fn top_windows() -> Result<Vec<HWND>> {
    let mut list: Vec<HWND> = Vec::new();
    // SAFETY: `collect` reads `list` only during this call.
    unsafe { EnumWindows(Some(collect), LPARAM(&mut list as *mut Vec<HWND> as isize)) }
        .map_err(|err| format!("Windows would not list its windows: {err}"))?;
    Ok(list)
}

/// Whether `window` is one of Keenu's own.
fn is_mine(window: HWND) -> bool {
    let mut id = 0;
    // SAFETY: only asks about a window Windows named.
    unsafe { GetWindowThreadProcessId(window, Some(&mut id)) };
    id == std::process::id()
}

/// Other programs' windows, front to back.
pub(crate) fn other_windows() -> Result<Vec<HWND>> {
    Ok(top_windows()?
        .into_iter()
        .filter(|w| !is_mine(*w))
        .collect())
}

fn window_id(window: HWND) -> String {
    format!("{:x}", window.0 as usize)
}

/// The window meant: by its id, else the only one whose title or app holds `wanted`; the one in front when empty.
/// That is never Keenu's own island, which has the focus while the user talks to it, but what they were using.
pub(crate) fn find_window(wanted: &str) -> Result<HWND> {
    let wanted = wanted.trim();
    if wanted.is_empty() {
        // SAFETY: only asks which window is in front.
        let front = unsafe { GetForegroundWindow() };
        if !front.is_invalid() && !is_mine(front) {
            return Ok(front);
        }
        return other_windows()?
            .into_iter()
            .next()
            .ok_or_else(|| "No window is in front.".to_owned());
    }
    let windows = top_windows()?;
    if let Some(window) = windows.iter().find(|window| window_id(**window) == wanted) {
        return Ok(*window);
    }
    let mine = std::process::id();
    let found: Vec<HWND> = windows
        .into_iter()
        .filter(|window| {
            let mut id = 0;
            // SAFETY: only asks about a window Windows just named.
            unsafe { GetWindowThreadProcessId(*window, Some(&mut id)) };
            id != mine && (holds(&title_of(*window), wanted) || holds(&app_of(*window), wanted))
        })
        .collect();
    match found.as_slice() {
        [] => Err(format!("No window called “{wanted}” is open.")),
        [only] => Ok(*only),
        several => {
            let titles: Vec<String> = several.iter().take(6).map(|w| title_of(*w)).collect();
            Err(format!(
                "More than one window could be “{wanted}”: {}. Say which.",
                titles.join(", ")
            ))
        }
    }
}

pub(crate) fn bring_to_front(window: HWND) -> Result<()> {
    // SAFETY: only acts on a window Windows named.
    unsafe {
        if IsIconic(window).as_bool() {
            let _ = ShowWindow(window, SW_RESTORE);
        }
        let _ = SetForegroundWindow(window);
        std::thread::sleep(std::time::Duration::from_millis(120));
        if GetForegroundWindow() == window {
            Ok(())
        } else {
            Err(format!(
                "Windows would not bring “{}” to the front.",
                title_of(window)
            ))
        }
    }
}

/// A control and how to reach it again.
struct Found {
    element: IUIAutomationElement,
    info: UiElement,
}

fn role_name(kind: i32) -> &'static str {
    // UIA_*ControlTypeId, whose numbers are fixed by the interface.
    match kind {
        50000 => "button",
        50002 => "checkbox",
        50003 => "combobox",
        50004 => "field",
        50005 => "link",
        50007 => "listitem",
        50011 => "menuitem",
        50013 => "radiobutton",
        50019 => "tab",
        50020 => "text",
        50025 => "treeitem",
        50030 => "document",
        50033 => "pane",
        50032 => "window",
        _ => "other",
    }
}

fn controls(window: HWND, limit: usize) -> Result<Vec<Found>> {
    let automation = automation()?;
    // SAFETY: COM calls on objects this function holds; strings are copied out at once.
    unsafe {
        let root = automation
            .element_from_handle(window)
            .map_err(|err| reach_error(&title_of(window), &err))?;
        let all = automation
            .create_true_condition()
            .map_err(|err| err.to_string())?;
        let found = root
            .find_all(TREE_SCOPE_DESCENDANTS, &all)
            .map_err(|err| reach_error(&title_of(window), &err))?;
        let count = found.length().unwrap_or(0).max(0) as usize;
        let mut list = Vec::new();
        for index in 0..count {
            if list.len() >= limit {
                break;
            }
            let Ok(element) = found.element(index as i32) else {
                continue;
            };
            let name = element.name().unwrap_or_default();
            let kind = element.control_type().unwrap_or(0);
            let role = role_name(kind);
            if role == "other" || (role == "pane" && name.is_empty()) {
                continue;
            }
            let secret = element.is_password().unwrap_or(false);
            // Never asked of a password field.
            let value = if secret {
                String::new()
            } else {
                element
                    .pattern::<IUIAutomationValuePattern>(UIA_VALUE_PATTERN)
                    .and_then(|pattern| pattern.value())
                    .unwrap_or_default()
            };
            if name.is_empty() && value.is_empty() && !matches!(role, "field" | "document") {
                continue;
            }
            list.push(Found {
                info: UiElement {
                    role: role.to_owned(),
                    name,
                    value,
                    enabled: element.is_enabled().unwrap_or(false),
                    secret,
                },
                element,
            });
        }
        Ok(list)
    }
}

fn reach_error(title: &str, err: &windows_core::Error) -> String {
    format!(
        "Keenu could not reach “{title}” ({err}). If it runs as administrator, Windows keeps other programs out of it."
    )
}

/// The control called `target`, from `found`.
fn pick<'a>(found: &'a [Found], target: &str, what: &str) -> Result<&'a Found> {
    let infos: Vec<UiElement> = found.iter().map(|f| f.info.clone()).collect();
    let picked = pick_element(&infos, target, what)?;
    let at = infos
        .iter()
        .position(|info| std::ptr::eq(info, picked))
        .unwrap_or(0);
    Ok(&found[at])
}

fn press(element: &IUIAutomationElement) -> Result<()> {
    // SAFETY: COM calls on an element this function was given.
    unsafe {
        if let Ok(pattern) = element.pattern::<IUIAutomationInvokePattern>(UIA_INVOKE_PATTERN) {
            return pattern.Invoke().ok().map_err(|err| err.to_string());
        }
        if let Ok(pattern) = element.pattern::<IUIAutomationTogglePattern>(UIA_TOGGLE_PATTERN) {
            return pattern.Toggle().ok().map_err(|err| err.to_string());
        }
        if let Ok(pattern) =
            element.pattern::<IUIAutomationSelectionItemPattern>(UIA_SELECTION_ITEM_PATTERN)
        {
            return pattern.Select().ok().map_err(|err| err.to_string());
        }
    }
    Err("That control cannot be pressed this way.".into())
}

fn virtual_key(key: &str) -> Option<VIRTUAL_KEY> {
    Some(match key {
        "enter" | "return" => VK_RETURN,
        "tab" => VK_TAB,
        "esc" | "escape" => VK_ESCAPE,
        "space" => VK_SPACE,
        "backspace" => VK_BACK,
        "delete" | "del" => VK_DELETE,
        "up" => VK_UP,
        "down" => VK_DOWN,
        "left" => VK_LEFT,
        "right" => VK_RIGHT,
        "home" => VK_HOME,
        "end" => VK_END,
        "pageup" => VK_PRIOR,
        "pagedown" => VK_NEXT,
        _ => {
            if let Some(number) = key.strip_prefix('f').and_then(|n| n.parse::<u16>().ok())
                && (1..=24).contains(&number)
            {
                return Some(VIRTUAL_KEY(VK_F1.0 + number - 1));
            }
            let mut chars = key.chars();
            let (Some(only), None) = (chars.next(), chars.next()) else {
                return None;
            };
            if only.is_ascii_alphanumeric() {
                VIRTUAL_KEY(only.to_ascii_uppercase() as u16)
            } else {
                return None;
            }
        }
    })
}

fn key_event(key: VIRTUAL_KEY, up: bool) -> INPUT {
    // SAFETY: only maps a key's number.
    let scan = unsafe { MapVirtualKeyW(u32::from(key.0), MAPVK_VK_TO_VSC) } as u16;
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: scan,
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

fn send_combo(combo: &KeyCombo) -> Result<()> {
    let key = virtual_key(&combo.key).ok_or_else(|| format!("“{}” is not a key.", combo.key))?;
    let held: Vec<VIRTUAL_KEY> = [
        (combo.ctrl, VK_CONTROL),
        (combo.shift, VK_SHIFT),
        (combo.alt, VK_LMENU),
        (combo.meta, VK_LWIN),
    ]
    .into_iter()
    .filter_map(|(on, key)| on.then_some(key))
    .collect();
    let mut inputs: Vec<INPUT> = held.iter().map(|key| key_event(*key, false)).collect();
    inputs.push(key_event(key, false));
    inputs.push(key_event(key, true));
    inputs.extend(held.iter().rev().map(|key| key_event(*key, true)));
    // SAFETY: the slice and the size go together.
    let sent = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
    if sent as usize == inputs.len() {
        Ok(())
    } else {
        Err("Windows did not take the keys; something may be blocking input.".into())
    }
}

/// Start Menu shortcuts for this user and for everyone.
fn start_menu_apps() -> Vec<(String, std::path::PathBuf)> {
    let mut found = Vec::new();
    let folders = [
        std::env::var_os("APPDATA").map(|dir| {
            std::path::PathBuf::from(dir).join("Microsoft\\Windows\\Start Menu\\Programs")
        }),
        std::env::var_os("ProgramData").map(|dir| {
            std::path::PathBuf::from(dir).join("Microsoft\\Windows\\Start Menu\\Programs")
        }),
    ];
    let mut pending: Vec<std::path::PathBuf> = folders.into_iter().flatten().collect();
    while let Some(folder) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("lnk"))
            {
                let name = path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                found.push((name, path));
            }
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

/// Hands `target` to the shell: an app's shortcut, a document, a folder or an address.
pub(crate) fn shell_open(target: &std::path::Path) -> Result<()> {
    // Explorer would read it as one of its switches.
    if target.to_string_lossy().starts_with(['/', '-']) {
        return Err("Windows could not open that.".into());
    }
    let system = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    std::process::Command::new(std::path::Path::new(&system).join("explorer.exe"))
        .arg(target)
        .creation_flags(0x0800_0000)
        .spawn()
        .map(drop)
        .map_err(|err| format!("Windows could not open that: {err}"))
}

impl AppControlProvider for AppControl {
    fn availability(&self) -> Availability {
        let _com = ComScope::enter();
        match automation() {
            Ok(_) => Availability::Available,
            Err(why) => Availability::Unsupported(why),
        }
    }

    fn windows(&self) -> Result<Vec<WindowInfo>> {
        // SAFETY: only asks which window is in front.
        let front = unsafe { GetForegroundWindow() };
        let mine = std::process::id();
        Ok(top_windows()?
            .into_iter()
            .filter(|window| {
                let mut id = 0;
                // SAFETY: only asks about a window Windows just named.
                unsafe { GetWindowThreadProcessId(*window, Some(&mut id)) };
                id != mine
            })
            .map(|window| WindowInfo {
                id: window_id(window),
                app: app_of(window),
                title: title_of(window),
                focused: window == front,
            })
            .collect())
    }

    fn read_window(&self, window: &str, limit: usize) -> Result<Vec<UiElement>> {
        let _com = ComScope::enter();
        let window = find_window(window)?;
        Ok(controls(window, limit)?
            .into_iter()
            .map(|found| found.info)
            .collect())
    }

    fn focus_window(&self, window: &str) -> Result<()> {
        bring_to_front(find_window(window)?)
    }

    fn close_window(&self, window: &str) -> Result<()> {
        let window = find_window(window)?;
        // SAFETY: a message to a window Windows named; the window decides what to do with it.
        unsafe { PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0)) }
            .map_err(|err| format!("Windows would not close it: {err}"))
    }

    fn click(&self, window: &str, target: &str) -> Result<String> {
        let _com = ComScope::enter();
        let window = find_window(window)?;
        let found = controls(window, 2000)?;
        let control = pick(&found, target, "control")?;
        if control.info.secret {
            return Err(SECRET_FIELD.into());
        }
        if !control.info.enabled {
            return Err(format!("“{}” is greyed out right now.", control.info.name));
        }
        press(&control.element)?;
        Ok(format!("Pressed “{}”.", control.info.name))
    }

    fn set_text(&self, window: &str, target: &str, text: &str) -> Result<String> {
        let _com = ComScope::enter();
        let window = find_window(window)?;
        let found = controls(window, 2000)?;
        let fields: Vec<&Found> = found
            .iter()
            .filter(|f| matches!(f.info.role.as_str(), "field" | "document" | "combobox"))
            .collect();
        let control = if target.trim().is_empty() {
            match fields.as_slice() {
                [only] => *only,
                [] => return Err("That window has no field to type in.".into()),
                _ => return Err("That window has several fields: say which.".into()),
            }
        } else {
            let infos: Vec<UiElement> = fields.iter().map(|f| f.info.clone()).collect();
            let picked = pick_element(&infos, target, "field")?;
            let at = infos
                .iter()
                .position(|info| std::ptr::eq(info, picked))
                .unwrap_or(0);
            fields[at]
        };
        if control.info.secret {
            return Err(SECRET_FIELD.into());
        }
        // SAFETY: COM calls on an element held in `found`.
        unsafe {
            control
                .element
                .pattern::<IUIAutomationValuePattern>(UIA_VALUE_PATTERN)
                .map_err(|_| "That field does not take text this way.".to_owned())?
                .set_value(text)
                .map_err(|err| format!("That field would not take the text: {err}"))?;
        }
        Ok(format!("Typed into “{}”.", control.info.name))
    }

    fn press_keys(&self, window: &str, keys: &str) -> Result<()> {
        let combo =
            KeyCombo::parse(keys).ok_or_else(|| format!("“{keys}” is not a key combination."))?;
        if !window.trim().is_empty() {
            bring_to_front(find_window(window)?)?;
        }
        send_combo(&combo)
    }

    fn open_app(&self, name: &str) -> Result<String> {
        let apps = start_menu_apps();
        let lower = name.trim().to_lowercase();
        if lower.is_empty() {
            return Err("Say which app.".into());
        }
        let exact = apps.iter().find(|(app, _)| app.to_lowercase() == lower);
        let found = exact.or_else(|| {
            let close: Vec<&(String, std::path::PathBuf)> =
                apps.iter().filter(|(app, _)| holds(app, name)).collect();
            (close.len() == 1).then(|| close[0])
        });
        let Some((app, shortcut)) = found else {
            let close: Vec<&str> = apps
                .iter()
                .filter(|(app, _)| holds(app, name))
                .take(6)
                .map(|(app, _)| app.as_str())
                .collect();
            return Err(if close.is_empty() {
                format!("No app called “{name}” is in the Start Menu.")
            } else {
                format!(
                    "More than one app could be “{name}”: {}. Say which.",
                    close.join(", ")
                )
            });
        };
        shell_open(shortcut)?;
        Ok(format!("Opened {app}."))
    }

    fn find_apps(&self, terms: &[String]) -> Result<Vec<String>> {
        let mut names: Vec<String> = start_menu_apps()
            .into_iter()
            .map(|(app, _)| app)
            .filter(|app| terms.is_empty() || terms.iter().any(|term| holds(app, term)))
            .collect();
        names.dedup();
        Ok(names)
    }

    fn open_path(&self, target: &str) -> Result<()> {
        shell_open(std::path::Path::new(target))
    }

    fn system(&self, command: &keenu_platform::SystemCommand) -> Result<String> {
        crate::machine::run(command)
    }

    fn report(&self, report: keenu_platform::Report) -> Result<String> {
        crate::machine::report(report)
    }

    fn capture_screen(&self) -> Result<keenu_platform::ScreenPicture> {
        crate::machine::capture()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_keys_as_windows_knows_them() {
        assert_eq!(virtual_key("enter"), Some(VK_RETURN));
        assert_eq!(virtual_key("f5"), Some(VIRTUAL_KEY(VK_F1.0 + 4)));
        assert_eq!(virtual_key("s"), Some(VIRTUAL_KEY(b'S' as u16)));
        assert_eq!(virtual_key("7"), Some(VIRTUAL_KEY(b'7' as u16)));
        assert_eq!(virtual_key("f99"), None);
        assert_eq!(virtual_key("nonsense"), None);
    }

    #[test]
    fn a_window_nobody_has_open_is_an_error() {
        assert!(find_window("no window is called this").is_err());
    }

    #[test]
    fn windows_are_listed() {
        // A test runner has at least its desktop's windows, or none at all; never an error.
        assert!(AppControl.windows().is_ok());
    }

    #[test]
    fn the_start_menu_can_be_searched() {
        assert!(
            AppControl
                .find_apps(&["no app is called this".into()])
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn asking_for_no_app_says_so() {
        assert!(AppControl.open_app(" ").is_err());
    }

    #[test]
    fn ui_automation_starts() {
        assert_eq!(AppControl.availability(), Availability::Available);
    }

    #[test]
    fn it_never_presses_a_password_field() {
        // Checked on the info alone: the real field is exercised by the smoke test.
        let secret = UiElement {
            secret: true,
            name: "Password".into(),
            ..UiElement::default()
        };
        assert!(
            pick_element(std::slice::from_ref(&secret), "password", "field")
                .unwrap()
                .secret
        );
    }
}
