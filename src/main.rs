#![cfg(target_os = "macos")]
#![allow(deprecated, unexpected_cfgs)]

use cocoa::appkit::{
    NSApp, NSApplication, NSApplicationActivationPolicyAccessory, NSBackingStoreBuffered, NSColor,
    NSMenu, NSMenuItem, NSStatusBar, NSView, NSWindow, NSWindowStyleMask,
};
use cocoa::base::{id, nil, NO, YES};
use cocoa::foundation::{NSAutoreleasePool, NSPoint, NSRect, NSSize, NSString};
use objc::declare::ClassDecl;
use objc::runtime::{Class, Object, Sel};
use objc::{class, msg_send, sel, sel_impl};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::HashMap;
use std::env;
use std::ffi::CStr;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

const APP_NAME: &str = "Desktop Sticky Note";
const DEFAULT_W: f64 = 260.0;
const DEFAULT_H: f64 = 210.0;
const MIN_W: f64 = 180.0;
const MIN_H: f64 = 130.0;
const NOTE_YELLOW: (f64, f64, f64) = (1.0, 0.91, 0.42);
const NOTE_TEXT: (f64, f64, f64) = (0.08, 0.06, 0.02);

const K_CG_DESKTOP_ICON_WINDOW_LEVEL_KEY: i32 = 18;
const NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES: u64 = 1 << 0;
const NS_WINDOW_COLLECTION_BEHAVIOR_STATIONARY: u64 = 1 << 4;
const NS_WINDOW_COLLECTION_BEHAVIOR_IGNORES_CYCLE: u64 = 1 << 6;
const NS_WINDOW_COLLECTION_BEHAVIOR_FULL_SCREEN_AUXILIARY: u64 = 1 << 8;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGWindowLevelForKey(key: i32) -> i32;
}

#[derive(Clone, Serialize, Deserialize)]
struct Note {
    id: u64,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    text: String,
}

struct AppState {
    notes: Vec<Note>,
    windows: HashMap<usize, u64>,
    text_views: HashMap<usize, u64>,
    data_path: PathBuf,
    delegate: id,
    status_item: id,
    next_id: u64,
    saving: bool,
}

thread_local! {
    static STATE: RefCell<Option<AppState>> = const { RefCell::new(None) };
}

static DELEGATE_CLASS: OnceLock<&'static Class> = OnceLock::new();

fn main() {
    unsafe {
        let _pool = NSAutoreleasePool::new(nil);
        let app = NSApp();
        app.setActivationPolicy_(NSApplicationActivationPolicyAccessory);

        let delegate = msg_send![delegate_class(), new];
        app.setDelegate_(delegate);

        let data_path = data_path();
        let notes = load_notes(&data_path);
        let next_id = notes.iter().map(|note| note.id).max().unwrap_or(0) + 1;

        STATE.with(|state| {
            *state.borrow_mut() = Some(AppState {
                notes,
                windows: HashMap::new(),
                text_views: HashMap::new(),
                data_path,
                delegate,
                status_item: nil,
                next_id,
                saving: false,
            });
        });

        app.run();
    }
}

fn data_path() -> PathBuf {
    let home = env::var_os("HOME").expect("HOME must be set");
    PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join(APP_NAME)
        .join("notes.json")
}

fn load_notes(path: &PathBuf) -> Vec<Note> {
    match fs::read_to_string(path) {
        Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

fn save_notes() {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(state) = state.as_mut() else {
            return;
        };
        if state.saving {
            return;
        }
        state.saving = true;

        let result = (|| {
            if let Some(parent) = state.data_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let json = serde_json::to_string_pretty(&state.notes)?;
            fs::write(&state.data_path, json)
        })();

        state.saving = false;
        if let Err(error) = result {
            eprintln!("save failed: {error}");
        }
    });
}

fn with_state<F>(f: F)
where
    F: FnOnce(&mut AppState),
{
    STATE.with(|state| {
        if let Some(state) = state.borrow_mut().as_mut() {
            f(state);
        }
    });
}

fn app_did_finish_launching() {
    unsafe {
        setup_status_menu();

        let notes = STATE.with(|state| {
            state
                .borrow()
                .as_ref()
                .map(|state| state.notes.clone())
                .unwrap_or_default()
        });

        if notes.is_empty() {
            create_note(None);
        } else {
            for note in notes {
                show_note(note);
            }
        }
    }
}

unsafe fn setup_status_menu() {
    let status_item: id =
        msg_send![NSStatusBar::systemStatusBar(nil), statusItemWithLength: -1.0f64];
    let button: id = msg_send![status_item, button];
    let title = NSString::alloc(nil).init_str("Notes");
    let _: () = msg_send![button, setTitle: title];

    let menu = NSMenu::new(nil).autorelease();
    let new_item = menu_item("New Note", sel!(newNote:));
    let quit_item = menu_item("Quit", sel!(quit:));
    let _: () = msg_send![menu, addItem: new_item];
    let _: () = msg_send![menu, addItem: quit_item];
    let _: () = msg_send![status_item, setMenu: menu];

    with_state(|state| state.status_item = status_item);
}

unsafe fn menu_item(title: &str, action: Sel) -> id {
    let item = NSMenuItem::alloc(nil).initWithTitle_action_keyEquivalent_(
        NSString::alloc(nil).init_str(title),
        action,
        NSString::alloc(nil).init_str(""),
    );
    let target = STATE.with(|state| state.borrow().as_ref().unwrap().delegate);
    item.setTarget_(target);
    item.autorelease()
}

fn create_note(text: Option<String>) {
    let note = STATE.with(|state| {
        let mut state = state.borrow_mut();
        let state = state.as_mut().expect("app state must exist");
        let offset = ((state.notes.len() % 8) as f64) * 24.0;
        let id = state.next_id;
        state.next_id += 1;
        let note = Note {
            id,
            x: 120.0 + offset,
            y: 620.0 - offset,
            w: DEFAULT_W,
            h: DEFAULT_H,
            text: text.unwrap_or_default(),
        };
        state.notes.push(note.clone());
        note
    });

    unsafe {
        show_note(note);
    }
    save_notes();
}

unsafe fn show_note(note: Note) {
    let frame = NSRect::new(NSPoint::new(note.x, note.y), NSSize::new(note.w, note.h));
    let window = NSWindow::alloc(nil).initWithContentRect_styleMask_backing_defer_(
        frame,
        NSWindowStyleMask::NSTitledWindowMask
            | NSWindowStyleMask::NSClosableWindowMask
            | NSWindowStyleMask::NSResizableWindowMask
            | NSWindowStyleMask::NSFullSizeContentViewWindowMask,
        NSBackingStoreBuffered,
        NO,
    );

    let title = NSString::alloc(nil).init_str("");
    window.setTitle_(title);
    let _: () = msg_send![window, setTitleVisibility: 1u64];
    let _: () = msg_send![window, setTitlebarAppearsTransparent: YES];
    let _: () = msg_send![window, setMovableByWindowBackground: YES];
    let _: () = msg_send![window, setMinSize: NSSize::new(MIN_W, MIN_H)];
    let _: () = msg_send![window, setReleasedWhenClosed: NO];
    let _: () = msg_send![window, setBackgroundColor: note_color()];
    let _: () = msg_send![window, setOpaque: NO];
    let _: () = msg_send![window, setHasShadow: YES];
    let _: () = msg_send![window, setLevel: desktop_note_level()];
    let behavior = NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES
        | NS_WINDOW_COLLECTION_BEHAVIOR_STATIONARY
        | NS_WINDOW_COLLECTION_BEHAVIOR_IGNORES_CYCLE
        | NS_WINDOW_COLLECTION_BEHAVIOR_FULL_SCREEN_AUXILIARY;
    let _: () = msg_send![window, setCollectionBehavior: behavior];

    let scroll: id = msg_send![class!(NSScrollView), alloc];
    let scroll: id = msg_send![scroll, initWithFrame: NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(note.w, note.h),
    )];
    scroll.setAutoresizingMask_(18);
    let _: () = msg_send![scroll, setDrawsBackground: NO];
    let _: () = msg_send![scroll, setHasVerticalScroller: YES];
    let _: () = msg_send![scroll, setBorderType: 0u64];

    let text_view: id = msg_send![class!(NSTextView), alloc];
    let text_view: id = msg_send![text_view, initWithFrame: NSRect::new(
        NSPoint::new(10.0, 8.0),
        NSSize::new(note.w - 20.0, note.h - 16.0),
    )];
    text_view.setAutoresizingMask_(18);
    let _: () = msg_send![text_view, setDrawsBackground: NO];
    let _: () = msg_send![text_view, setTextColor: text_color()];
    let font: id = msg_send![class!(NSFont), systemFontOfSize: 16.0f64];
    let _: () = msg_send![text_view, setFont: font];
    let _: () = msg_send![text_view, setString: NSString::alloc(nil).init_str(&note.text)];
    let _: () = msg_send![text_view, setAllowsUndo: YES];
    let _: () = msg_send![text_view, setAutomaticQuoteSubstitutionEnabled: NO];
    let _: () = msg_send![text_view, setRichText: NO];
    let _: () = msg_send![text_view, setDelegate: STATE.with(|state| state.borrow().as_ref().unwrap().delegate)];

    let _: () = msg_send![scroll, setDocumentView: text_view];
    window.setContentView_(scroll);
    let _: () = msg_send![window, setDelegate: STATE.with(|state| state.borrow().as_ref().unwrap().delegate)];
    window.makeKeyAndOrderFront_(nil);

    with_state(|state| {
        state.windows.insert(window as usize, note.id);
        state.text_views.insert(text_view as usize, note.id);
    });
}

unsafe fn note_color() -> id {
    NSColor::colorWithCalibratedRed_green_blue_alpha_(
        nil,
        NOTE_YELLOW.0,
        NOTE_YELLOW.1,
        NOTE_YELLOW.2,
        0.97,
    )
}

unsafe fn text_color() -> id {
    NSColor::colorWithCalibratedRed_green_blue_alpha_(
        nil,
        NOTE_TEXT.0,
        NOTE_TEXT.1,
        NOTE_TEXT.2,
        1.0,
    )
}

unsafe fn desktop_note_level() -> i32 {
    CGWindowLevelForKey(K_CG_DESKTOP_ICON_WINDOW_LEVEL_KEY) + 1
}

fn update_text(notification: id) {
    unsafe {
        let text_view: id = msg_send![notification, object];
        let string: id = msg_send![text_view, string];
        let text = ns_string_to_string(string);
        let text_view_key = text_view as usize;

        with_state(|state| {
            if let Some(note_id) = state.text_views.get(&text_view_key).copied() {
                if let Some(note) = state.notes.iter_mut().find(|note| note.id == note_id) {
                    note.text = text;
                }
            }
        });
    }
    save_notes();
}

fn update_frame(notification: id) {
    unsafe {
        let window: id = msg_send![notification, object];
        let frame: NSRect = msg_send![window, frame];
        let window_key = window as usize;

        with_state(|state| {
            if let Some(note_id) = state.windows.get(&window_key).copied() {
                if let Some(note) = state.notes.iter_mut().find(|note| note.id == note_id) {
                    note.x = frame.origin.x;
                    note.y = frame.origin.y;
                    note.w = frame.size.width;
                    note.h = frame.size.height;
                }
            }
        });
    }
    save_notes();
}

fn close_note(notification: id) {
    unsafe {
        let window: id = msg_send![notification, object];
        let window_key = window as usize;
        with_state(|state| {
            if let Some(note_id) = state.windows.remove(&window_key) {
                state.notes.retain(|note| note.id != note_id);
                state.text_views.retain(|_, id| *id != note_id);
            }
        });
    }
    save_notes();
}

unsafe fn ns_string_to_string(value: id) -> String {
    let utf8: *const std::os::raw::c_char = msg_send![value, UTF8String];
    if utf8.is_null() {
        return String::new();
    }
    CStr::from_ptr(utf8).to_string_lossy().into_owned()
}

fn delegate_class() -> &'static Class {
    DELEGATE_CLASS.get_or_init(|| {
        let superclass = class!(NSObject);
        let mut decl = ClassDecl::new("DesktopPostItDelegate", superclass).unwrap();
        unsafe {
            decl.add_method(
                sel!(applicationDidFinishLaunching:),
                application_did_finish_launching as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(applicationWillTerminate:),
                application_will_terminate as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(sel!(newNote:), new_note as extern "C" fn(&Object, Sel, id));
            decl.add_method(sel!(quit:), quit as extern "C" fn(&Object, Sel, id));
            decl.add_method(
                sel!(textDidChange:),
                text_did_change as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(windowDidMove:),
                window_did_move as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(windowDidResize:),
                window_did_resize as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(windowWillClose:),
                window_will_close as extern "C" fn(&Object, Sel, id),
            );
        }
        decl.register()
    })
}

extern "C" fn application_did_finish_launching(_: &Object, _: Sel, _: id) {
    app_did_finish_launching();
}

extern "C" fn application_will_terminate(_: &Object, _: Sel, _: id) {
    save_notes();
}

extern "C" fn new_note(_: &Object, _: Sel, _: id) {
    create_note(None);
}

extern "C" fn quit(_: &Object, _: Sel, _: id) {
    unsafe {
        let _: () = msg_send![NSApp(), terminate: nil];
    }
}

extern "C" fn text_did_change(_: &Object, _: Sel, notification: id) {
    update_text(notification);
}

extern "C" fn window_did_move(_: &Object, _: Sel, notification: id) {
    update_frame(notification);
}

extern "C" fn window_did_resize(_: &Object, _: Sel, notification: id) {
    update_frame(notification);
}

extern "C" fn window_will_close(_: &Object, _: Sel, notification: id) {
    close_note(notification);
}
