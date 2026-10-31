// SPDX-License-Identifier: GPL-3.0-or-later

//! The window: where the island sits, what it does on a click or a key, and everything Windows may do
//! around it, such as displays coming and going, sleep, Explorer restarting and the session ending.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use keenu_app::app::Waker;
use keenu_app::data::Paths;
use keenu_app::{App, Effect, Key, Platform};
use keenu_core::paths::APP_USER_MODEL_ID;
use keenu_platform::WindowLifecycleProvider;
use keenu_ui::view::{self, Action, Page};
use windows::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM,
};
use windows::Win32::Graphics::Gdi::ValidateRect;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, RemoveClipboardFormatListener,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::RemoteDesktop::{
    NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification, WTSUnRegisterSessionNotification,
};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetFocus, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent, VK_BACK,
    VK_ESCAPE, VK_RETURN, VK_SHIFT, VK_TAB,
};
use windows::Win32::UI::Shell::{
    DragAcceptFiles, DragFinish, DragQueryFileW, HDROP, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD,
    NIM_DELETE, NOTIFYICONDATAW, SetCurrentProcessExplicitAppUserModelID, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows_core::{HSTRING, PCWSTR, w};

use crate::paint::{Painter, Surface};
use crate::system::{self, Display};

/// Not in the crate's `WindowsAndMessaging`, where the rest of these are.
const WM_MOUSELEAVE: u32 = 0x02A3;
const WM_WTSSESSION_CHANGE: u32 = 0x02B1;
const WM_APP_WAKE: u32 = WM_APP + 1;
const WM_APP_TRAY: u32 = WM_APP + 2;
/// Sent by a second copy of Keenu: show the one already running.
const WM_APP_SHOW: u32 = WM_APP + 3;

const TIMER_TICK: usize = 1;
const TIMER_HOVER_OPEN: usize = 2;
const TIMER_HOVER_CLOSE: usize = 3;
const HOVER_OPEN_MS: u32 = 280;
const HOVER_CLOSE_MS: u32 = 500;

const MENU_SHOW: usize = 100;
const MENU_SETTINGS: usize = 101;
const MENU_QUIT: usize = 102;

const WTS_SESSION_LOCK: usize = 0x7;
const WTS_SESSION_UNLOCK: usize = 0x8;
const PBT_APMSUSPEND: usize = 0x4;
const PBT_APMRESUMEAUTOMATIC: usize = 0x12;
const PBT_APMRESUMESUSPEND: usize = 0x7;

const CLASS_NAME: PCWSTR = w!("KeenuIslandWindow");
const MUTEX_NAME: PCWSTR = w!("Local\\Keenu.Desktop.Singleton");

/// Wakes the UI thread from anywhere.
struct WindowWaker(isize);

impl Waker for WindowWaker {
    fn wake(&self) {
        // SAFETY: posting to a window that may have closed simply fails.
        unsafe {
            let _ = PostMessageW(
                Some(HWND(self.0 as *mut _)),
                WM_APP_WAKE,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

struct Host {
    window: HWND,
    app: App,
    painter: Painter,
    surface: Option<Surface>,
    scene: keenu_ui::scene::Scene<Action>,
    /// Where the window is, physically.
    origin: (i32, i32),
    display: Option<Display>,
    taskbar_created: u32,
    tracking_mouse: bool,
    hidden_for_fullscreen: bool,
    hidden_by_user: bool,
    dump_frames_to: Option<PathBuf>,
    /// For automated tests on a desktop in use: only the keyboard counts, not a person's clicks.
    ignore_pointer: bool,
    lifecycle: Arc<dyn WindowLifecycleProvider>,
}

thread_local! {
    static HOST: RefCell<Option<Rc<RefCell<Host>>>> = const { RefCell::new(None) };
}

/// A key the host passes on.
fn key_of(virtual_key: u16) -> Option<Key> {
    match virtual_key {
        k if k == VK_ESCAPE.0 => Some(Key::Escape),
        k if k == VK_RETURN.0 => Some(Key::Enter),
        k if k == VK_BACK.0 => Some(Key::Backspace),
        k if k == VK_TAB.0 => {
            // SAFETY: only reads the key's state.
            let shifted = unsafe { GetKeyState(i32::from(VK_SHIFT.0)) } < 0;
            Some(if shifted { Key::ShiftTab } else { Key::Tab })
        }
        _ => None,
    }
}

impl Host {
    fn scale(&self) -> f32 {
        self.display
            .as_ref()
            .map_or(1.0, |display| display.dpi as f32 / 96.0)
    }

    /// Finds the display in use again: after a change of displays, or of settings.
    fn refresh_display(&mut self) {
        let list = system::displays();
        self.display = system::choose_display(&list, self.app.monitor_name()).cloned();
    }

    /// Draws the current state and puts it on screen, at the right size and place.
    fn repaint(&mut self) {
        self.scene = self.app.scene();
        let scale = self.scale();
        let width = (self.scene.width * scale).ceil() as i32;
        let height = (self.scene.height * scale).ceil() as i32;
        if self
            .surface
            .as_ref()
            .is_none_or(|surface| surface.width != width || surface.height != height)
        {
            self.surface = Surface::new(width, height).ok();
        }
        let Some(surface) = &self.surface else {
            return;
        };
        self.painter.forget_images(self.app.images());
        if let Err(err) = self
            .painter
            .render(&self.scene, scale, self.app.images(), surface)
        {
            log::warn!("could not draw: {}", err.message());
            return;
        }
        if let Some(display) = &self.display {
            self.origin = system::place(display, width);
        }
        if let Some(path) = &self.dump_frames_to
            && let Some(png) = surface.to_png()
        {
            let _ = std::fs::write(path, png);
        }
        let _ = surface.present(self.window, self.origin.0, self.origin.1);
    }

    /// Keeps the window above others without taking focus, shown or hidden as it should be.
    fn sync_visibility(&mut self) {
        let fullscreen = self.lifecycle.fullscreen_active() && self.app.may_hide_for_fullscreen();
        self.hidden_for_fullscreen = fullscreen;
        let hide = (fullscreen || self.hidden_by_user) && !self.app.holds_open();
        // SAFETY: the window is ours.
        unsafe {
            if hide {
                let _ = ShowWindow(self.window, SW_HIDE);
            } else {
                let _ = SetWindowPos(
                    self.window,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
            }
        }
    }

    fn add_tray_icon(&self) {
        // SAFETY: the structure is filled in here and copied by the call; a failure leaves only no icon.
        unsafe {
            let mut data = tray_data(self.window);
            data.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
            data.uCallbackMessage = WM_APP_TRAY;
            data.hIcon = load_app_icon();
            let tip: Vec<u16> = "Keenu".encode_utf16().collect();
            data.szTip[..tip.len()].copy_from_slice(&tip);
            let _ = Shell_NotifyIconW(NIM_ADD, &data);
        }
    }

    fn remove_tray_icon(&self) {
        // SAFETY: names the icon added above.
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &tray_data(self.window));
        }
    }

    fn show_tray_menu(&mut self) {
        // SAFETY: the menu is local and destroyed after it closes.
        unsafe {
            let Ok(menu) = CreatePopupMenu() else {
                return;
            };
            let _ = AppendMenuW(menu, MF_STRING, MENU_SHOW, w!("Show Keenu"));
            let _ = AppendMenuW(menu, MF_STRING, MENU_SETTINGS, w!("Settings"));
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            let _ = AppendMenuW(menu, MF_STRING, MENU_QUIT, w!("Quit Keenu"));
            let mut point = POINT::default();
            let _ = GetCursorPos(&mut point);
            // Windows needs this for the menu to close when the user clicks elsewhere.
            let _ = SetForegroundWindow(self.window);
            let _ = TrackPopupMenu(
                menu,
                TPM_RIGHTBUTTON,
                point.x,
                point.y,
                None,
                self.window,
                None,
            );
            let _ = PostMessageW(Some(self.window), WM_NULL, WPARAM(0), LPARAM(0));
            let _ = DestroyMenu(menu);
        }
    }

    fn start_tracking_mouse(&mut self) {
        if self.tracking_mouse {
            return;
        }
        self.tracking_mouse = true;
        let mut track = TRACKMOUSEEVENT {
            cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE,
            hwndTrack: self.window,
            dwHoverTime: 0,
        };
        // SAFETY: `track` describes this window.
        unsafe {
            let _ = TrackMouseEvent(&mut track);
        }
    }

    /// Carries out a click or menu choice. False when Keenu is to quit.
    fn perform(&mut self, action: Action) -> bool {
        if matches!(action, Action::SelectMonitor) {
            let result = self.app.act(action);
            self.refresh_display();
            self.repaint();
            return result != Effect::Quit;
        }
        let result = self.app.act(action);
        self.repaint();
        result != Effect::Quit
    }

    /// Asks for the window to close, once this message is done with.
    fn close(&mut self) {
        // SAFETY: the window is ours.
        unsafe {
            let _ = PostMessageW(Some(self.window), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }
}

fn tray_data(window: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: window,
        uID: 1,
        ..NOTIFYICONDATAW::default()
    }
}

/// Keenu's icon from the program's resources, or Windows' plain one if a build has none.
fn load_app_icon() -> HICON {
    // SAFETY: loads a shared icon, which is not to be freed.
    unsafe {
        let instance = GetModuleHandleW(None)
            .ok()
            .map(|module| HINSTANCE(module.0));
        LoadIconW(instance, PCWSTR(std::ptr::without_provenance::<u16>(1)))
            .or_else(|_| LoadIconW(None, IDI_APPLICATION))
            .unwrap_or_default()
    }
}

fn dropped_files(drop: HDROP) -> Vec<PathBuf> {
    let mut files = Vec::new();
    // SAFETY: `drop` is valid until `DragFinish`, called once below.
    unsafe {
        let count = DragQueryFileW(drop, u32::MAX, None);
        for index in 0..count {
            let length = DragQueryFileW(drop, index, None) as usize;
            let mut buffer = vec![0u16; length + 1];
            let copied = DragQueryFileW(drop, index, Some(&mut buffer)) as usize;
            files.push(PathBuf::from(String::from_utf16_lossy(&buffer[..copied])));
        }
        DragFinish(drop);
    }
    files
}

unsafe extern "system" fn window_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let host = HOST.with(|host| host.borrow().clone());
    // Some calls below make Windows send further messages at once; those take the default path.
    let Some(host) = host else {
        // SAFETY: the arguments are as received.
        return unsafe { DefWindowProcW(window, message, wparam, lparam) };
    };
    let Ok(mut host) = host.try_borrow_mut() else {
        // SAFETY: the arguments are as received.
        return unsafe { DefWindowProcW(window, message, wparam, lparam) };
    };
    // SAFETY: handles are ours, and message parameters are read as each message defines them.
    unsafe {
        if message == host.taskbar_created {
            // Explorer restarted: its tray has forgotten every icon.
            host.add_tray_icon();
            return LRESULT(0);
        }
        match message {
            WM_APP_WAKE => {
                if host.app.pump() {
                    host.repaint();
                    host.sync_visibility();
                }
                LRESULT(0)
            }
            WM_APP_SHOW => {
                host.hidden_by_user = false;
                host.app.set_expanded(true);
                host.repaint();
                host.sync_visibility();
                let _ = SetForegroundWindow(window);
                LRESULT(0)
            }
            WM_TIMER => {
                match wparam.0 {
                    TIMER_TICK => {
                        host.app.tick();
                        host.repaint();
                        host.sync_visibility();
                    }
                    TIMER_HOVER_OPEN => {
                        let _ = KillTimer(Some(window), TIMER_HOVER_OPEN);
                        host.app.set_expanded(true);
                        host.repaint();
                    }
                    TIMER_HOVER_CLOSE => {
                        let _ = KillTimer(Some(window), TIMER_HOVER_CLOSE);
                        if !host.app.holds_open() {
                            host.app.set_expanded(false);
                            host.repaint();
                        }
                    }
                    _ => {}
                }
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                host.start_tracking_mouse();
                let _ = KillTimer(Some(window), TIMER_HOVER_CLOSE);
                if !host.app.view().expanded {
                    SetTimer(Some(window), TIMER_HOVER_OPEN, HOVER_OPEN_MS, None);
                }
                let scale = host.scale();
                let (x, y) = (
                    lparam.0 as i16 as f32 / scale,
                    (lparam.0 >> 16) as i16 as f32 / scale,
                );
                let cursor = if host.scene.hit_at(x, y).is_some() {
                    IDC_HAND
                } else {
                    IDC_ARROW
                };
                if let Ok(cursor) = LoadCursorW(None, cursor) {
                    SetCursor(Some(cursor));
                }
                LRESULT(0)
            }
            WM_MOUSELEAVE => {
                host.tracking_mouse = false;
                let _ = KillTimer(Some(window), TIMER_HOVER_OPEN);
                if host.app.view().expanded && !host.app.holds_open() {
                    SetTimer(Some(window), TIMER_HOVER_CLOSE, HOVER_CLOSE_MS, None);
                }
                LRESULT(0)
            }
            WM_LBUTTONUP if host.ignore_pointer => LRESULT(0),
            WM_LBUTTONUP => {
                let _ = ReleaseCapture();
                let _ = KillTimer(Some(window), TIMER_HOVER_OPEN);
                let scale = host.scale();
                let (x, y) = (
                    lparam.0 as i16 as f32 / scale,
                    (lparam.0 >> 16) as i16 as f32 / scale,
                );
                // Takes the keyboard from here on: typing and Tab need it.
                let _ = SetForegroundWindow(window);
                let _ = SetFocus(Some(window));
                log::debug!("click at {x},{y}");
                if let Some(action) = host.scene.hit_at(x, y).map(|hit| hit.action.clone())
                    && !host.perform(action)
                {
                    host.close();
                }
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                let notches = (wparam.0 >> 16) as i16 / WHEEL_DELTA as i16;
                host.app.scroll(i32::from(notches));
                host.repaint();
                LRESULT(0)
            }
            WM_KEYDOWN => {
                log::debug!("key down {}", wparam.0);
                if let Some(key) = key_of(wparam.0 as u16) {
                    if host.app.key(key) == Effect::Quit {
                        host.close();
                        return LRESULT(0);
                    }
                    host.repaint();
                }
                LRESULT(0)
            }
            WM_CHAR => {
                if let Some(c) = char::from_u32(wparam.0 as u32).filter(|c| !c.is_control()) {
                    if host.app.key(Key::Char(c)) == Effect::Quit {
                        host.close();
                        return LRESULT(0);
                    }
                    host.repaint();
                }
                LRESULT(0)
            }
            WM_DROPFILES => {
                let files = dropped_files(HDROP(wparam.0 as *mut _));
                host.app.files_dropped(files);
                host.repaint();
                LRESULT(0)
            }
            WM_CLIPBOARDUPDATE => {
                host.app.clipboard_changed();
                host.repaint();
                LRESULT(0)
            }
            WM_DISPLAYCHANGE | WM_SETTINGCHANGE => {
                host.refresh_display();
                host.repaint();
                LRESULT(0)
            }
            WM_DPICHANGED => {
                host.refresh_display();
                host.surface = None;
                host.repaint();
                LRESULT(0)
            }
            WM_POWERBROADCAST => {
                match wparam.0 {
                    PBT_APMSUSPEND => host.app.set_session_active(false),
                    PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND => {
                        host.app.set_session_active(true);
                        host.refresh_display();
                        host.repaint();
                        host.sync_visibility();
                    }
                    _ => {}
                }
                LRESULT(1)
            }
            WM_WTSSESSION_CHANGE => {
                match wparam.0 {
                    WTS_SESSION_LOCK => host.app.set_session_active(false),
                    WTS_SESSION_UNLOCK => host.app.set_session_active(true),
                    _ => {}
                }
                LRESULT(0)
            }
            WM_APP_TRAY => {
                match lparam.0 as u32 {
                    WM_LBUTTONUP => {
                        host.hidden_by_user = !host.hidden_by_user;
                        host.sync_visibility();
                    }
                    WM_RBUTTONUP | WM_CONTEXTMENU => host.show_tray_menu(),
                    _ => {}
                }
                LRESULT(0)
            }
            WM_COMMAND => {
                match wparam.0 & 0xFFFF {
                    MENU_SHOW => {
                        host.hidden_by_user = false;
                        host.app.set_expanded(true);
                        host.repaint();
                        host.sync_visibility();
                    }
                    MENU_SETTINGS => {
                        host.hidden_by_user = false;
                        host.app.set_expanded(true);
                        host.perform(Action::SelectPage(Page::Settings));
                        host.sync_visibility();
                    }
                    MENU_QUIT => host.close(),
                    _ => {}
                }
                LRESULT(0)
            }
            WM_PAINT => {
                // A layered window keeps its own bitmap; there is nothing to do but say so.
                let _ = ValidateRect(Some(window), None);
                LRESULT(0)
            }
            WM_QUERYENDSESSION => LRESULT(1),
            WM_ENDSESSION => {
                if wparam.0 != 0 {
                    host.app.shut_down();
                }
                LRESULT(0)
            }
            WM_DESTROY => {
                let _ = RemoveClipboardFormatListener(window);
                let _ = WTSUnRegisterSessionNotification(window);
                host.remove_tray_icon();
                host.app.shut_down();
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => {
                // It may send messages back at once (closing does), and those must find the host free.
                drop(host);
                DefWindowProcW(window, message, wparam, lparam)
            }
        }
    }
}

/// Makes this the only Keenu in the session. `false` means another is running; it was told to show.
fn claim_single_instance() -> bool {
    // SAFETY: the handle is meant to stay open for the life of the process.
    unsafe {
        let mutex = CreateMutexW(None, false, MUTEX_NAME);
        if mutex.is_ok() && GetLastError() != ERROR_ALREADY_EXISTS {
            std::mem::forget(mutex);
            return true;
        }
        if let Ok(other) = FindWindowW(CLASS_NAME, None) {
            let _ = AllowSetForegroundWindow(ASFW_ANY);
            let _ = PostMessageW(Some(other), WM_APP_SHOW, WPARAM(0), LPARAM(0));
        }
        false
    }
}

/// Starts the agents' pipe, with a fresh token only this user can read.
fn start_agent_ipc(app: &App, paths: &Paths) {
    let Ok(token) = keenu_security::random_hex(32) else {
        return;
    };
    if let Err(err) = keenu_core::json_file::write_private(&paths.agent_token(), token.as_bytes()) {
        log::warn!("could not save the agent token: {err}");
        return;
    }
    let handle = app.handle();
    let served = crate::ipc::serve(token, move |incoming| {
        let request = incoming.request.clone();
        let shared = Arc::new(std::sync::Mutex::new(Some(incoming)));
        let for_reply = shared.clone();
        handle.approval(
            request,
            Box::new(move |answer| {
                if let Some(incoming) = for_reply.lock().ok().and_then(|mut held| held.take()) {
                    incoming.answer(&answer);
                }
            }),
            Box::new(move || {
                shared
                    .lock()
                    .ok()
                    .is_some_and(|held| held.as_ref().is_some_and(crate::ipc::Incoming::abandoned))
            }),
        );
    });
    if let Err(err) = served {
        log::warn!("the agent pipe could not start: {err}");
    }
}

/// Runs the island until it is quit.
pub fn run(platform: Platform, paths: Paths) -> Result<(), String> {
    if !claim_single_instance() {
        return Ok(());
    }
    // SAFETY: process-wide settings at start, before any window; the window's data lives in `HOST`
    // until the message loop has ended.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(APP_USER_MODEL_ID));
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let module = GetModuleHandleW(None).map_err(|err| err.message())?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: HINSTANCE(module.0),
            lpszClassName: CLASS_NAME,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hIcon: load_app_icon(),
            ..WNDCLASSW::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err("Could not register Keenu's window.".into());
        }
        let window = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_ACCEPTFILES,
            CLASS_NAME,
            w!("Keenu"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(HINSTANCE(module.0)),
            None,
        )
        .map_err(|err| err.message())?;

        let waker: Arc<dyn Waker> = Arc::new(WindowWaker(window.0 as isize));
        let lifecycle = platform.lifecycle.clone();
        let app = App::new(platform, paths.clone(), waker);
        start_agent_ipc(&app, &paths);
        app.start_workers();

        let painter =
            Painter::new().map_err(|err| format!("Could not start drawing: {}", err.message()))?;
        let host = Rc::new(RefCell::new(Host {
            window,
            scene: app.scene(),
            app,
            painter,
            surface: None,
            origin: (0, 0),
            display: None,
            taskbar_created: RegisterWindowMessageW(w!("TaskbarCreated")),
            tracking_mouse: false,
            hidden_for_fullscreen: false,
            hidden_by_user: false,
            dump_frames_to: std::env::var_os("KEENU_DUMP_FRAME").map(PathBuf::from),
            ignore_pointer: std::env::var_os("KEENU_TEST_IGNORE_POINTER").is_some(),
            lifecycle,
        }));
        {
            let mut held = host.borrow_mut();
            held.refresh_display();
            held.repaint();
            held.add_tray_icon();
            held.sync_visibility();
        }
        HOST.with(|slot| *slot.borrow_mut() = Some(host.clone()));

        let _ = AddClipboardFormatListener(window);
        let _ = WTSRegisterSessionNotification(window, NOTIFY_FOR_THIS_SESSION);
        DragAcceptFiles(window, true);
        SetTimer(Some(window), TIMER_TICK, 1000, None);

        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        HOST.with(|slot| *slot.borrow_mut() = None);
    }
    Ok(())
}

/// Renders a page of made-up content to a PNG, for checking the look without running the program.
pub fn render_sample(page: &str, out: &std::path::Path, scale: f32) -> Result<(), String> {
    // SAFETY: COM stays initialised for the rest of this short-lived process.
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    let view = crate::sample::view(page).ok_or_else(|| format!("No sample for {page}."))?;
    let scene = view::build(&view);
    let images = crate::sample::images();
    let mut painter = Painter::new().map_err(|err| err.message())?;
    let surface = Surface::new(
        (scene.width * scale).ceil() as i32,
        (scene.height * scale).ceil() as i32,
    )
    .map_err(|err| err.message())?;
    painter
        .render(&scene, scale, &images, &surface)
        .map_err(|err| err.message())?;
    let png = surface.to_png().ok_or("Could not encode the frame.")?;
    std::fs::write(out, png).map_err(|err| err.to_string())
}
