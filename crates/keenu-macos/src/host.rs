// SPDX-License-Identifier: GPL-3.0-or-later

//! The island as a borderless panel at the top of the screen, painted from the shared scene.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::Arc;

use keenu_app::app::Waker;
use keenu_app::data::Paths;
use keenu_app::{App, Effect, Platform};
use keenu_ui::icons::polygons;
use keenu_ui::scene::{Align, Color, Op, Rect, Scene};
use keenu_ui::view::Action;
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AllocAnyThread, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSBackingStoreType,
    NSBezierPath, NSColor, NSEvent, NSFont, NSFontAttributeName, NSForegroundColorAttributeName,
    NSImage, NSLineBreakMode, NSMenu, NSMenuItem, NSMutableParagraphStyle, NSPanel,
    NSParagraphStyleAttributeName, NSScreen, NSStatusBar, NSStatusItem, NSStringDrawing,
    NSTextAlignment, NSView, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{
    MainThreadMarker, NSAttributedStringKey, NSData, NSDictionary, NSNotification, NSPoint, NSRect,
    NSSize, NSString, NSTimer,
};

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

struct State {
    app: App,
    scene: Scene<Action>,
    panel: Retained<NSPanel>,
    view: Retained<IslandView>,
    /// Keeps the menu bar item alive.
    _status_item: Retained<NSStatusItem>,
    images: HashMap<String, Retained<NSImage>>,
    /// The assistant's shortcut while it is held; null otherwise.
    hotkey: *mut c_void,
    /// Whether the shortcut was last asked for, so a refusal is not retried every second.
    hotkey_wanted: bool,
}

/// macOS measures from the bottom of the screen; the scene from the top.
fn flipped(rect: Rect, height: f32) -> NSRect {
    NSRect::new(
        NSPoint::new(f64::from(rect.x), f64::from(height - rect.y - rect.h)),
        NSSize::new(f64::from(rect.w), f64::from(rect.h)),
    )
}

fn ns_color(color: Color) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(
        f64::from(color.r) / 255.0,
        f64::from(color.g) / 255.0,
        f64::from(color.b) / 255.0,
        f64::from(color.a) / 255.0,
    )
}

fn draw_rounded(rect: NSRect, radius: f32, color: Color) {
    let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
        rect,
        f64::from(radius),
        f64::from(radius),
    );
    ns_color(color).setFill();
    path.fill();
}

fn draw_op(
    op: &Op,
    height: f32,
    images: &mut HashMap<String, Retained<NSImage>>,
    sources: &HashMap<String, Arc<Vec<u8>>>,
) {
    match op {
        Op::Fill {
            rect,
            radius,
            color,
        } => draw_rounded(flipped(*rect, height), *radius, *color),
        Op::Island {
            rect,
            radius,
            color,
        } => {
            // Reaching above the top edge: only the bottom corners show.
            let tall = Rect::new(rect.x, rect.y - radius * 2.0, rect.w, rect.h + radius * 2.0);
            draw_rounded(flipped(tall, height), *radius, *color);
        }
        Op::Outline {
            rect,
            radius,
            color,
            width,
        } => {
            let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                flipped(*rect, height),
                f64::from(*radius),
                f64::from(*radius),
            );
            path.setLineWidth(f64::from(*width));
            ns_color(*color).setStroke();
            path.stroke();
        }
        Op::Text { rect, text, style } => {
            let font = if style.mono {
                NSFont::userFixedPitchFontOfSize(f64::from(style.size))
                    .unwrap_or_else(|| NSFont::systemFontOfSize(f64::from(style.size)))
            } else if style.bold {
                NSFont::boldSystemFontOfSize(f64::from(style.size))
            } else {
                NSFont::systemFontOfSize(f64::from(style.size))
            };
            let paragraph = NSMutableParagraphStyle::new();
            paragraph.setAlignment(match style.align {
                Align::Left => NSTextAlignment::Left,
                Align::Center => NSTextAlignment::Center,
                Align::Right => NSTextAlignment::Right,
            });
            paragraph.setLineBreakMode(if style.wrap {
                NSLineBreakMode::ByWordWrapping
            } else {
                NSLineBreakMode::ByTruncatingTail
            });
            let color = ns_color(style.color);
            // SAFETY: the keys are AppKit's constants, and the values are the types they call for.
            let attributes = unsafe {
                let keys: [&NSAttributedStringKey; 3] = [
                    NSFontAttributeName,
                    NSForegroundColorAttributeName,
                    NSParagraphStyleAttributeName,
                ];
                let values: [&AnyObject; 3] = [font.as_ref(), color.as_ref(), paragraph.as_ref()];
                NSDictionary::from_slices(&keys, &values)
            };
            let string = NSString::from_str(text);
            // Single lines are centred in their box, as on Windows.
            let mut target = flipped(*rect, height);
            if !style.wrap {
                let line = f64::from(style.size) * 1.3;
                target.origin.y += (target.size.height - line) / 2.0;
                target.size.height = line;
            }
            // SAFETY: drawing into the current context, inside `drawRect:`.
            unsafe { string.drawInRect_withAttributes(target, Some(&attributes)) };
        }
        Op::Icon { rect, icon, color } => {
            let scale = f64::from(rect.w) / 24.0;
            ns_color(*color).setFill();
            for polygon in polygons(*icon) {
                let path = NSBezierPath::bezierPath();
                for (index, (x, y)) in polygon.iter().enumerate() {
                    let point = NSPoint::new(
                        f64::from(rect.x) + f64::from(*x) * scale,
                        f64::from(height) - f64::from(rect.y) - f64::from(*y) * scale,
                    );
                    if index == 0 {
                        path.moveToPoint(point);
                    } else {
                        path.lineToPoint(point);
                    }
                }
                path.closePath();
                path.fill();
            }
        }
        Op::Image { rect, radius, key } => {
            if !images.contains_key(key)
                && let Some(bytes) = sources.get(key)
            {
                let data = NSData::with_bytes(bytes);
                if let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) {
                    images.insert(key.clone(), image);
                }
            }
            let target = flipped(*rect, height);
            if let Some(image) = images.get(key) {
                let path = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(
                    target,
                    f64::from(*radius),
                    f64::from(*radius),
                );
                path.addClip();
                image.drawInRect(target);
                NSBezierPath::clipRect(flipped(Rect::new(0.0, 0.0, 10_000.0, 10_000.0), height));
            } else {
                draw_rounded(target, *radius, keenu_ui::scene::palette::BUTTON);
            }
        }
    }
}

define_class!(
    // SAFETY: NSView has no subclassing requirements beyond these overrides.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "KeenuIslandView"]
    struct IslandView;

    impl IslandView {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            STATE.with(|state| {
                let mut state = state.borrow_mut();
                let Some(state) = state.as_mut() else {
                    return;
                };
                let height = state.scene.height;
                let sources = state.app.images().clone();
                for op in &state.scene.ops {
                    draw_op(op, height, &mut state.images, &sources);
                }
            });
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            let point = event.locationInWindow();
            click(point.x as f32, point.y as f32);
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {}

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(isOpaque))]
        fn is_opaque(&self) -> bool {
            false
        }
    }
);

define_class!(
    // SAFETY: a plain delegate object, used on the main thread.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "KeenuDelegate"]
    struct Delegate;

    // SAFETY: NSObject implements the protocol.
    unsafe impl NSObjectProtocol for Delegate {}

    // SAFETY: only methods of the protocol are implemented.
    unsafe impl NSApplicationDelegate for Delegate {
        #[unsafe(method(applicationWillTerminate:))]
        fn will_terminate(&self, _notification: &NSNotification) {
            STATE.with(|state| {
                if let Some(state) = state.borrow_mut().as_mut() {
                    state.app.shut_down();
                }
            });
        }
    }

    impl Delegate {
        #[unsafe(method(tick:))]
        fn tick(&self, _timer: &NSTimer) {
            STATE.with(|state| {
                if let Some(state) = state.borrow_mut().as_mut() {
                    state.app.tick();
                    sync_hotkey(state);
                }
            });
            refresh();
        }

        #[unsafe(method(showIsland:))]
        fn show_island(&self, _sender: Option<&AnyObject>) {
            STATE.with(|state| {
                if let Some(state) = state.borrow_mut().as_mut() {
                    state.app.set_expanded(true);
                }
            });
            refresh();
        }
    }
);

impl Delegate {
    fn new(main: MainThreadMarker) -> Retained<Self> {
        // SAFETY: NSObject's `init` takes no arguments.
        unsafe { msg_send![Self::alloc(main), init] }
    }
}

fn click(x: f32, y: f32) {
    let mut quit = false;
    STATE.with(|state| {
        if let Some(state) = state.borrow_mut().as_mut() {
            let from_top = state.scene.height - y;
            if let Some(action) = state
                .scene
                .hit_at(x, from_top)
                .map(|hit| hit.action.clone())
            {
                quit = state.app.act(action) == Effect::Quit;
            }
        }
    });
    if quit {
        let main = MainThreadMarker::new().expect("clicks arrive on the main thread");
        NSApplication::sharedApplication(main).terminate(None);
        return;
    }
    refresh();
}

/// Puts the current state on screen, at the right size and place.
fn refresh() {
    let Some(main) = MainThreadMarker::new() else {
        return;
    };
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(state) = state.as_mut() else {
            return;
        };
        state.scene = state.app.scene();
        let (width, height) = (f64::from(state.scene.width), f64::from(state.scene.height));
        if let Some(screen) = NSScreen::mainScreen(main) {
            // visibleFrame stops below the menu bar, so the island hangs from it.
            let area = screen.visibleFrame();
            let origin = NSPoint::new(
                area.origin.x + (area.size.width - width) / 2.0,
                area.origin.y + area.size.height - height,
            );
            state
                .panel
                .setFrame_display(NSRect::new(origin, NSSize::new(width, height)), true);
        }
        state.view.setNeedsDisplay(true);
    });
}

/// Timers fire on the main run loop, which is where this runs; workers' news waits for the next tick.
struct TickWaker;

impl Waker for TickWaker {
    fn wake(&self) {}
}

/// Carbon's global shortcuts: unlike key monitors, they ask for no Accessibility permission.
#[repr(C)]
struct EventTypeSpec {
    class: u32,
    kind: u32,
}

#[repr(C)]
struct EventHotKeyId {
    signature: u32,
    id: u32,
}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn GetApplicationEventTarget() -> *mut c_void;
    fn InstallEventHandler(
        target: *mut c_void,
        handler: extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> i32,
        count: usize,
        kinds: *const EventTypeSpec,
        user_data: *mut c_void,
        out: *mut *mut c_void,
    ) -> i32;
    fn RegisterEventHotKey(
        code: u32,
        modifiers: u32,
        id: EventHotKeyId,
        target: *mut c_void,
        options: u32,
        out: *mut *mut c_void,
    ) -> i32;
    fn UnregisterEventHotKey(hotkey: *mut c_void) -> i32;
}

/// 'keyb', kEventHotKeyPressed; Space, and the Option key.
const EVENT_CLASS_KEYBOARD: u32 = 0x6b65_7962;
const EVENT_HOTKEY_PRESSED: u32 = 5;
const KEY_SPACE: u32 = 49;
const OPTION_KEY: u32 = 1 << 11;

extern "C" fn hotkey_pressed(_: *mut c_void, _: *mut c_void, _: *mut c_void) -> i32 {
    STATE.with(|state| {
        if let Some(state) = state.borrow_mut().as_mut() {
            state.app.summon();
        }
    });
    refresh();
    0
}

/// Listens for the shortcut once; whether it is held is up to `sync_hotkey`.
fn install_hotkey_handler() {
    let kinds = EventTypeSpec {
        class: EVENT_CLASS_KEYBOARD,
        kind: EVENT_HOTKEY_PRESSED,
    };
    let mut handler = std::ptr::null_mut();
    // SAFETY: the handler lives as long as the program; the call runs on the main thread and copies `kinds`.
    unsafe {
        InstallEventHandler(
            GetApplicationEventTarget(),
            hotkey_pressed,
            1,
            &kinds,
            std::ptr::null_mut(),
            &mut handler,
        );
    }
}

/// Option+Space opens the assistant. It is held only while the assistant is on, as it takes the keys from every other
/// app, where they type a non-breaking space.
fn sync_hotkey(state: &mut State) {
    let wanted = state.app.settings.assistant.enabled;
    if wanted == state.hotkey_wanted {
        return;
    }
    state.hotkey_wanted = wanted;
    // SAFETY: on the main thread; `state.hotkey` is only ever what `RegisterEventHotKey` gave.
    unsafe {
        if wanted {
            let id = EventHotKeyId {
                signature: 0x6b65_6e75,
                id: 1,
            };
            let mut hotkey = std::ptr::null_mut();
            let target = GetApplicationEventTarget();
            if RegisterEventHotKey(KEY_SPACE, OPTION_KEY, id, target, 0, &mut hotkey) == 0 {
                state.hotkey = hotkey;
            } else {
                log::warn!("another program holds Option+Space");
            }
        } else if !state.hotkey.is_null() {
            UnregisterEventHotKey(state.hotkey);
            state.hotkey = std::ptr::null_mut();
        }
    }
}

/// Runs the island until it is quit.
pub fn run(platform: Platform, paths: Paths) {
    let main = MainThreadMarker::new().expect("the program starts on the main thread");
    autoreleasepool(|_| {
        let application = NSApplication::sharedApplication(main);
        // No Dock icon: it lives in the menu bar.
        application.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        let delegate = Delegate::new(main);
        application.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));

        let mut app = App::new(platform, paths, Arc::new(TickWaker));
        app.start_workers();
        app.start_listening();
        let scene = app.scene();

        let frame = NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(f64::from(scene.width), f64::from(scene.height)),
        );
        // A plain borderless panel, kept alive in `STATE`.
        let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(main),
            frame,
            NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
            NSBackingStoreType::Buffered,
            false,
        );
        panel.setOpaque(false);
        panel.setBackgroundColor(Some(&NSColor::clearColor()));
        panel.setHasShadow(false);
        // Above the menu bar's level, and on every Space and beside fullscreen apps.
        panel.setLevel(25);
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary,
        );

        // SAFETY: NSView's `init` for a view without a frame yet.
        let view: Retained<IslandView> = unsafe { msg_send![IslandView::alloc(main), init] };
        panel.setContentView(Some(&view));
        panel.orderFrontRegardless();

        // The menu bar item, with a way back and a way out.
        let status_bar = NSStatusBar::systemStatusBar();
        let status_item = status_bar.statusItemWithLength(-1.0);
        if let Some(button) = status_item.button(main) {
            button.setTitle(&NSString::from_str("Keenu"));
        }
        let menu = NSMenu::new(main);
        // SAFETY: the items' actions exist on their targets; both outlive the menu.
        unsafe {
            let show = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(main),
                &NSString::from_str("Show Keenu"),
                Some(sel!(showIsland:)),
                &NSString::from_str(""),
            );
            show.setTarget(Some(&*delegate));
            menu.addItem(&show);
            let quit = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(main),
                &NSString::from_str("Quit Keenu"),
                Some(sel!(terminate:)),
                &NSString::from_str("q"),
            );
            menu.addItem(&quit);
        }
        status_item.setMenu(Some(&menu));

        STATE.with(|state| {
            *state.borrow_mut() = Some(State {
                app,
                scene,
                panel,
                view,
                _status_item: status_item,
                images: HashMap::new(),
                hotkey: std::ptr::null_mut(),
                hotkey_wanted: false,
            });
        });
        refresh();
        install_hotkey_handler();

        // SAFETY: the target and selector exist, and the timer runs on this run loop.
        unsafe {
            let _timer = NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                1.0,
                &delegate,
                sel!(tick:),
                None,
                true,
            );
        }
        application.run();
    });
}
