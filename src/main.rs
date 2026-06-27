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
const DEFAULT_BACKGROUND: &str = "yellow";
const DEFAULT_TEXT_COLOR: &str = "black";
const DEFAULT_FONT: &str = "system";
const DEFAULT_FONT_SIZE: f64 = 16.0;

const K_CG_DESKTOP_ICON_WINDOW_LEVEL_KEY: i32 = 18;
const NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES: u64 = 1 << 0;
const NS_WINDOW_COLLECTION_BEHAVIOR_STATIONARY: u64 = 1 << 4;
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
}

#[derive(Clone, Serialize, Deserialize)]
struct Note {
    id: u64,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    text: String,
    #[serde(default)]
    style: NoteStyle,
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

struct AppState {
    notes: Vec<Note>,
    windows: HashMap<usize, u64>,
    text_views: HashMap<usize, u64>,
    data_path: PathBuf,
    delegate: id,
    status_item: id,
    next_id: u64,
    saving: bool,
    active_note_id: Option<u64>,
    default_style: NoteStyle,
    background_items: Vec<MenuChoice>,
    text_color_items: Vec<MenuChoice>,
    font_items: Vec<MenuChoice>,
    font_size_items: Vec<SizeMenuChoice>,
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
                active_note_id: None,
                default_style: NoteStyle::default(),
                background_items: Vec::new(),
                text_color_items: Vec::new(),
                font_items: Vec::new(),
                font_size_items: Vec::new(),
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
    set_status_icon(button);

    let menu = NSMenu::new(nil).autorelease();
    let delegate = STATE.with(|state| state.borrow().as_ref().unwrap().delegate);
    let _: () = msg_send![menu, setDelegate: delegate];

    let new_item = menu_item("New Note", sel!(newNote:));
    let _: () = msg_send![menu, addItem: new_item];
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
        let _: () = msg_send![button, setImage: image];
        let _: () = msg_send![button, setImagePosition: 2u64];
        let _: () = msg_send![button, setImageScaling: 0u64];
    }
    let _: () = msg_send![button, setTitle: ns_string(" Sticky")];
    let _: () = msg_send![button, setToolTip: ns_string(APP_NAME)];
}

unsafe fn system_symbol(name: &str) -> id {
    msg_send![class!(NSImage),
        imageWithSystemSymbolName: ns_string(name)
        accessibilityDescription: ns_string(APP_NAME)
    ]
}

unsafe fn ns_string(value: &str) -> id {
    NSString::alloc(nil).init_str(value)
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
            style: state.default_style.clone(),
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
            | NSWindowStyleMask::NSFullSizeContentViewWindowMask,
        NSBackingStoreBuffered,
        NO,
    );

    let title = NSString::alloc(nil).init_str("");
    window.setTitle_(title);
    let _: () = msg_send![window, setTitleVisibility: 1u64];
    let _: () = msg_send![window, setTitlebarAppearsTransparent: YES];
    let _: () = msg_send![window, setMovableByWindowBackground: YES];
    let _: () = msg_send![window, setShowsResizeIndicator: NO];
    let _: () = msg_send![window, setReleasedWhenClosed: NO];
    let _: () = msg_send![window, setBackgroundColor: background_color(&note.style.background)];
    let _: () = msg_send![window, setOpaque: NO];
    let _: () = msg_send![window, setHasShadow: YES];
    let _: () = msg_send![window, setLevel: desktop_note_level()];
    let behavior = NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES
        | NS_WINDOW_COLLECTION_BEHAVIOR_STATIONARY
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

    let _: () = msg_send![scroll, setDocumentView: text_view];
    window.setContentView_(scroll);
    let _: () = msg_send![window, setDelegate: STATE.with(|state| state.borrow().as_ref().unwrap().delegate)];
    window.makeKeyAndOrderFront_(nil);

    with_state(|state| {
        state.windows.insert(window as usize, note.id);
        state.text_views.insert(text_view as usize, note.id);
        state.active_note_id = Some(note.id);
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
                state.active_note_id = Some(note_id);
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
                state.active_note_id = Some(note_id);
            }
        });
    }
}

fn close_note(notification: id) {
    unsafe {
        let window: id = msg_send![notification, object];
        let window_key = window as usize;
        with_state(|state| {
            if let Some(note_id) = state.windows.remove(&window_key) {
                state.notes.retain(|note| note.id != note_id);
                state.text_views.retain(|_, id| *id != note_id);
                if state.active_note_id == Some(note_id) {
                    state.active_note_id = state.notes.last().map(|note| note.id);
                }
            }
        });
    }
    save_notes();
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
            .or_else(|| state.notes.last().map(|note| note.id));

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
            if let Some(note) = state.notes.iter().find(|note| note.id == note_id) {
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
