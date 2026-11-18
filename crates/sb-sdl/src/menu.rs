//! The emulator's menu bar, drawn over the screen at display resolution,
//! or at 1x where the display is scaled below (see
//! [`crate::window::menu_place`]). Its menus open within the room it is
//! given. It stays hidden until the mouse reaches the top edge or F8 shows it,
//! so it takes no keys away from programs; while one runs, only F8 shows
//! it, as the top of the screen is the program's.

use sb_runtime::Framebuffer;
use sb_runtime::editor::EditorSize;
use sb_runtime::font::{Font, FontSize};
use sb_runtime::palette::PRESETS;
use sb_runtime::runtime::Settings;
use sb_runtime::screen::Filter;

use crate::editor_layout::SIZES;
use crate::ui::{
    BACKGROUND, BORDER, DISABLED, HIGHLIGHT, PAD, Rect, TEXT, draw_text, fill, outline, text_width,
};
use crate::window::SCALES;

const ROW_HEIGHT: u32 = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Projects,
    /// Ctrl+S's and Ctrl+L's windows.
    SaveSlot,
    LoadSlot,
    OpenFolder,
    /// The settings window.
    Settings,
    Quit,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Comment,
    Uncomment,
    /// Run PRG0, as F5 does.
    Run,
    Stop,
    /// Pause or resume the program, with the menu bar shown (F8).
    Pause,
    /// Give the keys and controls to the subprogram, or back to the main
    /// program, as ENVFOCUS does.
    SwitchFocus,
    /// SUBSTOP.
    StopSub,
    ToggleEditor,
    /// Show the editor in a window of its own.
    ToggleEditorWindow,
    ToggleTidyOnEnd,
    EditSlot(usize),
    GoToLine,
    Find,
    Replace,
    Index,
    Split,
    ToggleUpperCase,
    ToggleWrap,
    ToggleNumberTiles,
    ToggleMap,
    ToggleKeyboardButtons,
    EditorFont(FontSize),
    EditorSize(EditorSize),
    /// Use one of the preset palettes in the editor.
    ColorPreset(usize),
    /// Open the editor colors window.
    EditColors,
    ScreenFilter(Option<Filter>),
    /// The main window covers its display, or not (Ctrl+Shift+F11).
    ToggleFullscreen,
    /// The main window's size: an item of [`SCALES`].
    WindowSize(usize),
    /// The main window keeps 16:9 while it is resized.
    ToggleKeepShape,
    /// The debugger's commands, for a stopped program.
    Continue,
    StepInto,
    StepOver,
    StepOut,
    RunToCursor,
    JumpToCursor,
    ToggleBreakpoint,
    /// Type the condition of the breakpoint at the editor's cursor.
    BreakpointCondition,
    ClearBreakpoints,
    /// Run the program RUN last started again.
    Restart,
    /// Put the debugger's panel away, or show it again.
    DebugPanel,
    ToggleDebugOpenEditor,
    /// How large the debugger panel's text is drawn.
    DebugTextSize(u32),
    /// The help for the command at the editor's cursor (F1).
    Help,
    /// The window listing the keys.
    Shortcuts,
    About,
}

/// What the menu bar needs to know of the emulator to enable its items.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Context {
    pub running: bool,
    pub editing: bool,
    /// A program is stopped where it can go on.
    pub paused: bool,
    /// The editor shows in a window of its own, and can open while a
    /// program runs.
    pub detached: bool,
    /// A subprogram runs beside the main program.
    pub sub: bool,
    /// The main window covers its display.
    pub fullscreen: bool,
    /// The item of View > Window size the main window is at.
    pub window_size: Option<usize>,
    /// The items of View > Window size that fit the display, as bits.
    pub window_fits: u32,
    /// The items of View > Size that look the same as another in the
    /// window the editor shows in, as bits (see
    /// [`crate::editor_layout::distinct_sizes`]). They show disabled.
    pub same_sizes: u32,
}

impl Action {
    /// The mark before an item: a check box for toggles, a radio button
    /// for choices, blank for commands.
    fn mark(self, settings: &Settings, context: Context) -> &'static str {
        let check = |on: bool| if on { "[x]" } else { "[ ]" };
        match self {
            // The window's own state, as the system may change it too.
            Action::ToggleFullscreen => check(context.fullscreen),
            Action::WindowSize(i) if context.window_size == Some(i) => "(*)",
            Action::WindowSize(_) => "( )",
            Action::ToggleKeepShape => check(settings.keep_16_9),
            Action::ToggleUpperCase => check(settings.upper_case_letters),
            Action::ToggleEditorWindow => check(settings.editor_window),
            Action::ToggleTidyOnEnd => check(settings.tidy_on_end),
            Action::ToggleWrap => check(settings.editor_wrap),
            Action::ToggleNumberTiles => check(settings.editor_number_tiles),
            Action::ToggleMap => check(settings.editor_map),
            Action::ToggleKeyboardButtons => check(settings.keyboard_buttons),
            Action::ToggleDebugOpenEditor => check(settings.debug_open_editor),
            Action::DebugTextSize(n) if settings.debug_text_size == n => "(*)",
            Action::DebugTextSize(_) => "( )",
            Action::EditorFont(f) if settings.editor_font == f => "(*)",
            Action::EditorSize(size) if settings.editor_size == size => "(*)",
            Action::ColorPreset(i) if settings.editor_colors == PRESETS[i].1 => "(*)",
            Action::EditorFont(_) | Action::EditorSize(_) | Action::ColorPreset(_) => "( )",
            Action::ScreenFilter(f) if settings.screen_filter == f => "(*)",
            Action::ScreenFilter(_) => "( )",
            _ => "   ",
        }
    }

    /// Flips the setting of a toggle, or picks a choice.
    pub fn toggle(self, settings: &mut Settings) {
        let flag = match self {
            Action::ToggleUpperCase => &mut settings.upper_case_letters,
            Action::ToggleEditorWindow => &mut settings.editor_window,
            Action::ToggleTidyOnEnd => &mut settings.tidy_on_end,
            Action::ToggleWrap => &mut settings.editor_wrap,
            Action::ToggleNumberTiles => &mut settings.editor_number_tiles,
            Action::ToggleMap => &mut settings.editor_map,
            Action::ToggleKeyboardButtons => &mut settings.keyboard_buttons,
            Action::ToggleDebugOpenEditor => &mut settings.debug_open_editor,
            Action::ScreenFilter(f) => {
                settings.screen_filter = f;
                return;
            }
            Action::EditorFont(f) => {
                settings.editor_font = f;
                return;
            }
            Action::EditorSize(size) => {
                settings.editor_size = size;
                return;
            }
            Action::ColorPreset(i) => {
                settings.editor_colors = PRESETS[i].1;
                return;
            }
            Action::DebugTextSize(n) => {
                settings.debug_text_size = n;
                return;
            }
            _ => return,
        };
        *flag = !*flag;
    }

    /// Stop needs a running program; the editor and the project manager
    /// cannot open during one. Editing text needs the editor open; the
    /// commands that open the editor need no program running.
    fn enabled(self, context: Context) -> bool {
        match self {
            Action::Stop | Action::Pause => context.running,
            Action::SwitchFocus | Action::StopSub => context.sub,
            Action::Continue | Action::StepInto | Action::StepOver | Action::StepOut => {
                context.paused
            }
            Action::RunToCursor | Action::JumpToCursor => context.paused && context.editing,
            Action::DebugPanel => context.editing,
            Action::ToggleBreakpoint | Action::BreakpointCondition => context.editing,
            Action::Undo
            | Action::Redo
            | Action::Cut
            | Action::Copy
            | Action::Paste
            | Action::SelectAll
            | Action::Comment
            | Action::Uncomment
            | Action::Split => context.editing,
            Action::ToggleEditor
            | Action::EditSlot(_)
            | Action::Help
            | Action::GoToLine
            | Action::Find
            | Action::Replace
            | Action::Index => !context.running || context.detached,
            Action::Projects | Action::SaveSlot | Action::LoadSlot | Action::Run => {
                !context.running
            }
            Action::WindowSize(i) => context.window_fits >> i & 1 != 0,
            Action::EditorSize(size) => {
                let i = SIZES.iter().position(|&s| s == size).unwrap_or(0);
                context.same_sizes >> i & 1 == 0
            }
            _ => true,
        }
    }
}

enum Entry {
    Do(Action),
    /// Opens a submenu to the right.
    Sub(&'static [Item]),
    /// A line between groups of items.
    Line,
}

struct Item {
    label: &'static str,
    /// The keys that do the same, shown on the right.
    keys: &'static str,
    entry: Entry,
}

const fn item(label: &'static str, action: Action) -> Item {
    keyed(label, "", action)
}

const fn keyed(label: &'static str, keys: &'static str, action: Action) -> Item {
    Item {
        label,
        keys,
        entry: Entry::Do(action),
    }
}

const fn sub(label: &'static str, items: &'static [Item]) -> Item {
    Item {
        label,
        keys: "",
        entry: Entry::Sub(items),
    }
}

const LINE: Item = Item {
    label: "",
    keys: "",
    entry: Entry::Line,
};

/// The height of a line between items.
const LINE_HEIGHT: u32 = 9;

struct Menu {
    title: &'static str,
    items: &'static [Item],
}

const COLORS: &[Item] = &[
    item(PRESETS[0].0, Action::ColorPreset(0)),
    item(PRESETS[1].0, Action::ColorPreset(1)),
    item(PRESETS[2].0, Action::ColorPreset(2)),
    item(PRESETS[3].0, Action::ColorPreset(3)),
    item(PRESETS[4].0, Action::ColorPreset(4)),
    item(PRESETS[5].0, Action::ColorPreset(5)),
    LINE,
    item("Edit colors...", Action::EditColors),
];

/// View > Window size, an item for each of [`SCALES`].
const WINDOW_SIZES: [Item; SCALES.len()] = {
    let mut items = [LINE; SCALES.len()];
    let mut i = 0;
    while i < items.len() {
        items[i] = item(SCALES[i].0, Action::WindowSize(i));
        i += 1;
    }
    items
};

const MENUS: &[Menu] = &[
    Menu {
        title: "File",
        items: &[
            keyed("Projects...", "F12", Action::Projects),
            keyed("Save slot...", "Ctrl+S", Action::SaveSlot),
            keyed("Load into slot...", "Ctrl+L", Action::LoadSlot),
            LINE,
            item("Open projects folder", Action::OpenFolder),
            item("Settings...", Action::Settings),
            LINE,
            item("Quit", Action::Quit),
        ],
    },
    Menu {
        title: "Edit",
        items: &[
            keyed("Undo", "Ctrl+Z", Action::Undo),
            keyed("Redo", "Ctrl+Y", Action::Redo),
            LINE,
            keyed("Cut", "Ctrl+X", Action::Cut),
            keyed("Copy", "Ctrl+C", Action::Copy),
            keyed("Paste", "Ctrl+V", Action::Paste),
            keyed("Select all", "Ctrl+A", Action::SelectAll),
            LINE,
            keyed("Find", "Ctrl+F", Action::Find),
            keyed("Replace", "Ctrl+H", Action::Replace),
            keyed("Go to line", "Ctrl+G", Action::GoToLine),
            keyed("Labels and DEFs", "Ctrl+J", Action::Index),
            LINE,
            keyed("Comment out", "Ctrl+K", Action::Comment),
            keyed("Uncomment", "Ctrl+Shift+K", Action::Uncomment),
            LINE,
            item("Type capitals without Shift", Action::ToggleUpperCase),
        ],
    },
    Menu {
        title: "View",
        items: &[
            keyed("Editor/Direct", "F4", Action::ToggleEditor),
            sub(
                "Edit slot",
                &[
                    keyed("PRG0", "Ctrl+1", Action::EditSlot(0)),
                    keyed("PRG1", "Ctrl+2", Action::EditSlot(1)),
                    keyed("PRG2", "Ctrl+3", Action::EditSlot(2)),
                    keyed("PRG3", "Ctrl+4", Action::EditSlot(3)),
                    keyed("PRG4", "Ctrl+5", Action::EditSlot(4)),
                    keyed("PRG5", "Ctrl+6", Action::EditSlot(5)),
                ],
            ),
            keyed("Split display", "Ctrl+/", Action::Split),
            item("Editor in a separate window", Action::ToggleEditorWindow),
            LINE,
            keyed("Wrap lines", "Ctrl+W", Action::ToggleWrap),
            item("Overview on the right", Action::ToggleMap),
            item("Line number tiles", Action::ToggleNumberTiles),
            sub(
                "Font",
                &[
                    item("8 pixels", Action::EditorFont(FontSize::Small)),
                    item("16 pixels", Action::EditorFont(FontSize::Large)),
                ],
            ),
            sub(
                "Size (F2)",
                &[
                    item("Large", Action::EditorSize(EditorSize::Large)),
                    item("Medium", Action::EditorSize(EditorSize::Medium)),
                    item("Small", Action::EditorSize(EditorSize::Small)),
                    item("Tiny", Action::EditorSize(EditorSize::Tiny)),
                ],
            ),
            sub("Colors", COLORS),
            LINE,
            sub(
                "Screen filter",
                &[
                    item("Program (XSCREEN)", Action::ScreenFilter(None)),
                    item("Nearest", Action::ScreenFilter(Some(Filter::Nearest))),
                    item(
                        "SmartNearest",
                        Action::ScreenFilter(Some(Filter::SmartNearest)),
                    ),
                    item("Bilinear", Action::ScreenFilter(Some(Filter::Bilinear))),
                ],
            ),
            LINE,
            sub("Window size", &WINDOW_SIZES),
            item("Keep 16:9 when resizing", Action::ToggleKeepShape),
            keyed("Fullscreen", "Ctrl+Shift+F11", Action::ToggleFullscreen),
        ],
    },
    Menu {
        title: "Run",
        items: &[
            keyed("Run PRG0", "F5", Action::Run),
            keyed("Stop", "Pause", Action::Stop),
            keyed("Pause/Resume", "F8", Action::Pause),
            item(
                "Clear screen and sound when a program ends",
                Action::ToggleTidyOnEnd,
            ),
            LINE,
            item("Switch keys to/from subprogram", Action::SwitchFocus),
            item("Stop subprogram", Action::StopSub),
            LINE,
            item(
                "Use the keyboard as a controller",
                Action::ToggleKeyboardButtons,
            ),
        ],
    },
    Menu {
        title: "Debug",
        items: &[
            keyed("Continue", "Shift+F5", Action::Continue),
            keyed("Step into", "F6", Action::StepInto),
            keyed("Step over", "F7", Action::StepOver),
            keyed("Step out", "Shift+F6", Action::StepOut),
            keyed("Run to cursor", "Ctrl+F6", Action::RunToCursor),
            keyed("Go on from cursor", "Ctrl+Shift+F6", Action::JumpToCursor),
            LINE,
            keyed("Breakpoint", "Shift+F7", Action::ToggleBreakpoint),
            item("Breakpoint condition...", Action::BreakpointCondition),
            item("Clear breakpoints", Action::ClearBreakpoints),
            LINE,
            keyed("Show or hide the panel", "Ctrl+F7", Action::DebugPanel),
            sub(
                "Panel text",
                &[
                    item("Large", Action::DebugTextSize(2)),
                    item("Small", Action::DebugTextSize(1)),
                ],
            ),
            keyed("Restart", "Ctrl+Shift+F5", Action::Restart),
            LINE,
            item("Open the editor on stops", Action::ToggleDebugOpenEditor),
        ],
    },
    Menu {
        title: "Help",
        items: &[
            keyed("Help for the command", "F1", Action::Help),
            item("Keyboard shortcuts", Action::Shortcuts),
            LINE,
            item("About", Action::About),
        ],
    },
];

/// What a click did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Click {
    /// The click was not on the menu bar; pass it on.
    Ignored,
    /// The menu bar used the click.
    Consumed,
    Chosen(Action),
}

/// An item's text: a check box or radio button for settings, an arrow
/// for submenus, or the same width of space otherwise.
fn item_text(item: &Item, settings: &Settings, context: Context, marks: bool) -> String {
    match item.entry {
        _ if !marks => item.label.to_string(),
        Entry::Do(action) => format!("{} {}", action.mark(settings, context), item.label),
        Entry::Sub(_) => format!("    {}", item.label),
        Entry::Line => String::new(),
    }
}

/// Whether a list has check boxes or radio buttons, which leave room
/// before every label.
fn has_marks(items: &[Item]) -> bool {
    let (settings, context) = (Settings::default(), Context::default());
    (items.iter()).any(|i| matches!(i.entry, Entry::Do(a) if a.mark(&settings, context) != "   "))
}

/// What goes on an item's right: its keys, or the arrow of a submenu.
fn item_keys(item: &Item) -> &'static str {
    match item.entry {
        Entry::Sub(_) => ">",
        _ => item.keys,
    }
}

fn item_height(item: &Item) -> u32 {
    match item.entry {
        Entry::Line => LINE_HEIGHT,
        _ => ROW_HEIGHT,
    }
}

/// How far down a list item `i` starts.
fn item_top(items: &[Item], i: usize) -> u32 {
    items[..i].iter().map(item_height).sum()
}

fn title_rect(index: usize) -> Rect {
    let x = MENUS[..index]
        .iter()
        .map(|m| text_width(m.title) + 2 * PAD)
        .sum::<u32>();
    Rect {
        x,
        y: 0,
        w: text_width(MENUS[index].title) + 2 * PAD,
        h: ROW_HEIGHT,
    }
}

/// The width of a dropdown holding `items`.
fn list_width(items: &[Item]) -> u32 {
    let (settings, context) = (Settings::default(), Context::default());
    let widest = |text: &dyn Fn(&Item) -> u32| items.iter().map(text).max().unwrap_or(0);
    let marks = has_marks(items);
    let labels = widest(&|i| text_width(&item_text(i, &settings, context, marks)));
    let keys = widest(&|i| text_width(item_keys(i)));
    labels + if keys > 0 { keys + 4 * PAD } else { 0 } + 3 * PAD
}

/// The items of a submenu item.
fn sub_items(item: &Item) -> &'static [Item] {
    match item.entry {
        Entry::Sub(items) => items,
        Entry::Do(_) | Entry::Line => &[],
    }
}

/// Where a submenu `w` x `h` pixels large opens beside its parent item
/// at `p`, in `room` pixels under a bar: to the right of the parent's
/// list, or to its left where the right has no room, or against the
/// room's right edge where neither side has; level with the parent, or
/// higher where it would go past the bottom, but not over the bar.
fn sub_origin(p: Rect, (w, h): (u32, u32), (room_w, room_h): (u32, u32)) -> (u32, u32) {
    let x = if p.x + p.w + w <= room_w {
        p.x + p.w
    } else if p.x >= w {
        p.x - w
    } else {
        room_w.saturating_sub(w)
    };
    (x, p.y.min(room_h.saturating_sub(h)).max(ROW_HEIGHT))
}

/// The first item of a list of `items` that can be shown at the top for
/// the rest to reach the bottom of `room_h` pixels, under the bar: 0
/// where the whole list fits.
fn last_first(items: &[Item], room_h: u32) -> usize {
    let total = item_top(items, items.len());
    let fits = |first: &usize| ROW_HEIGHT + total - item_top(items, *first) <= room_h;
    (0..items.len())
        .find(fits)
        .unwrap_or(items.len().saturating_sub(1))
}

/// Draws a small triangle pointing up or down in the right margin of an
/// item at `r`, where a list too tall for the window goes on.
fn draw_more(fb: &mut Framebuffer, r: Rect, up: bool) {
    let (x, y) = (r.x + r.w - PAD, r.y + r.h / 2 - 2);
    for row in 0..4 {
        let half = if up { row } else { 3 - row };
        let line = Rect {
            x: x + 3 - half,
            y: y + row,
            w: 2 * half + 1,
            h: 1,
        };
        fill(fb, line, TEXT);
    }
}

/// Fits the text at the bar's right into `room` pixels: whole, or with
/// its start cut and "..." in its place, keeping the end, which says
/// whether a program runs and which slot is edited. Nothing where fewer
/// than 4 of its characters fit.
fn fit_status(status: &str, room: u32) -> Option<String> {
    if text_width(status) < room {
        return Some(status.to_string());
    }
    let chars: Vec<char> = status.chars().collect();
    (1..chars.len().saturating_sub(3))
        .map(|cut| format!("...{}", chars[cut..].iter().collect::<String>()))
        .find(|text| text_width(text) < room)
}

#[derive(Clone, Debug, Default)]
pub struct MenuBar {
    /// Mouse position in display pixels, if it is over the window.
    mouse: Option<(i32, i32)>,
    open: Option<usize>,
    /// The item of the open menu whose submenu is open.
    sub: Option<usize>,
    /// The bar stays shown, as over the editor, which leaves room for it.
    pub pinned: bool,
    /// F8 shows the bar, and pauses a running program, until pressed
    /// again.
    pub shown: bool,
    /// A program runs, so the mouse at the top edge leaves the bar hidden.
    pub running: bool,
    /// What the emulator is doing, on the bar's right: the project, the
    /// slot, a running or paused program.
    pub status: String,
    /// The room the bar's menus have, in its pixels, from the bar's top
    /// left corner: they open within it. `None` for no limit.
    pub space: Option<(u32, u32)>,
    /// The first item the open menu shows, where it is too tall for its
    /// room and has been scrolled with the wheel.
    first: usize,
}

impl MenuBar {
    /// The room the menus open in.
    fn room(&self) -> (u32, u32) {
        self.space.unwrap_or((u32::MAX / 4, u32::MAX / 4))
    }

    /// The first item `menu` shows, which may be past the top where it
    /// does not fit.
    fn first(&self, menu: usize) -> usize {
        match self.open {
            Some(open) if open == menu => {
                self.first.min(last_first(MENUS[menu].items, self.room().1))
            }
            _ => 0,
        }
    }

    /// Whether item `item` of `menu` shows in its list: not above the
    /// first item shown, and not past the room's bottom, unless it is the
    /// first.
    fn item_shown(&self, menu: usize, item: usize) -> bool {
        let first = self.first(menu);
        let r = self.item_rect(menu, item);
        item >= first && (item == first || r.y + r.h <= self.room().1)
    }

    /// Where item `item` of `menu`'s list is: under the menu's title,
    /// moved left where the list would go past the room's right edge, and
    /// up by the items scrolled past. Those have no height.
    fn item_rect(&self, menu: usize, item: usize) -> Rect {
        let items = MENUS[menu].items;
        let w = list_width(items);
        let first = self.first(menu);
        let top = ROW_HEIGHT + item_top(items, item.max(first)) - item_top(items, first);
        Rect {
            x: title_rect(menu).x.min(self.room().0.saturating_sub(w)),
            y: top,
            w,
            h: if item < first {
                0
            } else {
                item_height(&items[item])
            },
        }
    }

    /// An item of the submenu that item `parent` of `menu` opens, as
    /// [`sub_origin`] places it.
    fn sub_rect(&self, menu: usize, parent: usize, item: usize) -> Rect {
        let items = sub_items(&MENUS[menu].items[parent]);
        let size = (list_width(items), item_top(items, items.len()));
        let (x, y) = sub_origin(self.item_rect(menu, parent), size, self.room());
        Rect {
            x,
            y: y + item_top(items, item),
            w: size.0,
            h: item_height(&items[item]),
        }
    }

    /// Scrolls the open menu's list by `notches` of the wheel, up for
    /// more than 0, if `at` is over it and it is too tall for its room.
    /// True if the wheel was over the list, which then takes it.
    pub fn wheel(&mut self, at: (i32, i32), notches: i32) -> bool {
        let Some(menu) = self.open else {
            return false;
        };
        let column = self.item_rect(menu, 0);
        let over = Rect {
            y: ROW_HEIGHT,
            h: self.room().1.saturating_sub(ROW_HEIGHT),
            ..column
        };
        if !over.contains(at) {
            return false;
        }
        let most = last_first(MENUS[menu].items, self.room().1) as i64;
        let first = (self.first(menu) as i64 - i64::from(notches)).clamp(0, most) as usize;
        if first != self.first(menu) {
            self.first = first;
            self.sub = None;
        }
        true
    }

    pub fn visible(&self) -> bool {
        self.pinned
            || self.shown
            || self.open.is_some()
            || !self.running
                && self
                    .mouse
                    .is_some_and(|(_, y)| y >= 0 && y < ROW_HEIGHT as i32)
    }

    /// Opens menu `index` of the bar, for the bench, and its submenu
    /// `sub` if it has one of that label.
    pub fn open_menu(&mut self, index: usize, sub: &str) {
        let index = index.min(MENUS.len() - 1);
        self.open = Some(index);
        self.first = 0;
        self.sub = MENUS[index].items.iter().position(|i| i.label == sub);
        // Scrolled to the submenu's item where the list is too tall.
        if let Some(parent) = self.sub {
            while !self.item_shown(index, parent) && self.first < parent {
                self.first += 1;
            }
        }
    }

    /// A menu shows its items, which need the window's state.
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// The submenu items, with the open menu and parent item.
    fn submenu(&self) -> Option<(usize, usize, &'static [Item])> {
        let (menu, parent) = (self.open?, self.sub?);
        match MENUS[menu].items[parent].entry {
            Entry::Sub(items) => Some((menu, parent, items)),
            Entry::Do(_) | Entry::Line => None,
        }
    }

    /// Hovering an item of the open menu opens its submenu, or closes
    /// another one.
    pub fn mouse_move(&mut self, position: Option<(i32, i32)>) {
        self.mouse = position;
        let (Some(menu), Some(at)) = (self.open, position) else {
            return;
        };
        // An open submenu can lie over its parent's list in a narrow
        // window, and keeps the pointer then.
        if let Some((menu, parent, items)) = self.submenu()
            && (0..items.len()).any(|i| self.sub_rect(menu, parent, i).contains(at))
        {
            return;
        }
        let shown = |i: &usize| self.item_shown(menu, *i);
        if let Some(i) =
            (0..MENUS[menu].items.len()).find(|i| shown(i) && self.item_rect(menu, *i).contains(at))
        {
            self.sub = matches!(MENUS[menu].items[i].entry, Entry::Sub(_)).then_some(i);
        }
    }

    fn close(&mut self) {
        self.open = None;
        self.sub = None;
        self.first = 0;
    }

    /// Forgets the pointer and closes the open menu, as when the bar's
    /// window loses the keys.
    pub fn leave(&mut self) {
        self.mouse = None;
        self.close();
    }

    /// The bar as another window shows it, with nothing hovered or open.
    pub fn closed(&self) -> MenuBar {
        let mut bar = self.clone();
        bar.leave();
        bar
    }

    /// A left click at a display position. `context` enables and
    /// disables items.
    pub fn click(&mut self, at: (i32, i32), context: Context) -> Click {
        if let Some((menu, parent, items)) = self.submenu() {
            for (i, item) in items.iter().enumerate() {
                if !self.sub_rect(menu, parent, i).contains(at) {
                    continue;
                }
                return match item.entry {
                    Entry::Do(action) if action.enabled(context) => {
                        self.close();
                        Click::Chosen(action)
                    }
                    _ => Click::Consumed,
                };
            }
        }
        if let Some(menu) = self.open {
            for (i, item) in MENUS[menu].items.iter().enumerate() {
                if !self.item_shown(menu, i) || !self.item_rect(menu, i).contains(at) {
                    continue;
                }
                return match item.entry {
                    Entry::Do(action) if !action.enabled(context) => Click::Consumed,
                    Entry::Do(action) => {
                        self.close();
                        Click::Chosen(action)
                    }
                    Entry::Sub(_) => {
                        self.sub = Some(i);
                        Click::Consumed
                    }
                    Entry::Line => Click::Consumed,
                };
            }
        }
        if !self.visible() && self.open.is_none() {
            return Click::Ignored;
        }
        let title = (0..MENUS.len()).find(|&i| title_rect(i).contains(at));
        let was_open = self.open;
        self.close();
        match title {
            Some(i) => {
                self.open = (was_open != Some(i)).then_some(i);
                Click::Consumed
            }
            // A click outside closes an open menu without reaching the program.
            None if was_open.is_some() => Click::Consumed,
            None if at.1 >= 0 && at.1 < ROW_HEIGHT as i32 => Click::Consumed,
            None => Click::Ignored,
        }
    }

    /// The bar's height on the display.
    pub const HEIGHT: u32 = ROW_HEIGHT;

    /// Draws the bar onto a transparent frame as wide as the display; the
    /// open menus go apart, with [`MenuBar::draw_lists`].
    pub fn draw_bar(&self, fb: &mut Framebuffer, font: &Font) {
        let bar = Rect {
            x: 0,
            y: 0,
            w: fb.width(),
            h: ROW_HEIGHT,
        };
        fill(fb, bar, BACKGROUND);
        // A line under the bar sets it apart from a dark screen.
        fill(
            fb,
            Rect {
                y: ROW_HEIGHT - 2,
                h: 2,
                ..bar
            },
            BORDER,
        );
        for (i, menu) in MENUS.iter().enumerate() {
            let r = title_rect(i);
            if self.open == Some(i) || self.mouse.is_some_and(|m| r.contains(m)) {
                fill(fb, Rect { h: r.h - 2, ..r }, HIGHLIGHT);
            }
            draw_text(fb, font, r.x + PAD, r.y + 4, menu.title, TEXT);
        }
        let last = title_rect(MENUS.len() - 1);
        let room = fb.width().saturating_sub(last.x + last.w + 2 * PAD);
        if let Some(status) = fit_status(&self.status, room) {
            let x = fb.width() - text_width(&status) - PAD;
            draw_text(fb, font, x, 4, &status, TEXT);
        }
    }

    /// Draws the open menu's list and submenu, if any, below the bar.
    pub fn draw_lists(
        &self,
        fb: &mut Framebuffer,
        font: &Font,
        settings: &Settings,
        context: Context,
    ) {
        let Some(open) = self.open else { return };
        let items = MENUS[open].items;
        let shown: Vec<usize> = (0..items.len())
            .filter(|&i| self.item_shown(open, i))
            .collect();
        let rects: Vec<Rect> = (0..items.len()).map(|i| self.item_rect(open, i)).collect();
        self.draw_list(fb, font, settings, context, items, &rects, &shown, self.sub);
        // Where the list is cut, a mark says it goes on.
        if let (Some(&top), Some(&bottom)) = (shown.first(), shown.last()) {
            if top > 0 {
                draw_more(fb, rects[top], true);
            }
            if bottom + 1 < items.len() {
                draw_more(fb, rects[bottom], false);
            }
        }
        if let Some((menu, parent, sub)) = self.submenu() {
            let rects: Vec<Rect> = (0..sub.len())
                .map(|i| self.sub_rect(menu, parent, i))
                .collect();
            let all: Vec<usize> = (0..sub.len()).collect();
            self.draw_list(fb, font, settings, context, sub, &rects, &all, None);
        }
    }

    /// Draws the `shown` items of a dropdown list with a border around
    /// them; `selected` stays highlighted while its submenu is open.
    #[allow(clippy::too_many_arguments)]
    fn draw_list(
        &self,
        fb: &mut Framebuffer,
        font: &Font,
        settings: &Settings,
        context: Context,
        items: &[Item],
        rects: &[Rect],
        shown: &[usize],
        selected: Option<usize>,
    ) {
        let marks = has_marks(items);
        for &i in shown {
            let (item, r) = (&items[i], rects[i]);
            if matches!(item.entry, Entry::Line) {
                fill(fb, r, BACKGROUND);
                let line = Rect {
                    x: r.x + PAD,
                    y: r.y + r.h / 2,
                    w: r.w - 2 * PAD,
                    h: 1,
                };
                fill(fb, line, BORDER);
                continue;
            }
            let hovered = self.mouse.is_some_and(|m| r.contains(m)) || selected == Some(i);
            let enabled = match item.entry {
                Entry::Do(action) => action.enabled(context),
                Entry::Sub(_) | Entry::Line => true,
            };
            let back = if hovered && enabled {
                HIGHLIGHT
            } else {
                BACKGROUND
            };
            fill(fb, r, back);
            let color = if enabled { TEXT } else { DISABLED };
            draw_text(
                fb,
                font,
                r.x + PAD,
                r.y + 4,
                &item_text(item, settings, context, marks),
                color,
            );
            let keys = item_keys(item);
            let x = r.x + r.w - PAD - text_width(keys);
            draw_text(fb, font, x, r.y + 4, keys, color);
        }
        if let (Some(&first), Some(&last)) = (shown.first(), shown.last()) {
            let (first, last) = (rects[first], rects[last]);
            let all = Rect {
                h: last.y + last.h - first.y,
                ..first
            };
            outline(fb, all, BORDER);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDLE: Context = Context {
        running: false,
        editing: false,
        paused: false,
        detached: false,
        sub: false,
        fullscreen: false,
        window_size: None,
        window_fits: 0,
        same_sizes: 0,
    };
    const RUNNING: Context = Context {
        running: true,
        ..IDLE
    };
    const EDITING: Context = Context {
        editing: true,
        ..IDLE
    };

    fn menu(title: &str) -> usize {
        MENUS.iter().position(|m| m.title == title).unwrap()
    }

    fn at(r: Rect) -> (i32, i32) {
        ((r.x + 1) as i32, (r.y + 1) as i32)
    }

    /// Where an item is with room enough for every list.
    fn item_rect(menu: usize, item: usize) -> Rect {
        MenuBar::default().item_rect(menu, item)
    }

    fn sub_rect(menu: usize, parent: usize, item: usize) -> Rect {
        MenuBar::default().sub_rect(menu, parent, item)
    }

    #[test]
    fn hidden_until_the_mouse_reaches_the_top() {
        let mut bar = MenuBar::default();
        assert!(!bar.visible());
        assert_eq!(bar.click((10, 5), IDLE), Click::Ignored);
        bar.mouse_move(Some((10, 300)));
        assert!(!bar.visible());
        bar.mouse_move(Some((10, 5)));
        assert!(bar.visible());
    }

    #[test]
    fn only_f8_shows_the_bar_over_a_running_program() {
        let mut bar = MenuBar {
            running: true,
            ..MenuBar::default()
        };
        bar.mouse_move(Some((10, 5)));
        assert!(!bar.visible());
        assert_eq!(bar.click((10, 5), RUNNING), Click::Ignored);
        bar.shown = true;
        assert!(bar.visible());
    }

    #[test]
    fn open_menu_and_choose() {
        let mut bar = MenuBar::default();
        bar.mouse_move(Some((10, 5)));
        let run = menu("Run");
        let keys = (MENUS[run].items.iter())
            .position(|i| i.label == "Use the keyboard as a controller")
            .unwrap();
        assert_eq!(bar.click(at(title_rect(run)), IDLE), Click::Consumed);
        bar.mouse_move(Some((500, 500)));
        assert!(bar.visible(), "an open menu stays visible");
        assert_eq!(
            bar.click(at(item_rect(run, keys)), IDLE),
            Click::Chosen(Action::ToggleKeyboardButtons)
        );
        assert!(!bar.visible());

        let mut settings = Settings::default();
        Action::ToggleKeyboardButtons.toggle(&mut settings);
        assert!(!settings.keyboard_buttons);
        assert_eq!(Action::ToggleKeyboardButtons.mark(&settings, IDLE), "[ ]");
    }

    #[test]
    fn items_need_a_program_or_the_editor() {
        let mut bar = MenuBar::default();
        bar.mouse_move(Some((10, 5)));
        let run = menu("Run");
        bar.click(at(title_rect(run)), IDLE);
        assert_eq!(bar.click(at(item_rect(run, 1)), IDLE), Click::Consumed);
        assert_eq!(
            bar.click(at(item_rect(run, 1)), RUNNING),
            Click::Chosen(Action::Stop)
        );
        bar.click(at(title_rect(run)), RUNNING);
        assert_eq!(bar.click(at(item_rect(run, 0)), RUNNING), Click::Consumed);
        assert_eq!(
            bar.click(at(item_rect(run, 0)), IDLE),
            Click::Chosen(Action::Run)
        );
        let edit = menu("Edit");
        bar.click(at(title_rect(edit)), IDLE);
        assert_eq!(bar.click(at(item_rect(edit, 0)), IDLE), Click::Consumed);
        assert_eq!(
            bar.click(at(item_rect(edit, 0)), EDITING),
            Click::Chosen(Action::Undo)
        );
    }

    #[test]
    fn the_editor_opens_during_a_program_in_its_own_window() {
        assert!(!Action::ToggleEditor.enabled(RUNNING));
        let apart = Context {
            detached: true,
            ..RUNNING
        };
        assert!(Action::ToggleEditor.enabled(apart));
        assert!(Action::EditSlot(1).enabled(apart));
        assert!(!Action::Projects.enabled(apart));
    }

    #[test]
    fn a_closed_copy_has_nothing_open() {
        let mut bar = MenuBar::default();
        bar.mouse_move(Some((10, 5)));
        bar.click(at(title_rect(0)), IDLE);
        let closed = bar.closed();
        assert!(closed.open.is_none() && closed.mouse.is_none());
        assert!(bar.open.is_some());
    }

    #[test]
    fn lines_part_groups() {
        let edit = menu("Edit");
        let (redo, line, cut) = (item_rect(edit, 1), item_rect(edit, 2), item_rect(edit, 3));
        assert_eq!(line.h, LINE_HEIGHT);
        assert_eq!((line.y, cut.y), (redo.y + redo.h, line.y + LINE_HEIGHT));
        let mut bar = MenuBar::default();
        bar.mouse_move(Some((10, 5)));
        bar.click(at(title_rect(edit)), EDITING);
        assert_eq!(bar.click(at(line), EDITING), Click::Consumed);
        assert!(bar.open.is_some(), "a line does nothing");
        // Keys sit right of the widest label.
        let width = list_width(MENUS[edit].items);
        assert!(width > text_width("Labels and DEFs") + text_width("Ctrl+Shift+K"));
        assert!(!has_marks(MENUS[menu("File")].items) && has_marks(MENUS[edit].items));
    }

    #[test]
    fn filters_are_a_submenu() {
        let mut bar = MenuBar::default();
        let view = menu("View");
        let parent = (MENUS[view].items.iter())
            .position(|i| i.label == "Screen filter")
            .unwrap();
        bar.mouse_move(Some((10, 5)));
        bar.click(at(title_rect(view)), IDLE);
        bar.mouse_move(Some(at(item_rect(view, parent))));
        assert_eq!(bar.sub, Some(parent), "hovering opens the submenu");
        let bilinear = sub_rect(view, parent, 3);
        let p = item_rect(view, parent);
        assert!(bilinear.x >= p.x + p.w);
        bar.mouse_move(Some(at(bilinear)));
        assert_eq!(bar.sub, Some(parent), "moving into it keeps it open");
        assert_eq!(
            bar.click(at(bilinear), IDLE),
            Click::Chosen(Action::ScreenFilter(Some(Filter::Bilinear)))
        );
        // Clicking the parent opens it too, and another item closes it.
        bar.mouse_move(Some(at(title_rect(view))));
        bar.click(at(title_rect(view)), IDLE);
        assert_eq!(bar.click(at(p), IDLE), Click::Consumed);
        assert_eq!(bar.sub, Some(parent));
        bar.mouse_move(Some(at(item_rect(view, 0))));
        assert_eq!(bar.sub, None);

        let mut settings = Settings::default();
        Action::ScreenFilter(None).toggle(&mut settings);
        assert_eq!(settings.screen_filter, None);
        assert_eq!(Action::ScreenFilter(None).mark(&settings, IDLE), "(*)");
    }

    #[test]
    fn fullscreen_is_checked_by_the_window() {
        let view = menu("View");
        let i = (MENUS[view].items.iter())
            .position(|i| i.label == "Fullscreen")
            .unwrap();
        assert_eq!(MENUS[view].items[i].keys, "Ctrl+Shift+F11");
        let mut bar = MenuBar::default();
        bar.mouse_move(Some((10, 5)));
        bar.click(at(title_rect(view)), IDLE);
        assert_eq!(
            bar.click(at(item_rect(view, i)), RUNNING),
            Click::Chosen(Action::ToggleFullscreen),
            "also while a program runs"
        );
        let settings = Settings::default();
        let full = Context {
            fullscreen: true,
            ..IDLE
        };
        assert_eq!(Action::ToggleFullscreen.mark(&settings, IDLE), "[ ]");
        assert_eq!(Action::ToggleFullscreen.mark(&settings, full), "[x]");
    }

    #[test]
    fn window_sizes_are_a_submenu_of_the_table() {
        let view = menu("View");
        let parent = (MENUS[view].items.iter())
            .position(|i| i.label == "Window size")
            .unwrap();
        let Entry::Sub(items) = MENUS[view].items[parent].entry else {
            panic!("a submenu");
        };
        let labels: Vec<_> = items.iter().map(|i| i.label).collect();
        assert_eq!(labels, ["0.5x", "1x", "1.5x", "2x", "2.5x", "3x"]);
        // Up to 1.5x fit; the window is at 1x.
        let context = Context {
            window_size: Some(1),
            window_fits: 0b111,
            ..IDLE
        };
        let mut bar = MenuBar::default();
        bar.open_menu(view, "Window size");
        assert_eq!(bar.sub, Some(parent));
        assert_eq!(
            bar.click(at(sub_rect(view, parent, 2)), context),
            Click::Chosen(Action::WindowSize(2))
        );
        bar.open_menu(view, "Window size");
        assert_eq!(
            bar.click(at(sub_rect(view, parent, 3)), context),
            Click::Consumed,
            "2x does not fit"
        );
        let settings = Settings::default();
        assert_eq!(Action::WindowSize(1).mark(&settings, context), "(*)");
        assert_eq!(Action::WindowSize(0).mark(&settings, context), "( )");
        assert_eq!(Action::WindowSize(1).mark(&settings, IDLE), "( )");
    }

    /// Every item of every list and submenu, with the bar's menu open.
    fn every_rect(bar: &mut MenuBar) -> Vec<(Rect, bool)> {
        let mut all = Vec::new();
        for (menu, m) in MENUS.iter().enumerate() {
            bar.open_menu(menu, "");
            for (i, item) in m.items.iter().enumerate() {
                all.push((bar.item_rect(menu, i), bar.item_shown(menu, i)));
                for s in 0..sub_items(item).len() {
                    all.push((bar.sub_rect(menu, i, s), true));
                }
            }
        }
        all
    }

    #[test]
    fn menus_with_room_open_where_they_always_did() {
        let unlimited = every_rect(&mut MenuBar::default());
        assert!(unlimited.iter().all(|&(_, shown)| shown));
        // The main window's 1280x720 display and taller ones.
        for space in [(1280, 720), (1280, 960), (1280, 1280)] {
            let mut bar = MenuBar {
                space: Some(space),
                ..MenuBar::default()
            };
            assert_eq!(every_rect(&mut bar), unlimited, "{space:?}");
        }
        // The editor's own window at its smallest, 800x450, where only
        // View > Window size moves: up, from past the bottom into it.
        let mut bar = MenuBar {
            space: Some((800, 450)),
            ..MenuBar::default()
        };
        let moved: Vec<_> = (every_rect(&mut bar).into_iter().zip(&unlimited))
            .filter(|(now, before)| now != *before)
            .collect();
        assert_eq!(moved.len(), SCALES.len());
        for ((now, _), (before, _)) in &moved {
            assert!(
                now.y < before.y && now.y + now.h <= 450,
                "{now:?} {before:?}"
            );
        }
        let ((_, _), (last, _)) = moved[moved.len() - 1];
        assert!(last.y + last.h > 450, "it went past the bottom");
    }

    #[test]
    fn menus_stay_in_a_small_window() {
        // 640x480 at 1x: every list and submenu fits as it is.
        for space in [(640, 480), (800, 600), (720, 720), (1024, 768)] {
            let mut bar = MenuBar {
                space: Some(space),
                ..MenuBar::default()
            };
            for (r, shown) in every_rect(&mut bar) {
                assert!(shown, "{space:?} {r:?}");
                assert!(
                    r.x + r.w <= space.0 && r.y + r.h <= space.1,
                    "{space:?} {r:?}"
                );
            }
        }
        // Narrower: a submenu without room on the right opens on the left
        // of its list.
        let view = menu("View");
        let parent = (MENUS[view].items.iter())
            .position(|i| i.label == "Window size")
            .unwrap();
        let mut bar = MenuBar {
            space: Some((560, 480)),
            ..MenuBar::default()
        };
        bar.open_menu(view, "Window size");
        let (p, s) = (bar.item_rect(view, parent), bar.sub_rect(view, parent, 0));
        assert_eq!(s.x + s.w, p.x, "on the left");
        assert_eq!(s.y, p.y);
        let fits = Context {
            window_fits: 0b11_1111,
            ..IDLE
        };
        assert_eq!(
            bar.click(at(bar.sub_rect(view, parent, 1)), fits),
            Click::Chosen(Action::WindowSize(1))
        );
        // Neither side: against the right edge, over the list, where the
        // pointer stays with the submenu.
        let mut bar = MenuBar {
            space: Some((320, 480)),
            ..MenuBar::default()
        };
        bar.open_menu(view, "Window size");
        let s = bar.sub_rect(view, parent, 0);
        assert_eq!(s.x + s.w, 320);
        bar.mouse_move(Some(at(bar.sub_rect(view, parent, 3))));
        assert_eq!(bar.sub, Some(parent));
        // A list moves left to stay in the window.
        let help = menu("Help");
        let r = bar.item_rect(help, 0);
        assert!(r.x + r.w <= 320 && r.x < title_rect(help).x);
        // A submenu low in a short window goes up to fit.
        let mut bar = MenuBar {
            space: Some((640, 420)),
            ..MenuBar::default()
        };
        bar.open_menu(view, "Window size");
        let last = bar.sub_rect(view, parent, SCALES.len() - 1);
        assert_eq!(last.y + last.h, 420);
    }

    #[test]
    fn a_list_too_tall_scrolls_with_the_wheel() {
        // The 0.5x window size, 640x360: View does not fit under the bar.
        let view = menu("View");
        let count = MENUS[view].items.len();
        let mut bar = MenuBar {
            space: Some((640, 360)),
            ..MenuBar::default()
        };
        bar.mouse_move(Some((10, 5)));
        bar.click(at(title_rect(view)), IDLE);
        let shown = |bar: &MenuBar| (0..count).filter(|&i| bar.item_shown(view, i)).count();
        let at_first = shown(&bar);
        assert!(at_first < count && bar.item_shown(view, 0));
        assert!(!bar.item_shown(view, count - 1));
        // Every shown item is in the window, under the bar.
        for i in (0..count).filter(|&i| bar.item_shown(view, i)) {
            let r = bar.item_rect(view, i);
            assert!(r.y >= ROW_HEIGHT && r.y + r.h <= 360, "{r:?}");
        }
        // A hidden item cannot be clicked: the click lands on nothing.
        let last = bar.item_rect(view, count - 1);
        assert!(!matches!(bar.click(at(last), IDLE), Click::Chosen(_)));
        // The wheel over the list scrolls to the end, and no further.
        bar.click(at(title_rect(view)), IDLE);
        let over = at(bar.item_rect(view, 0));
        for _ in 0..count {
            assert!(bar.wheel(over, -1));
        }
        assert!(bar.item_shown(view, count - 1) && !bar.item_shown(view, 0));
        let fullscreen = (MENUS[view].items.iter())
            .position(|i| i.label == "Fullscreen")
            .unwrap();
        assert_eq!(
            bar.click(at(bar.item_rect(view, fullscreen)), IDLE),
            Click::Chosen(Action::ToggleFullscreen)
        );
        // Back up, and the wheel elsewhere is left to the window.
        bar.click(at(title_rect(view)), IDLE);
        assert!(!bar.wheel((600, 300), -1));
        for _ in 0..count {
            bar.wheel(over, 1);
        }
        assert_eq!(shown(&bar), at_first);
        assert!(bar.item_shown(view, 0));
        // Where it fits, the wheel does nothing to it.
        let mut roomy = MenuBar {
            space: Some((640, 480)),
            ..MenuBar::default()
        };
        roomy.open_menu(view, "");
        assert!(roomy.wheel(over, -1));
        assert!((0..count).all(|i| roomy.item_shown(view, i)));
        // The bench's View > Window size opens scrolled to the submenu.
        bar.open_menu(view, "Window size");
        let parent = bar.sub.unwrap();
        assert!(bar.item_shown(view, parent));
    }

    #[test]
    fn the_status_is_cut_from_its_start() {
        let status = "MYPROJECT - EDIT PRG0*";
        let full = text_width(status);
        assert_eq!(fit_status(status, full + 1).as_deref(), Some(status));
        let cut = fit_status(status, full).unwrap();
        assert!(cut.starts_with("...") && cut.ends_with("EDIT PRG0*"));
        assert!(text_width(&cut) < full);
        // Too narrow for "..." and 4 characters: nothing.
        assert_eq!(fit_status(status, text_width("...RG0*")), None);
        assert_eq!(fit_status(status, 0), None);
        // On 640 pixels at 1x, the bar's titles leave room for it.
        let last = title_rect(MENUS.len() - 1);
        assert!(fit_status(status, 640 - last.x - last.w - 2 * PAD).is_some());
    }

    #[test]
    fn keeping_16_9_is_a_check_item_of_view() {
        let view = menu("View");
        let items = MENUS[view].items;
        let i = (items.iter())
            .position(|i| i.label == "Keep 16:9 when resizing")
            .unwrap();
        // Between the window's size and fullscreen.
        assert_eq!(items[i - 1].label, "Window size");
        assert_eq!(items[i + 1].label, "Fullscreen");
        let mut bar = MenuBar::default();
        bar.open_menu(view, "");
        assert_eq!(
            bar.click(at(item_rect(view, i)), RUNNING),
            Click::Chosen(Action::ToggleKeepShape)
        );
        let mut settings = Settings::default();
        assert_eq!(Action::ToggleKeepShape.mark(&settings, IDLE), "[x]");
        settings.keep_16_9 = false;
        assert_eq!(Action::ToggleKeepShape.mark(&settings, IDLE), "[ ]");
    }

    #[test]
    fn f8_shows_the_bar_and_pause_needs_a_program() {
        let bar = MenuBar {
            shown: true,
            ..MenuBar::default()
        };
        assert!(bar.visible());
        assert!(!Action::Pause.enabled(IDLE));
        assert!(Action::Pause.enabled(RUNNING));
    }

    #[test]
    fn clicking_outside_closes() {
        let mut bar = MenuBar::default();
        bar.mouse_move(Some((10, 5)));
        bar.click((10, 5), IDLE);
        assert_eq!(bar.click((600, 400), IDLE), Click::Consumed);
        bar.mouse_move(Some((600, 400)));
        assert!(!bar.visible());
    }

    #[test]
    fn view_menu_offers_every_preset() {
        let presets: Vec<_> = COLORS
            .iter()
            .filter_map(|i| match i.entry {
                Entry::Do(Action::ColorPreset(n)) => Some((n, i.label)),
                _ => None,
            })
            .collect();
        assert_eq!(presets.len(), PRESETS.len());
        assert!(presets.iter().all(|&(n, label)| PRESETS[n].0 == label));

        let mut settings = Settings::default();
        Action::ColorPreset(2).toggle(&mut settings);
        assert_eq!(Action::ColorPreset(2).mark(&settings, IDLE), "(*)");
        assert_eq!(Action::ColorPreset(0).mark(&settings, IDLE), "( )");
        Action::EditorSize(EditorSize::Small).toggle(&mut settings);
        assert_eq!(settings.editor_size, EditorSize::Small);
    }

    #[test]
    fn sizes_that_look_the_same_are_disabled() {
        // Small and Tiny look like Medium.
        let same = Context {
            same_sizes: 0b1100,
            ..IDLE
        };
        let enabled = SIZES.map(|size| Action::EditorSize(size).enabled(same));
        assert_eq!(enabled, [true, true, false, false]);
        assert!(SIZES.iter().all(|&s| Action::EditorSize(s).enabled(IDLE)));
        // The list follows F2's order, which the bits go by.
        let view = menu("View");
        let listed: Vec<_> = (MENUS[view].items.iter())
            .filter_map(|item| match item.entry {
                Entry::Sub(sub) => Some(sub),
                _ => None,
            })
            .flatten()
            .filter_map(|item| match item.entry {
                Entry::Do(Action::EditorSize(size)) => Some(size),
                _ => None,
            })
            .collect();
        assert_eq!(listed, SIZES);
    }
}
