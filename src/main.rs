#![cfg(target_os = "macos")]
#![allow(deprecated, unexpected_cfgs)]

use block::{Block, ConcreteBlock};
use chrono::{
    Datelike, Duration as ChronoDuration, Local, LocalResult, NaiveDate, NaiveTime, TimeZone,
    Timelike,
};
use cocoa::appkit::{
    NSApp, NSApplication, NSApplicationActivationPolicyAccessory, NSBackingStoreBuffered, NSColor,
    NSMenu, NSMenuItem, NSStatusBar, NSView, NSWindow, NSWindowStyleMask,
};
use cocoa::base::{id, nil, BOOL, NO, YES};
use cocoa::foundation::{NSAutoreleasePool, NSPoint, NSRange, NSRect, NSSize, NSString};
use objc::declare::ClassDecl;
use objc::runtime::{Class, Object, Sel};
use objc::{class, msg_send, sel, sel_impl};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::env;
use std::ffi::{CStr, CString};
use std::fs;
use std::os::raw::{c_char, c_int, c_void};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

const APP_NAME: &str = "Desktop Sticky Note";
const DEFAULT_W: f64 = 260.0;
const DEFAULT_H: f64 = 210.0;
const MIN_W: f64 = 180.0;
const MIN_H: f64 = 130.0;
const DEFAULT_BACKGROUND: &str = "yellow";
const DEFAULT_TEXT_COLOR: &str = "black";
const DEFAULT_FONT: &str = "system";
const DEFAULT_FONT_SIZE: f64 = 16.0;
const STATUS_ICON_SIZE: f64 = 15.0;
const STATUS_ICON_POINT_SIZE: f64 = 15.0;
const DATE_ONLY_REMINDER_HOUR: u32 = 9;
const SCHEDULER_TICK_SECONDS: u64 = 5;
const REMINDER_LEADS: &[(i64, &str)] = &[
    (30 * 60, "30 minutes before"),
    (10 * 60, "10 minutes before"),
    (5 * 60, "5 minutes before"),
    (0, "Due now"),
];
const DEFAULT_DESKTOP_ID: &str = "desktop:default";
const ACTIVE_SPACE_NOTIFICATION: &str = "NSWorkspaceActiveSpaceDidChangeNotification";
const UN_AUTHORIZATION_OPTION_ALERT: u64 = 1 << 2;
const UN_AUTHORIZATION_OPTION_SOUND: u64 = 1 << 1;
const UN_NOTIFICATION_PRESENTATION_OPTION_SOUND: u64 = 1 << 1;
const UN_NOTIFICATION_PRESENTATION_OPTION_ALERT: u64 = 1 << 2;
const UN_NOTIFICATION_PRESENTATION_OPTION_LIST: u64 = 1 << 3;
const UN_NOTIFICATION_PRESENTATION_OPTION_BANNER: u64 = 1 << 4;
const RTLD_LAZY: c_int = 0x1;

const K_CG_DESKTOP_ICON_WINDOW_LEVEL_KEY: i32 = 18;
const NS_WINDOW_COLLECTION_BEHAVIOR_MANAGED: u64 = 1 << 2;
const NS_WINDOW_COLLECTION_BEHAVIOR_IGNORES_CYCLE: u64 = 1 << 6;
const NS_WINDOW_COLLECTION_BEHAVIOR_FULL_SCREEN_AUXILIARY: u64 = 1 << 8;

struct ColorChoice {
    key: &'static str,
    label: &'static str,
    rgb: (f64, f64, f64),
}

struct FontChoice {
    key: &'static str,
    label: &'static str,
}

struct MenuChoice {
    item: usize,
    key: &'static str,
}

struct SizeMenuChoice {
    item: usize,
    size: f64,
}

const BACKGROUNDS: &[ColorChoice] = &[
    ColorChoice {
        key: "yellow",
        label: "Yellow",
        rgb: (1.0, 0.91, 0.42),
    },
    ColorChoice {
        key: "white",
        label: "White",
        rgb: (0.98, 0.98, 0.94),
    },
    ColorChoice {
        key: "black",
        label: "Black",
        rgb: (0.04, 0.04, 0.04),
    },
    ColorChoice {
        key: "blue",
        label: "Blue",
        rgb: (0.52, 0.78, 1.0),
    },
    ColorChoice {
        key: "pink",
        label: "Pink",
        rgb: (1.0, 0.68, 0.82),
    },
    ColorChoice {
        key: "green",
        label: "Green",
        rgb: (0.66, 0.91, 0.55),
    },
];

const TEXT_COLORS: &[ColorChoice] = &[
    ColorChoice {
        key: "black",
        label: "Black",
        rgb: (0.08, 0.06, 0.02),
    },
    ColorChoice {
        key: "white",
        label: "White",
        rgb: (0.96, 0.96, 0.92),
    },
    ColorChoice {
        key: "gray",
        label: "Gray",
        rgb: (0.36, 0.36, 0.34),
    },
    ColorChoice {
        key: "red",
        label: "Red",
        rgb: (0.75, 0.1, 0.08),
    },
    ColorChoice {
        key: "blue",
        label: "Blue",
        rgb: (0.05, 0.28, 0.7),
    },
];

const FONTS: &[FontChoice] = &[
    FontChoice {
        key: "system",
        label: "System",
    },
    FontChoice {
        key: "rounded",
        label: "Rounded",
    },
    FontChoice {
        key: "serif",
        label: "Serif",
    },
    FontChoice {
        key: "mono",
        label: "Mono",
    },
    FontChoice {
        key: "marker",
        label: "Marker",
    },
];

const FONT_SIZES: &[f64] = &[14.0, 16.0, 18.0, 22.0, 28.0, 36.0];

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGWindowLevelForKey(key: i32) -> i32;
    fn CGDisplayCreateUUIDFromDisplayID(display: u32) -> id;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFUUIDCreateString(allocator: *const c_void, uuid: id) -> id;
    fn CFRelease(value: *const c_void);
}

#[link(name = "UserNotifications", kind = "framework")]
extern "C" {}

extern "C" {
    fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

#[derive(Clone, Serialize, Deserialize)]
struct Note {
    id: u64,
    #[serde(default)]
    desktop_id: String,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    text: String,
    #[serde(default)]
    style: NoteStyle,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    placements: HashMap<String, NotePlacement>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    reminders: Vec<Reminder>,
    #[serde(default, rename = "reminder", skip_serializing)]
    legacy_reminder: Option<Reminder>,
}

#[derive(Clone, Serialize, Deserialize)]
struct NoteStyle {
    background: String,
    text_color: String,
    font: String,
    font_size: f64,
}

impl Default for NoteStyle {
    fn default() -> Self {
        Self {
            background: DEFAULT_BACKGROUND.to_string(),
            text_color: DEFAULT_TEXT_COLOR.to_string(),
            font: DEFAULT_FONT.to_string(),
            font_size: DEFAULT_FONT_SIZE,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Reminder {
    due_at: i64,
    date_text: Option<String>,
    time_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    body: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
struct NotePlacement {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

struct AppState {
    notes: Vec<Note>,
    windows: HashMap<usize, u64>,
    text_views: HashMap<usize, u64>,
    data_path: PathBuf,
    delegate: id,
    status_item: id,
    status_menu: id,
    next_id: u64,
    saving: bool,
    active_note_id: Option<u64>,
    default_style: NoteStyle,
    background_items: Vec<MenuChoice>,
    text_color_items: Vec<MenuChoice>,
    font_items: Vec<MenuChoice>,
    font_size_items: Vec<SizeMenuChoice>,
    active_desktop_ids: HashSet<String>,
    refreshing_desktops: bool,
    terminating: bool,
    suppressed_frame_updates: HashMap<usize, Instant>,
}

#[derive(Clone)]
struct ReminderJob {
    notify_at: i64,
    body: String,
    when: String,
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
struct ReminderJobKey {
    note_id: u64,
    reminder_index: usize,
    lead_seconds: i64,
}

#[derive(Clone)]
struct DateEntry {
    date: NaiveDate,
    text: String,
}

#[derive(Clone)]
struct TimeEntry {
    time: NaiveTime,
    text: String,
}

struct DateToken {
    start: usize,
    end: usize,
    date: NaiveDate,
    replacement: String,
}

struct TimeToken {
    start: usize,
    end: usize,
    time: NaiveTime,
    replacement: String,
}

#[derive(Clone)]
struct ActiveDesktop {
    display_identifier: Option<String>,
    desktop_id: String,
}

struct NotePlacementTarget {
    desktop_id: String,
    visible_frame: Option<NSRect>,
}

#[derive(Clone, Copy)]
struct CgsFunctions {
    main_connection_id: unsafe extern "C" fn() -> i32,
    copy_managed_display_spaces: unsafe extern "C" fn(i32) -> id,
    copy_spaces_for_windows: Option<unsafe extern "C" fn(i32, c_int, id) -> id>,
}

thread_local! {
    static STATE: RefCell<Option<AppState>> = const { RefCell::new(None) };
}

static DELEGATE_CLASS: OnceLock<&'static Class> = OnceLock::new();
static REMINDER_JOBS: OnceLock<Mutex<HashMap<ReminderJobKey, ReminderJob>>> = OnceLock::new();
static PENDING_NOTIFICATIONS: OnceLock<Mutex<Vec<ReminderJob>>> = OnceLock::new();
static SCHEDULER_STARTED: OnceLock<()> = OnceLock::new();
static CGS_FUNCTIONS: OnceLock<Option<CgsFunctions>> = OnceLock::new();
static NOTIFICATION_TARGET: OnceLock<usize> = OnceLock::new();
static SYSTEM_NOTIFICATIONS_ALLOWED: AtomicBool = AtomicBool::new(false);
static SYSTEM_NOTIFICATIONS_RESOLVED: AtomicBool = AtomicBool::new(false);

fn main() {
    unsafe {
        let _pool = NSAutoreleasePool::new(nil);
        let app = NSApp();
        app.setActivationPolicy_(NSApplicationActivationPolicyAccessory);

        let delegate = msg_send![delegate_class(), new];
        let _ = NOTIFICATION_TARGET.set(delegate as usize);
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
                status_menu: nil,
                next_id,
                saving: false,
                active_note_id: None,
                default_style: NoteStyle::default(),
                background_items: Vec::new(),
                text_color_items: Vec::new(),
                font_items: Vec::new(),
                font_size_items: Vec::new(),
                active_desktop_ids: HashSet::new(),
                refreshing_desktops: false,
                terminating: false,
                suppressed_frame_updates: HashMap::new(),
            });
        });

        start_scheduler();
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
    let mut notes: Vec<Note> = match fs::read_to_string(path) {
        Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    for note in &mut notes {
        migrate_note_reminders(note);
    }
    notes
}

fn migrate_note_reminders(note: &mut Note) {
    if note.reminders.is_empty() {
        if let Some(reminder) = note.legacy_reminder.take() {
            note.reminders.push(reminder);
        }
    } else {
        note.legacy_reminder = None;
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
        setup_notification_center();
        setup_desktop_change_notifications();

        initialize_desktop_state();

        let notes_empty = STATE.with(|state| {
            state
                .borrow()
                .as_ref()
                .map(|state| state.notes.is_empty())
                .unwrap_or(true)
        });

        if notes_empty {
            create_note(None);
        } else {
            refresh_desktop_windows();
        }
        sync_reminders();
    }
}

unsafe fn setup_notification_center() {
    log_main_bundle_identifier();

    let un_center: id = msg_send![class!(UNUserNotificationCenter), currentNotificationCenter];
    let delegate = STATE.with(|state| state.borrow().as_ref().unwrap().delegate);
    let _: () = msg_send![un_center, setDelegate: delegate];

    let options = UN_AUTHORIZATION_OPTION_ALERT | UN_AUTHORIZATION_OPTION_SOUND;
    let auth_block = ConcreteBlock::new(|granted: BOOL, error: id| {
        SYSTEM_NOTIFICATIONS_ALLOWED.store(granted == YES && error == nil, Ordering::SeqCst);
        SYSTEM_NOTIFICATIONS_RESOLVED.store(true, Ordering::SeqCst);
        if error != nil {
            unsafe {
                let description: id = msg_send![error, localizedDescription];
                eprintln!(
                    "notification authorization failed: {}",
                    ns_string_to_string(description)
                );
            }
        } else if granted == YES {
            eprintln!("notification authorization granted");
        } else {
            eprintln!("notification authorization denied");
        }
    })
    .copy();
    let _: () = msg_send![un_center,
        requestAuthorizationWithOptions: options
        completionHandler: &*auth_block
    ];
    std::mem::forget(auth_block);

    let center: id = msg_send![
        class!(NSUserNotificationCenter),
        defaultUserNotificationCenter
    ];
    let _: () = msg_send![center, setDelegate: delegate];
}

unsafe fn log_main_bundle_identifier() {
    let bundle: id = msg_send![class!(NSBundle), mainBundle];
    let identifier: id = msg_send![bundle, bundleIdentifier];
    if identifier == nil {
        eprintln!("notification bundle id: <none>");
    } else {
        eprintln!(
            "notification bundle id: {}",
            ns_string_to_string(identifier)
        );
    }
}

unsafe fn setup_desktop_change_notifications() {
    let workspace: id = msg_send![class!(NSWorkspace), sharedWorkspace];
    let center: id = msg_send![workspace, notificationCenter];
    let delegate = STATE.with(|state| state.borrow().as_ref().unwrap().delegate);
    let _: () = msg_send![center,
        addObserver: delegate
        selector: sel!(activeSpaceDidChange:)
        name: ns_string(ACTIVE_SPACE_NOTIFICATION)
        object: nil
    ];
}

fn initialize_desktop_state() {
    let active_desktops = unsafe { current_desktops() };
    let active_desktop_ids = desktop_id_set(&active_desktops);
    let fallback_desktop_id = active_desktops
        .first()
        .map(|desktop| desktop.desktop_id.clone())
        .unwrap_or_else(|| DEFAULT_DESKTOP_ID.to_string());

    let migrated = STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(state) = state.as_mut() else {
            return false;
        };

        state.active_desktop_ids = active_desktop_ids;
        let mut migrated = false;
        for note in &mut state.notes {
            if note.desktop_id.is_empty() || note.desktop_id == DEFAULT_DESKTOP_ID {
                note.desktop_id = unsafe { desktop_id_for_note_frame(note) }
                    .unwrap_or_else(|| fallback_desktop_id.clone());
                migrated = true;
            } else if let Some(frame_desktop_id) = unsafe { desktop_id_for_note_frame(note) } {
                if desktop_display_identifier(&note.desktop_id)
                    .zip(desktop_display_identifier(&frame_desktop_id))
                    .is_some_and(|(saved, frame)| !saved.eq_ignore_ascii_case(frame))
                {
                    note.desktop_id = frame_desktop_id;
                    migrated = true;
                }
            }
            let desktop_id = note.desktop_id.clone();
            if ensure_note_placement(note, &desktop_id) {
                migrated = true;
            }
        }
        migrated
    });

    if migrated {
        save_notes();
    }
}

fn active_space_changed() {
    refresh_desktop_windows();
}

fn refresh_desktop_windows() {
    let frames_changed = persist_visible_window_frames(false);
    let active_desktops = unsafe { current_desktops() };
    let active_desktop_ids = desktop_id_set(&active_desktops);
    let window_keys = STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(state) = state.as_mut() else {
            return Vec::new();
        };
        state.active_desktop_ids = active_desktop_ids;
        state.refreshing_desktops = true;
        state.windows.keys().copied().collect::<Vec<_>>()
    });

    unsafe {
        for window_key in window_keys {
            let _: () = msg_send![window_key as id, close];
        }
    }

    let notes = STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(state) = state.as_mut() else {
            return Vec::new();
        };
        state.windows.clear();
        state.text_views.clear();
        let notes = state
            .notes
            .iter()
            .filter(|note| note_is_on_active_desktop(state, note))
            .cloned()
            .collect::<Vec<_>>();
        if !state
            .active_note_id
            .is_some_and(|note_id| state.notes.iter().any(|note| note.id == note_id))
        {
            state.active_note_id = notes.last().map(|note| note.id);
        }
        notes
    });

    unsafe {
        for note in notes {
            show_note(note, false);
        }
    }
    with_state(|state| state.refreshing_desktops = false);
    update_menu_states();
    if frames_changed {
        save_notes();
    }
}

unsafe fn setup_status_menu() {
    let status_item: id =
        msg_send![NSStatusBar::systemStatusBar(nil), statusItemWithLength: -1.0f64];
    let _: id = msg_send![status_item, retain];
    let _: () = msg_send![status_item, setAutosaveName: ns_string("DesktopStickyNoteStatusItem")];
    let _: () = msg_send![status_item, setVisible: YES];
    let button: id = msg_send![status_item, button];
    set_status_icon(button);

    let menu = NSMenu::new(nil).autorelease();
    let _: id = msg_send![menu, retain];
    let delegate = STATE.with(|state| state.borrow().as_ref().unwrap().delegate);
    let _: () = msg_send![menu, setDelegate: delegate];

    let new_item = menu_item("New Note", sel!(newNote:));
    let _: () = msg_send![menu, addItem: new_item];
    let move_item = menu_item(
        "Move Selected Note to This Desktop",
        sel!(moveSelectedNoteHere:),
    );
    let _: () = msg_send![menu, addItem: move_item];
    add_separator(menu);

    let background_items = add_color_submenu(menu, "Note Color", BACKGROUNDS, sel!(setBackground:));
    let text_color_items = add_color_submenu(menu, "Text Color", TEXT_COLORS, sel!(setTextColor:));
    let font_items = add_font_submenu(menu);
    let font_size_items = add_font_size_submenu(menu);

    add_separator(menu);
    let quit_item = menu_item("Quit", sel!(quit:));
    let _: () = msg_send![menu, addItem: quit_item];
    let _: () = msg_send![status_item, setMenu: menu];

    with_state(|state| {
        state.status_item = status_item;
        state.status_menu = menu;
        state.background_items = background_items;
        state.text_color_items = text_color_items;
        state.font_items = font_items;
        state.font_size_items = font_size_items;
    });
    update_menu_states();
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

unsafe fn tagged_menu_item(title: &str, action: Sel, tag: usize) -> id {
    let item = menu_item(title, action);
    let _: () = msg_send![item, setTag: tag as isize];
    item
}

unsafe fn add_separator(menu: id) {
    let item: id = msg_send![class!(NSMenuItem), separatorItem];
    let _: () = msg_send![menu, addItem: item];
}

unsafe fn submenu(title: &str) -> (id, id) {
    let item: id = msg_send![class!(NSMenuItem), alloc];
    let item: id = msg_send![item,
        initWithTitle: ns_string(title)
        action: nil
        keyEquivalent: ns_string("")
    ];
    let menu = NSMenu::new(nil).autorelease();
    let _: () = msg_send![item, setSubmenu: menu];
    (item.autorelease(), menu)
}

unsafe fn add_color_submenu(
    menu: id,
    title: &str,
    choices: &[ColorChoice],
    action: Sel,
) -> Vec<MenuChoice> {
    let (item, submenu) = submenu(title);
    let mut items = Vec::with_capacity(choices.len());
    for (index, choice) in choices.iter().enumerate() {
        let choice_item = tagged_menu_item(choice.label, action, index);
        let _: () = msg_send![submenu, addItem: choice_item];
        items.push(MenuChoice {
            item: choice_item as usize,
            key: choice.key,
        });
    }
    let _: () = msg_send![menu, addItem: item];
    items
}

unsafe fn add_font_submenu(menu: id) -> Vec<MenuChoice> {
    let (item, submenu) = submenu("Font");
    let mut items = Vec::with_capacity(FONTS.len());
    for (index, choice) in FONTS.iter().enumerate() {
        let choice_item = tagged_menu_item(choice.label, sel!(setFont:), index);
        let _: () = msg_send![submenu, addItem: choice_item];
        items.push(MenuChoice {
            item: choice_item as usize,
            key: choice.key,
        });
    }
    let _: () = msg_send![menu, addItem: item];
    items
}

unsafe fn add_font_size_submenu(menu: id) -> Vec<SizeMenuChoice> {
    let (item, submenu) = submenu("Font Size");
    let mut items = Vec::with_capacity(FONT_SIZES.len());
    for (index, size) in FONT_SIZES.iter().copied().enumerate() {
        let choice_item = tagged_menu_item(&format!("{size:.0} pt"), sel!(setFontSize:), index);
        let _: () = msg_send![submenu, addItem: choice_item];
        items.push(SizeMenuChoice {
            item: choice_item as usize,
            size,
        });
    }
    let _: () = msg_send![menu, addItem: item];
    items
}

unsafe fn set_status_icon(button: id) {
    let mut image = system_symbol("note.text");
    if image == nil {
        image = system_symbol("square.and.pencil");
    }

    if image != nil {
        let _: () = msg_send![image, setTemplate: YES];
        let _: () = msg_send![image, setSize: NSSize::new(STATUS_ICON_SIZE, STATUS_ICON_SIZE)];
        let _: () = msg_send![button, setImage: image];
        let _: () = msg_send![button, setImagePosition: 2u64];
        let _: () = msg_send![button, setImageScaling: 0u64];
        let can_hug_title: BOOL = msg_send![button, respondsToSelector: sel!(setImageHugsTitle:)];
        if can_hug_title == YES {
            let _: () = msg_send![button, setImageHugsTitle: YES];
        }
    }
    let font: id = msg_send![class!(NSFont), menuBarFontOfSize: 0.0f64];
    if font != nil {
        let _: () = msg_send![button, setFont: font];
    }
    let _: () = msg_send![button, setTitle: ns_string("Sticky")];
    let _: () = msg_send![button, setToolTip: ns_string(APP_NAME)];
}

unsafe fn system_symbol(name: &str) -> id {
    let image: id = msg_send![class!(NSImage),
        imageWithSystemSymbolName: ns_string(name)
        accessibilityDescription: ns_string(APP_NAME)
    ];
    if image == nil {
        return nil;
    }

    let Some(config_class) = Class::get("NSImageSymbolConfiguration") else {
        return image;
    };
    let can_configure: BOOL =
        msg_send![image, respondsToSelector: sel!(imageWithSymbolConfiguration:)];
    if can_configure == NO {
        return image;
    }

    let config: id = msg_send![config_class,
        configurationWithPointSize: STATUS_ICON_POINT_SIZE
        weight: 0.23f64
        scale: 2isize
    ];
    if config == nil {
        return image;
    }

    let configured: id = msg_send![image, imageWithSymbolConfiguration: config];
    if configured == nil {
        image
    } else {
        configured
    }
}

unsafe fn ns_string(value: &str) -> id {
    NSString::alloc(nil).init_str(value)
}

fn note_placement_from_values(x: f64, y: f64, w: f64, h: f64) -> NotePlacement {
    NotePlacement { x, y, w, h }
}

fn note_placement_from_note(note: &Note) -> NotePlacement {
    note_placement_from_values(note.x, note.y, note.w, note.h)
}

fn frame_from_placement(placement: &NotePlacement) -> NSRect {
    NSRect::new(
        NSPoint::new(placement.x, placement.y),
        NSSize::new(placement.w, placement.h),
    )
}

fn frame_from_note(note: &Note) -> NSRect {
    NSRect::new(NSPoint::new(note.x, note.y), NSSize::new(note.w, note.h))
}

unsafe fn desktop_id_for_note_frame(note: &Note) -> Option<String> {
    desktop_id_for_frame(frame_from_note(note))
}

fn ensure_note_placement(note: &mut Note, desktop_id: &str) -> bool {
    if note.placements.contains_key(desktop_id) {
        return false;
    }
    note.placements
        .insert(desktop_id.to_string(), note_placement_from_note(note));
    true
}

fn note_placement_for_desktop(note: &Note, desktop_id: &str) -> NotePlacement {
    note.placements
        .get(desktop_id)
        .cloned()
        .unwrap_or_else(|| note_placement_from_note(note))
}

fn apply_note_frame(note: &mut Note, desktop_id: &str, frame: NSRect) {
    note.desktop_id = desktop_id.to_string();
    note.x = frame.origin.x;
    note.y = frame.origin.y;
    note.w = frame.size.width;
    note.h = frame.size.height;
    note.placements.insert(
        desktop_id.to_string(),
        note_placement_from_values(
            frame.origin.x,
            frame.origin.y,
            frame.size.width,
            frame.size.height,
        ),
    );
}

fn save_note_frame_without_reassigning_desktop(note: &mut Note, frame: NSRect) {
    let desktop_id = note_desktop_id(note).to_string();
    apply_note_frame(note, &desktop_id, frame);
}

fn save_note_window_frame(note: &mut Note, window: id, frame: NSRect) {
    let desktop_id =
        unsafe { desktop_id_for_window_space(window).or_else(|| desktop_id_for_frame(frame)) };
    if let Some(desktop_id) = desktop_id {
        apply_note_frame(note, &desktop_id, frame);
    } else {
        save_note_frame_without_reassigning_desktop(note, frame);
    }
}

fn move_active_note_to_current_desktop() {
    let target = note_placement_target();
    let moved = STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(state) = state.as_mut() else {
            return false;
        };
        let Some(note_id) = selected_note_id_for_move(state) else {
            return false;
        };
        let Some(note_index) = state.notes.iter().position(|note| note.id == note_id) else {
            return false;
        };

        state.active_desktop_ids.insert(target.desktop_id.clone());
        let frame = note_frame_for_target_desktop(&state.notes[note_index], &target);
        apply_note_frame(&mut state.notes[note_index], &target.desktop_id, frame);
        state.active_note_id = Some(note_id);
        true
    });

    if moved {
        save_notes();
        refresh_desktop_windows();
        update_menu_states();
    }
}

fn selected_note_id_for_move(state: &AppState) -> Option<u64> {
    state
        .active_note_id
        .filter(|note_id| state.notes.iter().any(|note| note.id == *note_id))
        .or_else(|| {
            state
                .notes
                .iter()
                .rev()
                .find(|note| note_is_on_active_desktop(state, note))
                .map(|note| note.id)
        })
        .or_else(|| state.notes.last().map(|note| note.id))
}

fn note_frame_for_target_desktop(note: &Note, target: &NotePlacementTarget) -> NSRect {
    let mut placement = note_placement_for_desktop(note, &target.desktop_id);
    if !note.placements.contains_key(&target.desktop_id) {
        placement = clamp_placement_to_target(placement, target);
    }
    frame_from_placement(&placement)
}

fn clamp_placement_to_target(
    mut placement: NotePlacement,
    target: &NotePlacementTarget,
) -> NotePlacement {
    let Some(frame) = target.visible_frame else {
        return placement;
    };

    let max_w = (frame.size.width - 40.0).max(MIN_W);
    let max_h = (frame.size.height - 40.0).max(MIN_H);
    placement.w = clamp(placement.w, MIN_W, max_w);
    placement.h = clamp(placement.h, MIN_H, max_h);

    let min_x = frame.origin.x + 20.0;
    let max_x = frame.origin.x + frame.size.width - placement.w - 20.0;
    let min_y = frame.origin.y + 20.0;
    let max_y = frame.origin.y + frame.size.height - placement.h - 20.0;
    placement.x = clamp(placement.x, min_x, max_x);
    placement.y = clamp(placement.y, min_y, max_y);
    placement
}

fn persist_visible_window_frames(reassign_desktops: bool) -> bool {
    let windows = STATE.with(|state| {
        let state = state.borrow();
        let Some(state) = state.as_ref() else {
            return Vec::new();
        };
        state
            .windows
            .iter()
            .map(|(window, note_id)| (*window, *note_id))
            .collect::<Vec<_>>()
    });

    let mut changed = false;
    for (window_key, note_id) in windows {
        unsafe {
            let window = window_key as id;
            let frame: NSRect = msg_send![window, frame];
            let window_desktop_id = if reassign_desktops {
                None
            } else {
                desktop_id_for_window_space(window)
            };
            with_state(|state| {
                if let Some(note) = state.notes.iter_mut().find(|note| note.id == note_id) {
                    if reassign_desktops {
                        save_note_window_frame(note, window, frame);
                    } else {
                        if window_desktop_id.as_deref().is_some_and(|desktop_id| {
                            !desktop_id.eq_ignore_ascii_case(note_desktop_id(note))
                        }) {
                            return;
                        }
                        save_note_frame_without_reassigning_desktop(note, frame);
                    }
                    changed = true;
                }
            });
        }
    }
    changed
}

fn create_note(text: Option<String>) {
    let target = note_placement_target();
    let note = STATE.with(|state| {
        let mut state = state.borrow_mut();
        let state = state.as_mut().expect("app state must exist");
        state.active_desktop_ids.insert(target.desktop_id.clone());
        let desktop_note_count = state
            .notes
            .iter()
            .filter(|note| note.desktop_id == target.desktop_id)
            .count();
        let offset = ((desktop_note_count % 8) as f64) * 24.0;
        let (x, y) = note_origin(&target, offset);
        let placement = note_placement_from_values(x, y, DEFAULT_W, DEFAULT_H);
        let placements = HashMap::from([(target.desktop_id.clone(), placement)]);
        let id = state.next_id;
        state.next_id += 1;
        state.active_note_id = Some(id);
        let note = Note {
            id,
            desktop_id: target.desktop_id.clone(),
            x,
            y,
            w: DEFAULT_W,
            h: DEFAULT_H,
            text: text.unwrap_or_default(),
            style: state.default_style.clone(),
            placements,
            reminders: Vec::new(),
            legacy_reminder: None,
        };
        state.notes.push(note.clone());
        note
    });

    unsafe {
        show_note(note, true);
    }
    save_notes();
    sync_reminders();
}

unsafe fn show_note(note: Note, make_key: bool) {
    let desktop_id = note_desktop_id(&note).to_string();
    let placement = note_placement_for_desktop(&note, &desktop_id);
    let frame = frame_from_placement(&placement);
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
    let _: () = msg_send![window, setTitlebarSeparatorStyle: 1isize];
    let _: () = msg_send![window, setMovableByWindowBackground: YES];
    let _: () = msg_send![window, setMinSize: NSSize::new(MIN_W, MIN_H)];
    let _: () = msg_send![window, setShowsResizeIndicator: NO];
    let _: () = msg_send![window, setReleasedWhenClosed: NO];
    let _: () = msg_send![window, setBackgroundColor: background_color(&note.style.background)];
    let _: () = msg_send![window, setOpaque: NO];
    let _: () = msg_send![window, setHasShadow: YES];
    let _: () = msg_send![window, setLevel: desktop_note_level()];
    let behavior = NS_WINDOW_COLLECTION_BEHAVIOR_MANAGED
        | NS_WINDOW_COLLECTION_BEHAVIOR_IGNORES_CYCLE
        | NS_WINDOW_COLLECTION_BEHAVIOR_FULL_SCREEN_AUXILIARY;
    let _: () = msg_send![window, setCollectionBehavior: behavior];
    hide_window_button(window, 1);
    hide_window_button(window, 2);

    let scroll: id = msg_send![class!(NSScrollView), alloc];
    let scroll: id = msg_send![scroll, initWithFrame: NSRect::new(
        NSPoint::new(0.0, 0.0),
        NSSize::new(note.w, note.h),
    )];
    scroll.setAutoresizingMask_(18);
    let _: () = msg_send![scroll, setDrawsBackground: NO];
    let _: () = msg_send![scroll, setHasVerticalScroller: NO];
    let _: () = msg_send![scroll, setHasHorizontalScroller: NO];
    let _: () = msg_send![scroll, setAutohidesScrollers: YES];
    let _: () = msg_send![scroll, setScrollerStyle: 1u64];
    let _: () = msg_send![scroll, setBorderType: 0u64];

    let text_view: id = msg_send![class!(NSTextView), alloc];
    let text_view: id = msg_send![text_view, initWithFrame: NSRect::new(
        NSPoint::new(10.0, 8.0),
        NSSize::new(note.w - 20.0, note.h - 16.0),
    )];
    text_view.setAutoresizingMask_(18);
    let _: () = msg_send![text_view, setDrawsBackground: NO];
    let text_color = text_color(&note.style.text_color);
    let _: () = msg_send![text_view, setTextColor: text_color];
    let _: () = msg_send![text_view, setInsertionPointColor: text_color];
    let font = note_font(&note.style);
    let _: () = msg_send![text_view, setFont: font];
    let _: () = msg_send![text_view, setString: NSString::alloc(nil).init_str(&note.text)];
    let _: () = msg_send![text_view, setAllowsUndo: YES];
    let _: () = msg_send![text_view, setAutomaticQuoteSubstitutionEnabled: NO];
    let _: () = msg_send![text_view, setRichText: NO];
    let _: () = msg_send![text_view, setDelegate: STATE.with(|state| state.borrow().as_ref().unwrap().delegate)];
    let _: () = msg_send![text_view, setMenu: note_context_menu()];

    let _: () = msg_send![scroll, setDocumentView: text_view];
    window.setContentView_(scroll);
    let _: () = msg_send![window, setDelegate: STATE.with(|state| state.borrow().as_ref().unwrap().delegate)];
    if make_key {
        window.makeKeyAndOrderFront_(nil);
    } else {
        let _: () = msg_send![window, orderFrontRegardless];
    }

    with_state(|state| {
        state.windows.insert(window as usize, note.id);
        state.text_views.insert(text_view as usize, note.id);
        if state.refreshing_desktops {
            state
                .suppressed_frame_updates
                .insert(window as usize, Instant::now() + Duration::from_secs(5));
        }
    });
}

unsafe fn hide_window_button(window: id, button: u64) {
    let button: id = msg_send![window, standardWindowButton: button];
    if button != nil {
        let _: () = msg_send![button, setHidden: YES];
        let _: () = msg_send![button, setEnabled: NO];
    }
}

unsafe fn background_color(key: &str) -> id {
    color_from_choices(key, BACKGROUNDS, 0.97)
}

unsafe fn text_color(key: &str) -> id {
    color_from_choices(key, TEXT_COLORS, 1.0)
}

unsafe fn color_from_choices(key: &str, choices: &[ColorChoice], alpha: f64) -> id {
    let rgb = choices
        .iter()
        .find(|choice| choice.key == key)
        .map(|choice| choice.rgb)
        .unwrap_or(choices[0].rgb);
    NSColor::colorWithCalibratedRed_green_blue_alpha_(nil, rgb.0, rgb.1, rgb.2, alpha)
}

unsafe fn note_font(style: &NoteStyle) -> id {
    match style.font.as_str() {
        "rounded" => named_font("Avenir Next", style.font_size),
        "serif" => named_font("Georgia", style.font_size),
        "mono" => msg_send![class!(NSFont), userFixedPitchFontOfSize: style.font_size],
        "marker" => named_font("Marker Felt", style.font_size),
        _ => msg_send![class!(NSFont), systemFontOfSize: style.font_size],
    }
}

unsafe fn named_font(name: &str, size: f64) -> id {
    let font: id = msg_send![class!(NSFont), fontWithName: ns_string(name) size: size];
    if font == nil {
        msg_send![class!(NSFont), systemFontOfSize: size]
    } else {
        font
    }
}

unsafe fn desktop_note_level() -> i32 {
    CGWindowLevelForKey(K_CG_DESKTOP_ICON_WINDOW_LEVEL_KEY) + 1
}

fn desktop_id_set(desktops: &[ActiveDesktop]) -> HashSet<String> {
    let mut ids = desktops
        .iter()
        .map(|desktop| desktop.desktop_id.clone())
        .collect::<HashSet<_>>();
    if ids.is_empty() {
        ids.insert(DEFAULT_DESKTOP_ID.to_string());
    }
    ids
}

fn note_is_on_active_desktop(state: &AppState, note: &Note) -> bool {
    state.active_desktop_ids.is_empty() || state.active_desktop_ids.contains(note_desktop_id(note))
}

fn note_desktop_id(note: &Note) -> &str {
    if note.desktop_id.is_empty() {
        DEFAULT_DESKTOP_ID
    } else {
        &note.desktop_id
    }
}

fn note_placement_target() -> NotePlacementTarget {
    unsafe {
        let mouse: NSPoint = msg_send![class!(NSEvent), mouseLocation];
        let screen = screen_for_point(mouse);
        let desktop_id = desktop_id_for_screen(screen).unwrap_or_else(primary_desktop_id);
        let visible_frame = if screen == nil {
            None
        } else {
            Some(msg_send![screen, visibleFrame])
        };

        NotePlacementTarget {
            desktop_id,
            visible_frame,
        }
    }
}

unsafe fn desktop_id_for_frame(frame: NSRect) -> Option<String> {
    let center = NSPoint::new(
        frame.origin.x + frame.size.width / 2.0,
        frame.origin.y + frame.size.height / 2.0,
    );
    desktop_id_for_screen(screen_for_point(center))
}

fn primary_desktop_id() -> String {
    STATE
        .with(|state| {
            state
                .borrow()
                .as_ref()
                .and_then(|state| state.active_desktop_ids.iter().next().cloned())
        })
        .or_else(|| unsafe {
            current_desktops()
                .first()
                .map(|desktop| desktop.desktop_id.clone())
        })
        .unwrap_or_else(|| DEFAULT_DESKTOP_ID.to_string())
}

fn note_origin(target: &NotePlacementTarget, offset: f64) -> (f64, f64) {
    let Some(frame) = target.visible_frame else {
        return (120.0 + offset, 620.0 - offset);
    };

    let min_x = frame.origin.x + 20.0;
    let max_x = frame.origin.x + frame.size.width - DEFAULT_W - 20.0;
    let min_y = frame.origin.y + 20.0;
    let max_y = frame.origin.y + frame.size.height - DEFAULT_H - 20.0;
    let x = clamp(frame.origin.x + 80.0 + offset, min_x, max_x);
    let y = clamp(
        frame.origin.y + frame.size.height - DEFAULT_H - 80.0 - offset,
        min_y,
        max_y,
    );
    (x, y)
}

fn clamp(value: f64, min: f64, max: f64) -> f64 {
    if max < min {
        min
    } else {
        value.max(min).min(max)
    }
}

unsafe fn desktop_id_for_screen(screen: id) -> Option<String> {
    let display_identifier = screen_display_identifier(screen);
    let desktops = current_desktops();

    if let Some(display_identifier) = display_identifier {
        if let Some(desktop) = desktops.iter().find(|desktop| {
            desktop
                .display_identifier
                .as_deref()
                .is_some_and(|candidate| candidate.eq_ignore_ascii_case(&display_identifier))
        }) {
            return Some(desktop.desktop_id.clone());
        }
    }

    desktops.first().map(|desktop| desktop.desktop_id.clone())
}

unsafe fn screen_for_point(point: NSPoint) -> id {
    let screens: id = msg_send![class!(NSScreen), screens];
    let count: usize = msg_send![screens, count];
    for index in 0..count {
        let screen: id = msg_send![screens, objectAtIndex: index];
        let frame: NSRect = msg_send![screen, frame];
        if rect_contains_point(frame, point) {
            return screen;
        }
    }

    let main: id = msg_send![class!(NSScreen), mainScreen];
    if main != nil || count == 0 {
        main
    } else {
        msg_send![screens, objectAtIndex: 0usize]
    }
}

fn rect_contains_point(rect: NSRect, point: NSPoint) -> bool {
    point.x >= rect.origin.x
        && point.x <= rect.origin.x + rect.size.width
        && point.y >= rect.origin.y
        && point.y <= rect.origin.y + rect.size.height
}

unsafe fn screen_display_identifier(screen: id) -> Option<String> {
    if screen == nil {
        return None;
    }

    let description: id = msg_send![screen, deviceDescription];
    let screen_number: id = msg_send![description, objectForKey: ns_string("NSScreenNumber")];
    if screen_number == nil {
        return None;
    }

    let display_id: u32 = msg_send![screen_number, unsignedIntValue];
    display_uuid(display_id).or_else(|| Some(format!("display-id:{display_id}")))
}

unsafe fn display_uuid(display_id: u32) -> Option<String> {
    let uuid: id = CGDisplayCreateUUIDFromDisplayID(display_id);
    if uuid == nil {
        return None;
    }

    let value: id = CFUUIDCreateString(std::ptr::null(), uuid);
    CFRelease(uuid as *const c_void);
    if value == nil {
        return None;
    }

    let identifier = ns_string_to_string(value);
    CFRelease(value as *const c_void);
    (!identifier.is_empty()).then_some(identifier)
}

fn cgs_functions() -> Option<CgsFunctions> {
    *CGS_FUNCTIONS.get_or_init(|| unsafe {
        let path =
            CString::new("/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics").ok()?;
        let handle = dlopen(path.as_ptr(), RTLD_LAZY);
        if handle.is_null() {
            return None;
        }

        let main_name = CString::new("CGSMainConnectionID").ok()?;
        let spaces_name = CString::new("CGSCopyManagedDisplaySpaces").ok()?;
        let window_spaces_name = CString::new("CGSCopySpacesForWindows").ok()?;
        let main_connection_id = dlsym(handle, main_name.as_ptr());
        let copy_managed_display_spaces = dlsym(handle, spaces_name.as_ptr());
        let copy_spaces_for_windows = dlsym(handle, window_spaces_name.as_ptr());
        if main_connection_id.is_null() || copy_managed_display_spaces.is_null() {
            return None;
        }

        Some(CgsFunctions {
            main_connection_id: std::mem::transmute::<*mut c_void, unsafe extern "C" fn() -> i32>(
                main_connection_id,
            ),
            copy_managed_display_spaces: std::mem::transmute::<
                *mut c_void,
                unsafe extern "C" fn(i32) -> id,
            >(copy_managed_display_spaces),
            copy_spaces_for_windows: (!copy_spaces_for_windows.is_null()).then(|| {
                std::mem::transmute::<*mut c_void, unsafe extern "C" fn(i32, c_int, id) -> id>(
                    copy_spaces_for_windows,
                )
            }),
        })
    })
}

unsafe fn current_desktops() -> Vec<ActiveDesktop> {
    let Some(functions) = cgs_functions() else {
        return fallback_desktops();
    };

    let connection = (functions.main_connection_id)();
    let managed_spaces = (functions.copy_managed_display_spaces)(connection);
    if managed_spaces == nil {
        return fallback_desktops();
    }

    let mut desktops = Vec::new();
    let display_count: usize = msg_send![managed_spaces, count];
    for display_index in 0..display_count {
        let display: id = msg_send![managed_spaces, objectAtIndex: display_index];
        let display_identifier = dict_string(display, "Display Identifier");
        let current_space = dict_object(display, "Current Space");
        if current_space != nil {
            if let Some(space_identifier) = space_identifier(current_space) {
                desktops.push(ActiveDesktop {
                    desktop_id: desktop_id_for_space(
                        display_identifier.as_deref(),
                        &space_identifier,
                    ),
                    display_identifier,
                });
            }
            continue;
        }

        let spaces = dict_object(display, "Spaces");
        if spaces == nil {
            continue;
        }

        let space_count: usize = msg_send![spaces, count];
        for space_index in 0..space_count {
            let space: id = msg_send![spaces, objectAtIndex: space_index];
            if !dict_bool(space, "Current Space") {
                continue;
            }

            if let Some(space_identifier) = space_identifier(space) {
                desktops.push(ActiveDesktop {
                    desktop_id: desktop_id_for_space(
                        display_identifier.as_deref(),
                        &space_identifier,
                    ),
                    display_identifier: display_identifier.clone(),
                });
            }
        }
    }

    CFRelease(managed_spaces as *const c_void);
    if desktops.is_empty() {
        fallback_desktops()
    } else {
        desktops
    }
}

unsafe fn all_desktops() -> Vec<ActiveDesktop> {
    let Some(functions) = cgs_functions() else {
        return fallback_desktops();
    };

    let connection = (functions.main_connection_id)();
    let managed_spaces = (functions.copy_managed_display_spaces)(connection);
    if managed_spaces == nil {
        return fallback_desktops();
    }

    let mut desktops = Vec::new();
    let display_count: usize = msg_send![managed_spaces, count];
    for display_index in 0..display_count {
        let display: id = msg_send![managed_spaces, objectAtIndex: display_index];
        let display_identifier = dict_string(display, "Display Identifier");
        let spaces = dict_object(display, "Spaces");
        if spaces == nil {
            continue;
        }

        let space_count: usize = msg_send![spaces, count];
        for space_index in 0..space_count {
            let space: id = msg_send![spaces, objectAtIndex: space_index];
            if let Some(space_identifier) = space_identifier(space) {
                desktops.push(ActiveDesktop {
                    desktop_id: desktop_id_for_space(
                        display_identifier.as_deref(),
                        &space_identifier,
                    ),
                    display_identifier: display_identifier.clone(),
                });
            }
        }
    }

    CFRelease(managed_spaces as *const c_void);
    if desktops.is_empty() {
        fallback_desktops()
    } else {
        desktops
    }
}

fn fallback_desktops() -> Vec<ActiveDesktop> {
    vec![ActiveDesktop {
        display_identifier: None,
        desktop_id: DEFAULT_DESKTOP_ID.to_string(),
    }]
}

fn desktop_id_for_space(display_identifier: Option<&str>, space_identifier: &str) -> String {
    match display_identifier {
        Some(display_identifier) => {
            format!("display:{display_identifier}/space:{space_identifier}")
        }
        None => format!("space:{space_identifier}"),
    }
}

fn desktop_display_identifier(desktop_id: &str) -> Option<&str> {
    desktop_id
        .strip_prefix("display:")?
        .split_once("/space:")?
        .0
        .into()
}

fn managed_space_id_from_desktop_id(desktop_id: &str) -> Option<u64> {
    let space = desktop_id
        .rsplit_once("/space:")
        .map(|(_, space)| space)
        .or_else(|| desktop_id.strip_prefix("space:"))?;
    space
        .strip_prefix("id:")
        .or_else(|| space.strip_prefix("managed:"))?
        .parse()
        .ok()
}

unsafe fn desktop_id_for_managed_space_id(space_id: u64) -> Option<String> {
    all_desktops().into_iter().find_map(|desktop| {
        (managed_space_id_from_desktop_id(&desktop.desktop_id) == Some(space_id))
            .then_some(desktop.desktop_id)
    })
}

unsafe fn managed_space_id_for_window(window: id) -> Option<u64> {
    let Some(functions) = cgs_functions() else {
        return None;
    };
    let Some(copy_spaces_for_windows) = functions.copy_spaces_for_windows else {
        return None;
    };

    let window_number: isize = msg_send![window, windowNumber];
    if window_number <= 0 {
        return None;
    }
    let number: id = msg_send![class!(NSNumber), numberWithUnsignedInteger: window_number as usize];
    let windows: id = msg_send![class!(NSArray), arrayWithObject: number];
    let connection = (functions.main_connection_id)();
    let spaces = copy_spaces_for_windows(connection, 7, windows);
    if spaces == nil {
        return None;
    }

    let count: usize = msg_send![spaces, count];
    let space_id = if count == 0 {
        None
    } else {
        let value: id = msg_send![spaces, objectAtIndex: 0usize];
        if value == nil {
            None
        } else {
            Some(msg_send![value, unsignedLongLongValue])
        }
    };
    CFRelease(spaces as *const c_void);
    space_id
}

unsafe fn desktop_id_for_window_space(window: id) -> Option<String> {
    managed_space_id_for_window(window)
        .and_then(|space_id| desktop_id_for_managed_space_id(space_id))
}

unsafe fn dict_object(dict: id, key: &str) -> id {
    msg_send![dict, objectForKey: ns_string(key)]
}

unsafe fn dict_bool(dict: id, key: &str) -> bool {
    let value = dict_object(dict, key);
    if value == nil {
        return false;
    }
    let result: BOOL = msg_send![value, boolValue];
    result == YES
}

unsafe fn dict_u64(dict: id, key: &str) -> Option<u64> {
    let value = dict_object(dict, key);
    if value == nil {
        return None;
    }
    let result: u64 = msg_send![value, unsignedLongLongValue];
    Some(result)
}

unsafe fn dict_string(dict: id, key: &str) -> Option<String> {
    let value = dict_object(dict, key);
    if value == nil {
        return None;
    }

    let description: id = msg_send![value, description];
    if description == nil {
        return None;
    }

    let value = ns_string_to_string(description);
    (!value.is_empty()).then_some(value)
}

unsafe fn space_identifier(space: id) -> Option<String> {
    dict_u64(space, "id64")
        .map(|id| format!("id:{id}"))
        .or_else(|| dict_u64(space, "ManagedSpaceID").map(|id| format!("managed:{id}")))
        .or_else(|| dict_string(space, "uuid").map(|uuid| format!("uuid:{uuid}")))
}

fn update_text(notification: id) {
    unsafe {
        let text_view: id = msg_send![notification, object];
        let string: id = msg_send![text_view, string];
        let original_text = ns_string_to_string(string);
        let text_view_key = text_view as usize;
        let existing = STATE.with(|state| {
            let state = state.borrow();
            let state = state.as_ref()?;
            let note_id = state.text_views.get(&text_view_key).copied()?;
            state
                .notes
                .iter()
                .find(|note| note.id == note_id)
                .map(|note| note.reminders.clone())
        });
        let existing = existing.unwrap_or_default();
        let (text, reminders) = normalize_reminder_text(&original_text, &existing);

        with_state(|state| {
            if let Some(note_id) = state.text_views.get(&text_view_key).copied() {
                if let Some(note) = state.notes.iter_mut().find(|note| note.id == note_id) {
                    note.text = text.clone();
                    note.reminders = reminders;
                    note.legacy_reminder = None;
                }
                state.active_note_id = Some(note_id);
            }
        });

        if text != original_text {
            let ns_text = NSString::alloc(nil).init_str(&text);
            let length: usize = msg_send![ns_text, length];
            let _: () = msg_send![text_view, setString: ns_text];
            let _: () = msg_send![text_view, setSelectedRange: NSRange::new(length as _, 0)];
        }
    }
    save_notes();
    sync_reminders();
}

fn update_frame(notification: id) {
    unsafe {
        let window: id = msg_send![notification, object];
        let frame: NSRect = msg_send![window, frame];
        let window_key = window as usize;

        with_state(|state| {
            if state.refreshing_desktops {
                return;
            }
            if let Some(suppressed_until) = state.suppressed_frame_updates.get(&window_key).copied()
            {
                if Instant::now() < suppressed_until {
                    return;
                }
                state.suppressed_frame_updates.remove(&window_key);
            }
            if let Some(note_id) = state.windows.get(&window_key).copied() {
                if let Some(note) = state.notes.iter_mut().find(|note| note.id == note_id) {
                    save_note_window_frame(note, window, frame);
                }
                state.active_note_id = Some(note_id);
            }
        });
    }
    save_notes();
}

fn activate_note(notification: id) {
    unsafe {
        let window: id = msg_send![notification, object];
        let window_key = window as usize;
        with_state(|state| {
            if let Some(note_id) = state.windows.get(&window_key).copied() {
                if state.notes.iter().any(|note| note.id == note_id) {
                    state.active_note_id = Some(note_id);
                }
            }
        });
    }
    update_menu_states();
}

fn close_note(notification: id) {
    let mut deleted_note = false;
    unsafe {
        let window: id = msg_send![notification, object];
        let window_key = window as usize;
        with_state(|state| {
            if let Some(note_id) = state.windows.remove(&window_key) {
                state.text_views.retain(|_, id| *id != note_id);
                let internal_close = state.refreshing_desktops || state.terminating;
                if !internal_close {
                    state.notes.retain(|note| note.id != note_id);
                    deleted_note = true;
                }
                if !internal_close && state.active_note_id == Some(note_id) {
                    state.active_note_id = state
                        .notes
                        .iter()
                        .rev()
                        .find(|note| note_is_on_active_desktop(state, note))
                        .map(|note| note.id);
                }
            }
        });
    }
    if deleted_note {
        save_notes();
        sync_reminders();
    }
    update_menu_states();
}

fn reminder_jobs() -> &'static Mutex<HashMap<ReminderJobKey, ReminderJob>> {
    REMINDER_JOBS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn pending_notifications() -> &'static Mutex<Vec<ReminderJob>> {
    PENDING_NOTIFICATIONS.get_or_init(|| Mutex::new(Vec::new()))
}

fn start_scheduler() {
    SCHEDULER_STARTED.get_or_init(|| {
        thread::spawn(|| loop {
            let now = Local::now().timestamp();
            let due = {
                let mut jobs = reminder_jobs().lock().unwrap();
                let due_ids: Vec<ReminderJobKey> = jobs
                    .iter()
                    .filter_map(|(id, job)| (job.notify_at <= now).then_some(*id))
                    .collect();
                due_ids
                    .into_iter()
                    .filter_map(|id| jobs.remove(&id))
                    .collect::<Vec<_>>()
            };

            for job in due {
                queue_notification(job);
            }

            thread::sleep(Duration::from_secs(SCHEDULER_TICK_SECONDS));
        });
    });
}

fn sync_reminders() {
    let now = Local::now().timestamp();
    let jobs = STATE.with(|state| {
        let state = state.borrow();
        let Some(state) = state.as_ref() else {
            return HashMap::new();
        };

        let mut jobs = HashMap::new();
        for note in &state.notes {
            for (reminder_index, reminder) in note.reminders.iter().enumerate() {
                for (lead_seconds, lead_label) in REMINDER_LEADS {
                    let notify_at = reminder.due_at - lead_seconds;
                    if notify_at <= now {
                        continue;
                    }

                    jobs.insert(
                        ReminderJobKey {
                            note_id: note.id,
                            reminder_index,
                            lead_seconds: *lead_seconds,
                        },
                        ReminderJob {
                            notify_at,
                            body: notification_body_for_reminder(&note.text, reminder),
                            when: reminder_notification_when(reminder, lead_label),
                        },
                    );
                }
            }
        }
        jobs
    });

    *reminder_jobs().lock().unwrap() = jobs;
}

fn queue_notification(job: ReminderJob) {
    pending_notifications().lock().unwrap().push(job);
    wake_notification_target();
}

fn wake_notification_target() {
    let Some(target) = NOTIFICATION_TARGET.get().copied() else {
        return;
    };

    unsafe {
        let _: () = msg_send![target as id,
            performSelectorOnMainThread: sel!(deliverPendingNotifications:)
            withObject: nil
            waitUntilDone: NO
        ];
    }
}

fn deliver_pending_notifications() {
    let jobs = {
        let mut pending = pending_notifications().lock().unwrap();
        std::mem::take(&mut *pending)
    };

    for job in jobs {
        deliver_notification(&job);
    }
}

fn deliver_notification(job: &ReminderJob) {
    if SYSTEM_NOTIFICATIONS_ALLOWED.load(Ordering::SeqCst) {
        schedule_system_notification(job);
    }
    show_system_reminder_alert(job);
}

fn schedule_system_notification(job: &ReminderJob) {
    unsafe {
        let _pool = NSAutoreleasePool::new(nil);
        let content: id = msg_send![class!(UNMutableNotificationContent), new];
        let _: () = msg_send![content, setTitle: ns_string(APP_NAME)];
        let _: () = msg_send![content, setSubtitle: ns_string(&job.when)];
        let _: () = msg_send![content, setBody: ns_string(&job.body)];
        let sound: id = msg_send![class!(UNNotificationSound), defaultSound];
        let _: () = msg_send![content, setSound: sound];

        let identifier = format!("desktop-sticky-note-{}", Local::now().timestamp_millis());
        let request: id = msg_send![class!(UNNotificationRequest),
            requestWithIdentifier: ns_string(&identifier)
            content: content
            trigger: nil
        ];
        let center: id = msg_send![class!(UNUserNotificationCenter), currentNotificationCenter];
        let _: () = msg_send![center, addNotificationRequest: request withCompletionHandler: nil];
    }
    eprintln!("scheduled reminder notification: {}", job.when);
}

fn show_system_reminder_alert(job: &ReminderJob) {
    let message = format!("{}\n\n{}", job.when, job.body);
    let script = format!(
        "beep 1\ndisplay dialog {} with title {} buttons {{\"OK\"}} default button \"OK\" with icon note",
        apple_script_string(&message),
        apple_script_string("Sticky Note Reminder"),
    );

    match Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(script)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(_) => eprintln!("spawned system reminder alert: {}", job.when),
        Err(error) => eprintln!("system reminder alert failed to spawn: {error}"),
    }
}

fn apple_script_string(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\r', "\\r")
        .replace('\n', "\\n");
    format!("\"{escaped}\"")
}

fn notification_body_for_reminder(note_text: &str, reminder: &Reminder) -> String {
    reminder
        .body
        .as_deref()
        .map(notification_body)
        .filter(|body| body != "Sticky note reminder")
        .unwrap_or_else(|| notification_body(note_text))
}

fn notification_body(text: &str) -> String {
    let text = text.trim();
    if text.is_empty() {
        return "Sticky note reminder".to_string();
    }

    let first_line = text.lines().next().unwrap_or(text).trim();
    truncate_chars(first_line, 160)
}

fn truncate_chars(value: &str, max: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn reminder_when(reminder: &Reminder) -> String {
    match (&reminder.date_text, &reminder.time_text) {
        (Some(date), Some(time)) => format!("{date} at {time}"),
        (Some(date), None) => date.clone(),
        (None, Some(time)) => time.clone(),
        _ => "Reminder".to_string(),
    }
}

fn reminder_notification_when(reminder: &Reminder, lead_label: &str) -> String {
    format!("{lead_label}: {}", reminder_when(reminder))
}

fn normalize_reminder_text(text: &str, existing: &[Reminder]) -> (String, Vec<Reminder>) {
    let mut normalized = String::with_capacity(text.len());
    let mut reminders = Vec::new();

    for segment in text.split_inclusive('\n') {
        let (line, newline) = split_line_segment(segment);
        let (line, mut line_reminders) = normalize_reminder_line(line, existing);
        normalized.push_str(&line);
        normalized.push_str(newline);
        reminders.append(&mut line_reminders);
    }

    (normalized, reminders)
}

fn split_line_segment(segment: &str) -> (&str, &str) {
    segment
        .strip_suffix('\n')
        .map(|line| (line, "\n"))
        .unwrap_or((segment, ""))
}

fn normalize_reminder_line(line: &str, existing: &[Reminder]) -> (String, Vec<Reminder>) {
    let date_tokens = find_date_tokens(line);
    let time_tokens = find_time_tokens(line);
    let normalized = normalize_line_tokens(line, &date_tokens, &time_tokens);
    let body = reminder_body_for_line(&normalized);
    let mut date_entries = Vec::new();
    let mut time_entries = Vec::new();

    for token in &date_tokens {
        push_date_entry(&mut date_entries, token.date, token.replacement.clone());
    }
    for token in &time_tokens {
        push_time_entry(&mut time_entries, token.time, token.replacement.clone());
    }

    for reminder in existing {
        if let Some(date) = visible_reminder_date(line, reminder)
            .or_else(|| visible_reminder_date(&normalized, reminder))
        {
            if let Some(text) = &reminder.date_text {
                push_date_entry(&mut date_entries, date, text.clone());
            }
        }
        if let Some(time) = visible_reminder_time(line, reminder)
            .or_else(|| visible_reminder_time(&normalized, reminder))
        {
            if let Some(text) = &reminder.time_text {
                push_time_entry(&mut time_entries, time, text.clone());
            }
        }
    }

    let mut reminders = Vec::new();
    if !date_entries.is_empty() && !time_entries.is_empty() {
        for date in &date_entries {
            for time in &time_entries {
                if let Some(reminder) = build_reminder(
                    Some(date.date),
                    Some(time.time),
                    Some(date.text.clone()),
                    Some(time.text.clone()),
                    body.clone(),
                ) {
                    push_reminder(&mut reminders, reminder);
                }
            }
        }
    } else if !date_entries.is_empty() {
        for date in &date_entries {
            if let Some(reminder) = build_reminder(
                Some(date.date),
                None,
                Some(date.text.clone()),
                None,
                body.clone(),
            ) {
                push_reminder(&mut reminders, reminder);
            }
        }
    } else if !time_entries.is_empty() {
        for time in &time_entries {
            if let Some(reminder) = build_reminder(
                None,
                Some(time.time),
                None,
                Some(time.text.clone()),
                body.clone(),
            ) {
                push_reminder(&mut reminders, reminder);
            }
        }
    }

    (normalized, reminders)
}

fn normalize_line_tokens(
    line: &str,
    date_tokens: &[DateToken],
    time_tokens: &[TimeToken],
) -> String {
    let mut normalized = line.to_string();
    let mut replacements = Vec::with_capacity(date_tokens.len() + time_tokens.len());

    for token in date_tokens {
        replacements.push((token.start, token.end, token.replacement.clone()));
    }
    for token in time_tokens {
        replacements.push((token.start, token.end, token.replacement.clone()));
    }

    replacements.sort_by(|left, right| right.0.cmp(&left.0));
    for (start, end, replacement) in replacements {
        normalized.replace_range(start..end, &replacement);
    }
    normalized
}

fn reminder_body_for_line(line: &str) -> Option<String> {
    let line = line.trim();
    (!line.is_empty()).then(|| line.to_string())
}

fn push_date_entry(entries: &mut Vec<DateEntry>, date: NaiveDate, text: String) {
    if !entries
        .iter()
        .any(|entry| entry.date == date && entry.text == text)
    {
        entries.push(DateEntry { date, text });
    }
}

fn push_time_entry(entries: &mut Vec<TimeEntry>, time: NaiveTime, text: String) {
    if !entries
        .iter()
        .any(|entry| entry.time == time && entry.text == text)
    {
        entries.push(TimeEntry { time, text });
    }
}

fn build_reminder(
    date: Option<NaiveDate>,
    time: Option<NaiveTime>,
    date_text: Option<String>,
    time_text: Option<String>,
    body: Option<String>,
) -> Option<Reminder> {
    let due_at = build_due_at(date, time)?;
    Some(Reminder {
        due_at,
        date_text,
        time_text,
        body,
    })
}

fn push_reminder(reminders: &mut Vec<Reminder>, reminder: Reminder) {
    if !reminders.iter().any(|existing| {
        existing.due_at == reminder.due_at
            && existing.date_text == reminder.date_text
            && existing.time_text == reminder.time_text
            && existing.body == reminder.body
    }) {
        reminders.push(reminder);
    }
}

fn visible_reminder_date(text: &str, reminder: &Reminder) -> Option<NaiveDate> {
    let date_text = reminder.date_text.as_ref()?;
    text.contains(date_text)
        .then(|| reminder_date(reminder))
        .flatten()
}

fn visible_reminder_time(text: &str, reminder: &Reminder) -> Option<NaiveTime> {
    let time_text = reminder.time_text.as_ref()?;
    text.contains(time_text)
        .then(|| reminder_time(reminder))
        .flatten()
}

fn reminder_date(reminder: &Reminder) -> Option<NaiveDate> {
    let due = Local.timestamp_opt(reminder.due_at, 0).single()?;
    reminder.date_text.as_ref().map(|_| due.date_naive())
}

fn reminder_time(reminder: &Reminder) -> Option<NaiveTime> {
    let due = Local.timestamp_opt(reminder.due_at, 0).single()?;
    reminder.time_text.as_ref().map(|_| due.time())
}

fn build_due_at(date: Option<NaiveDate>, time: Option<NaiveTime>) -> Option<i64> {
    let now = Local::now();
    let due_at = match (date, time) {
        (Some(date), Some(time)) => local_timestamp(date, time)?,
        (Some(date), None) => local_timestamp(
            date,
            NaiveTime::from_hms_opt(DATE_ONLY_REMINDER_HOUR, 0, 0)?,
        )?,
        (None, Some(time)) => {
            let today = now.date_naive();
            let today_at_time = local_timestamp(today, time)?;
            if today_at_time > now.timestamp() {
                today_at_time
            } else {
                local_timestamp(today + ChronoDuration::days(1), time)?
            }
        }
        (None, None) => return None,
    };

    (due_at > now.timestamp()).then_some(due_at)
}

fn local_timestamp(date: NaiveDate, time: NaiveTime) -> Option<i64> {
    match Local.from_local_datetime(&date.and_time(time)) {
        LocalResult::Single(value) => Some(value.timestamp()),
        LocalResult::Ambiguous(first, second) => Some(first.min(second).timestamp()),
        LocalResult::None => None,
    }
}

fn find_date_tokens(text: &str) -> Vec<DateToken> {
    let mut tokens = Vec::new();
    let mut consumed_until = 0;

    for (start, character) in text.char_indices() {
        if start < consumed_until {
            continue;
        }
        if !character.is_ascii_digit() || !is_token_start(text, start) {
            continue;
        }

        let Some((end, month, day, year)) = parse_date_prefix(text, start) else {
            continue;
        };
        if !is_token_end(text, end) {
            continue;
        }

        let Some(date) = NaiveDate::from_ymd_opt(year as i32, month, day) else {
            continue;
        };
        tokens.push(DateToken {
            start,
            end,
            date,
            replacement: format_date(date),
        });
        consumed_until = end;
    }
    tokens
}

fn find_time_tokens(text: &str) -> Vec<TimeToken> {
    let mut tokens = Vec::new();
    let mut consumed_until = 0;

    for (start, character) in text.char_indices() {
        if start < consumed_until {
            continue;
        }
        if !character.is_ascii_digit() || !is_token_start(text, start) {
            continue;
        }

        let Some((end, time)) = parse_time_prefix(text, start) else {
            continue;
        };
        if !is_token_end(text, end) {
            continue;
        }

        tokens.push(TimeToken {
            start,
            end,
            time,
            replacement: format_time(time),
        });
        consumed_until = end;
    }
    tokens
}

fn parse_date_prefix(text: &str, start: usize) -> Option<(usize, u32, u32, u32)> {
    let bytes = text.as_bytes();
    let (month, mut index) = parse_digits(bytes, start, 2)?;
    if bytes.get(index) != Some(&b'/') {
        return None;
    }
    index += 1;

    let (day, next) = parse_digits(bytes, index, 2)?;
    index = next;
    if bytes.get(index) != Some(&b'/') {
        return None;
    }
    index += 1;

    let mut year = 0;
    for _ in 0..4 {
        let digit = bytes.get(index).copied()?;
        if !digit.is_ascii_digit() {
            return None;
        }
        year = year * 10 + u32::from(digit - b'0');
        index += 1;
    }

    Some((index, month, day, year))
}

fn parse_time_prefix(text: &str, start: usize) -> Option<(usize, NaiveTime)> {
    let bytes = text.as_bytes();
    let (hour, mut index) = parse_digits(bytes, start, 2)?;
    if hour == 0 || hour > 12 || bytes.get(index) != Some(&b':') {
        return None;
    }
    index += 1;

    let minute_tens = bytes.get(index).copied()?;
    let minute_ones = bytes.get(index + 1).copied()?;
    if !minute_tens.is_ascii_digit() || !minute_ones.is_ascii_digit() {
        return None;
    }
    let minute = u32::from(minute_tens - b'0') * 10 + u32::from(minute_ones - b'0');
    if minute > 59 {
        return None;
    }
    index += 2;

    while bytes.get(index) == Some(&b' ') {
        index += 1;
    }

    let meridiem = bytes.get(index..index + 2)?;
    let pm = meridiem.eq_ignore_ascii_case(b"pm");
    let am = meridiem.eq_ignore_ascii_case(b"am");
    if !pm && !am {
        return None;
    }
    index += 2;

    let hour = match (hour, pm) {
        (12, false) => 0,
        (12, true) => 12,
        (_, true) => hour + 12,
        _ => hour,
    };
    Some((index, NaiveTime::from_hms_opt(hour, minute, 0)?))
}

fn parse_digits(bytes: &[u8], start: usize, max_digits: usize) -> Option<(u32, usize)> {
    let mut value = 0;
    let mut index = start;
    let mut count = 0;

    while count < max_digits {
        let Some(byte) = bytes.get(index).copied() else {
            break;
        };
        if !byte.is_ascii_digit() {
            break;
        }
        value = value * 10 + u32::from(byte - b'0');
        index += 1;
        count += 1;
    }

    (count > 0).then_some((value, index))
}

fn is_token_start(text: &str, start: usize) -> bool {
    text[..start]
        .chars()
        .next_back()
        .map_or(true, |character| !character.is_ascii_alphanumeric())
}

fn is_token_end(text: &str, end: usize) -> bool {
    text[end..]
        .chars()
        .next()
        .map_or(true, |character| !character.is_ascii_alphanumeric())
}

fn format_date(date: NaiveDate) -> String {
    format!(
        "{} {} {}, {}",
        weekday_name(date.weekday().num_days_from_sunday()),
        month_name(date.month()),
        ordinal(date.day()),
        date.year()
    )
}

fn format_time(time: NaiveTime) -> String {
    let hour = time.hour();
    let suffix = if hour >= 12 { "PM" } else { "AM" };
    let hour = match hour % 12 {
        0 => 12,
        value => value,
    };
    format!("{hour}:{:02} {suffix}", time.minute())
}

fn weekday_name(index: u32) -> &'static str {
    [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ][index as usize]
}

fn month_name(month: u32) -> &'static str {
    [
        "",
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ][month as usize]
}

fn ordinal(day: u32) -> String {
    let suffix = match day % 100 {
        11..=13 => "th",
        _ => match day % 10 {
            1 => "st",
            2 => "nd",
            3 => "rd",
            _ => "th",
        },
    };
    format!("{day}{suffix}")
}

fn set_background(sender: id) {
    let Some(choice) = tagged_choice(sender, BACKGROUNDS) else {
        return;
    };
    update_active_style(|style| {
        style.background = choice.key.to_string();
        if choice.key == "black" {
            style.text_color = "white".to_string();
        } else if style.text_color == "white" {
            style.text_color = "black".to_string();
        }
    });
}

fn set_text_color(sender: id) {
    let Some(choice) = tagged_choice(sender, TEXT_COLORS) else {
        return;
    };
    update_active_style(|style| style.text_color = choice.key.to_string());
}

fn set_font(sender: id) {
    let index = sender_tag(sender);
    let Some(choice) = FONTS.get(index) else {
        return;
    };
    update_active_style(|style| style.font = choice.key.to_string());
}

fn set_font_size(sender: id) {
    let index = sender_tag(sender);
    let Some(size) = FONT_SIZES.get(index).copied() else {
        return;
    };
    update_active_style(|style| style.font_size = size);
}

fn tagged_choice<'a>(sender: id, choices: &'a [ColorChoice]) -> Option<&'a ColorChoice> {
    choices.get(sender_tag(sender))
}

fn sender_tag(sender: id) -> usize {
    unsafe {
        let tag: isize = msg_send![sender, tag];
        tag.max(0) as usize
    }
}

fn update_active_style<F>(mut update: F)
where
    F: FnMut(&mut NoteStyle),
{
    let note_id = STATE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(state) = state.as_mut() else {
            return None;
        };

        let note_id = state
            .active_note_id
            .filter(|note_id| {
                state
                    .notes
                    .iter()
                    .any(|note| note.id == *note_id && note_is_on_active_desktop(state, note))
            })
            .or_else(|| {
                state
                    .notes
                    .iter()
                    .rev()
                    .find(|note| note_is_on_active_desktop(state, note))
                    .map(|note| note.id)
            });

        if let Some(note_id) = note_id {
            if let Some(note) = state.notes.iter_mut().find(|note| note.id == note_id) {
                update(&mut note.style);
                state.default_style = note.style.clone();
                state.active_note_id = Some(note_id);
                return Some(note_id);
            }
        }

        update(&mut state.default_style);
        None
    });

    if let Some(note_id) = note_id {
        unsafe {
            apply_style_to_note(note_id);
        }
    }
    update_menu_states();
    save_notes();
}

unsafe fn apply_style_to_note(note_id: u64) {
    let Some((window, text_view, style)) = note_ui(note_id) else {
        return;
    };

    let background = background_color(&style.background);
    let text = text_color(&style.text_color);
    let font = note_font(&style);
    let _: () = msg_send![window, setBackgroundColor: background];
    let _: () = msg_send![text_view, setTextColor: text];
    let _: () = msg_send![text_view, setInsertionPointColor: text];
    let _: () = msg_send![text_view, setFont: font];
    let _: () = msg_send![text_view, setNeedsDisplay: YES];
}

fn note_ui(note_id: u64) -> Option<(id, id, NoteStyle)> {
    STATE.with(|state| {
        let state = state.borrow();
        let state = state.as_ref()?;
        let window = state
            .windows
            .iter()
            .find_map(|(window, id)| (*id == note_id).then_some(*window as id))?;
        let text_view = state
            .text_views
            .iter()
            .find_map(|(text_view, id)| (*id == note_id).then_some(*text_view as id))?;
        let style = state
            .notes
            .iter()
            .find(|note| note.id == note_id)?
            .style
            .clone();
        Some((window, text_view, style))
    })
}

fn current_style() -> NoteStyle {
    STATE.with(|state| {
        let state = state.borrow();
        let Some(state) = state.as_ref() else {
            return NoteStyle::default();
        };
        if let Some(note_id) = state.active_note_id {
            if let Some(note) = state
                .notes
                .iter()
                .find(|note| note.id == note_id && note_is_on_active_desktop(state, note))
            {
                return note.style.clone();
            }
        }
        state.default_style.clone()
    })
}

fn update_menu_states() {
    let style = current_style();
    STATE.with(|state| {
        let state = state.borrow();
        let Some(state) = state.as_ref() else {
            return;
        };
        unsafe {
            for choice in &state.background_items {
                set_menu_state(choice.item, choice.key == style.background);
            }
            for choice in &state.text_color_items {
                set_menu_state(choice.item, choice.key == style.text_color);
            }
            for choice in &state.font_items {
                set_menu_state(choice.item, choice.key == style.font);
            }
            for choice in &state.font_size_items {
                set_menu_state(choice.item, (choice.size - style.font_size).abs() < 0.1);
            }
        }
    });
}

unsafe fn set_menu_state(item: usize, selected: bool) {
    let value = if selected { 1isize } else { 0isize };
    let _: () = msg_send![item as id, setState: value];
}

unsafe fn note_context_menu() -> id {
    let menu = NSMenu::new(nil).autorelease();
    let delegate = STATE.with(|state| state.borrow().as_ref().unwrap().delegate);
    let _: () = msg_send![menu, setDelegate: delegate];
    let _: () = msg_send![menu, addItem: menu_item("New Note", sel!(newNote:))];
    let _: () = msg_send![
        menu,
        addItem: menu_item(
            "Move Selected Note to This Desktop",
            sel!(moveSelectedNoteHere:)
        )
    ];
    add_separator(menu);
    let _ = add_color_submenu(menu, "Note Color", BACKGROUNDS, sel!(setBackground:));
    let _ = add_color_submenu(menu, "Text Color", TEXT_COLORS, sel!(setTextColor:));
    let _ = add_font_submenu(menu);
    let _ = add_font_size_submenu(menu);
    menu
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
            decl.add_method(
                sel!(moveSelectedNoteHere:),
                move_selected_note_here as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(sel!(quit:), quit as extern "C" fn(&Object, Sel, id));
            decl.add_method(
                sel!(setBackground:),
                set_background_action as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(setTextColor:),
                set_text_color_action as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(setFont:),
                set_font_action as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(setFontSize:),
                set_font_size_action as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(menuWillOpen:),
                menu_will_open as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(activeSpaceDidChange:),
                active_space_did_change as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(userNotificationCenter:shouldPresentNotification:),
                should_present_notification as extern "C" fn(&Object, Sel, id, id) -> BOOL,
            );
            decl.add_method(
                sel!(userNotificationCenter:willPresentNotification:withCompletionHandler:),
                will_present_un_notification as extern "C" fn(&Object, Sel, id, id, id),
            );
            decl.add_method(
                sel!(deliverPendingNotifications:),
                deliver_pending_notifications_action as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(textDidChange:),
                text_did_change as extern "C" fn(&Object, Sel, id),
            );
            decl.add_method(
                sel!(windowDidBecomeKey:),
                window_did_become_key as extern "C" fn(&Object, Sel, id),
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
    with_state(|state| state.terminating = true);
    persist_visible_window_frames(false);
    save_notes();
}

extern "C" fn new_note(_: &Object, _: Sel, _: id) {
    create_note(None);
}

extern "C" fn move_selected_note_here(_: &Object, _: Sel, _: id) {
    move_active_note_to_current_desktop();
}

extern "C" fn quit(_: &Object, _: Sel, _: id) {
    with_state(|state| state.terminating = true);
    persist_visible_window_frames(false);
    save_notes();
    unsafe {
        let _: () = msg_send![NSApp(), terminate: nil];
    }
}

extern "C" fn set_background_action(_: &Object, _: Sel, sender: id) {
    set_background(sender);
}

extern "C" fn set_text_color_action(_: &Object, _: Sel, sender: id) {
    set_text_color(sender);
}

extern "C" fn set_font_action(_: &Object, _: Sel, sender: id) {
    set_font(sender);
}

extern "C" fn set_font_size_action(_: &Object, _: Sel, sender: id) {
    set_font_size(sender);
}

extern "C" fn menu_will_open(_: &Object, _: Sel, _: id) {
    update_menu_states();
}

extern "C" fn active_space_did_change(_: &Object, _: Sel, _: id) {
    active_space_changed();
}

extern "C" fn should_present_notification(_: &Object, _: Sel, _: id, _: id) -> BOOL {
    YES
}

extern "C" fn will_present_un_notification(
    _: &Object,
    _: Sel,
    _: id,
    _: id,
    completion_handler: id,
) {
    let options = UN_NOTIFICATION_PRESENTATION_OPTION_BANNER
        | UN_NOTIFICATION_PRESENTATION_OPTION_LIST
        | UN_NOTIFICATION_PRESENTATION_OPTION_SOUND
        | UN_NOTIFICATION_PRESENTATION_OPTION_ALERT;
    unsafe {
        let block = &*(completion_handler as *const Block<(u64,), ()>);
        block.call((options,));
    }
}

extern "C" fn deliver_pending_notifications_action(_: &Object, _: Sel, _: id) {
    if std::panic::catch_unwind(deliver_pending_notifications).is_err() {
        eprintln!("reminder delivery failed before showing alert");
    }
}

extern "C" fn text_did_change(_: &Object, _: Sel, notification: id) {
    update_text(notification);
}

extern "C" fn window_did_become_key(_: &Object, _: Sel, notification: id) {
    activate_note(notification);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn future_date() -> NaiveDate {
        Local::now().date_naive() + ChronoDuration::days(30)
    }

    fn numeric_date(date: NaiveDate) -> String {
        format!("{:02}/{:02}/{}", date.month(), date.day(), date.year())
    }

    fn test_note(id: u64, desktop_id: &str) -> Note {
        let placement = NotePlacement {
            x: 40.0 + id as f64,
            y: 50.0 + id as f64,
            w: DEFAULT_W,
            h: DEFAULT_H,
        };
        Note {
            id,
            desktop_id: desktop_id.to_string(),
            x: placement.x,
            y: placement.y,
            w: placement.w,
            h: placement.h,
            text: String::new(),
            style: NoteStyle::default(),
            placements: HashMap::from([(desktop_id.to_string(), placement)]),
            reminders: Vec::new(),
            legacy_reminder: None,
        }
    }

    fn test_state(
        notes: Vec<Note>,
        active_desktop_id: &str,
        active_note_id: Option<u64>,
    ) -> AppState {
        AppState {
            notes,
            windows: HashMap::new(),
            text_views: HashMap::new(),
            data_path: PathBuf::new(),
            delegate: nil,
            status_item: nil,
            status_menu: nil,
            next_id: 1,
            saving: false,
            active_note_id,
            default_style: NoteStyle::default(),
            background_items: Vec::new(),
            text_color_items: Vec::new(),
            font_items: Vec::new(),
            font_size_items: Vec::new(),
            active_desktop_ids: HashSet::from([active_desktop_id.to_string()]),
            refreshing_desktops: false,
            terminating: false,
            suppressed_frame_updates: HashMap::new(),
        }
    }

    #[test]
    fn move_selection_keeps_hidden_active_note() {
        let state = test_state(
            vec![test_note(1, "space:one"), test_note(2, "space:two")],
            "space:two",
            Some(1),
        );

        assert_eq!(selected_note_id_for_move(&state), Some(1));
    }

    #[test]
    fn move_selection_falls_back_to_visible_note() {
        let state = test_state(
            vec![test_note(1, "space:one"), test_note(2, "space:two")],
            "space:two",
            None,
        );

        assert_eq!(selected_note_id_for_move(&state), Some(2));
    }

    #[test]
    fn clamps_new_desktop_placement_to_visible_frame() {
        let target = NotePlacementTarget {
            desktop_id: "space:two".to_string(),
            visible_frame: Some(NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(500.0, 400.0),
            )),
        };
        let placement = clamp_placement_to_target(
            NotePlacement {
                x: -300.0,
                y: 900.0,
                w: 900.0,
                h: 900.0,
            },
            &target,
        );

        assert_eq!(placement.x, 20.0);
        assert_eq!(placement.y, 20.0);
        assert_eq!(placement.w, 460.0);
        assert_eq!(placement.h, 360.0);
    }

    #[test]
    fn parses_managed_space_ids_from_desktop_ids() {
        assert_eq!(
            managed_space_id_from_desktop_id("display:abc/space:id:546"),
            Some(546)
        );
        assert_eq!(
            managed_space_id_from_desktop_id("display:abc/space:managed:57"),
            Some(57)
        );
        assert_eq!(
            managed_space_id_from_desktop_id("display:abc/space:uuid:not-numeric"),
            None
        );
    }

    #[test]
    fn extracts_display_identifier_from_desktop_id() {
        assert_eq!(
            desktop_display_identifier("display:abc/space:id:1"),
            Some("abc")
        );
        assert_eq!(desktop_display_identifier("space:id:1"), None);
    }

    #[test]
    fn parses_multiple_reminder_lines() {
        let date = future_date();
        let raw = format!(
            "Call {} 9:15am\nPay {} 10:20pm\nStretch 11:25am",
            numeric_date(date),
            numeric_date(date)
        );

        let (normalized, reminders) = normalize_reminder_text(&raw, &[]);

        assert!(normalized.contains(&format_date(date)));
        assert!(normalized.contains("9:15 AM"));
        assert!(normalized.contains("10:20 PM"));
        assert!(normalized.contains("11:25 AM"));
        assert_eq!(reminders.len(), 3);
    }

    #[test]
    fn parses_multiple_times_on_one_dated_line() {
        let date = future_date();
        let raw = format!("{} calls at 9:15am and 10:20pm", numeric_date(date));

        let (_, reminders) = normalize_reminder_text(&raw, &[]);

        assert_eq!(reminders.len(), 2);
        assert!(reminders
            .iter()
            .all(|reminder| reminder.date_text.as_deref() == Some(format_date(date).as_str())));
        assert!(reminders
            .iter()
            .any(|reminder| reminder.time_text.as_deref() == Some("9:15 AM")));
        assert!(reminders
            .iter()
            .any(|reminder| reminder.time_text.as_deref() == Some("10:20 PM")));
    }

    #[test]
    fn combines_new_time_with_existing_visible_date() {
        let date = future_date();
        let existing = Reminder {
            due_at: build_due_at(Some(date), None).unwrap(),
            date_text: Some(format_date(date)),
            time_text: None,
            body: Some("Dentist".to_string()),
        };
        let raw = format!("Dentist {} 9:45am", existing.date_text.as_deref().unwrap());

        let (normalized, reminders) = normalize_reminder_text(&raw, &[existing]);

        assert!(normalized.contains("9:45 AM"));
        assert_eq!(reminders.len(), 1);
        assert_eq!(
            reminders[0].date_text.as_deref(),
            Some(format_date(date).as_str())
        );
        assert_eq!(reminders[0].time_text.as_deref(), Some("9:45 AM"));
    }

    #[test]
    fn migrates_legacy_single_reminder() {
        let mut note: Note = serde_json::from_str(
            r#"{
                "id": 1,
                "desktop_id": "",
                "x": 0.0,
                "y": 0.0,
                "w": 260.0,
                "h": 210.0,
                "text": "Legacy",
                "reminder": {
                    "due_at": 1999999999,
                    "date_text": null,
                    "time_text": "9:00 AM"
                }
            }"#,
        )
        .unwrap();

        migrate_note_reminders(&mut note);
        let saved = serde_json::to_string(&note).unwrap();

        assert_eq!(note.reminders.len(), 1);
        assert!(saved.contains("\"reminders\""));
        assert!(!saved.contains("\"reminder\""));
    }
}
