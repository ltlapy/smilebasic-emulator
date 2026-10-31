//! The emulator's menu bar, drawn over the screen at display resolution.
//! It stays hidden until the mouse reaches the top edge or F8 shows it,
//! so it takes no keys away from programs.

use sb_runtime::Framebuffer;
use sb_runtime::editor::EditorSize;
use sb_runtime::font::{Font, FontSize};
use sb_runtime::palette::PRESETS;
use sb_runtime::runtime::Settings;
use sb_runtime::screen::Filter;

use crate::ui::{
    BACKGROUND, BORDER, DISABLED, HIGHLIGHT, PAD, Rect, TEXT, draw_text, fill, outline, text_width,
};

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
    ToggleEditor,
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
    /// The debugger's commands, for a stopped program.
    Continue,
    StepInto,
    StepOver,
    StepOut,
    RunToCursor,
    JumpToCursor,
    ToggleBreakpoint,
    ClearBreakpoints,
    /// Run the program RUN last started again.
    Restart,
    ToggleDebugOpenEditor,
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
}

impl Action {
    /// The mark before an item: a check box for toggles, a radio button
    /// for choices, blank for commands.
    fn mark(self, settings: &Settings) -> &'static str {
        let check = |on: bool| if on { "[x]" } else { "[ ]" };
        match self {
            Action::ToggleUpperCase => check(settings.upper_case_letters),
            Action::ToggleWrap => check(settings.editor_wrap),
            Action::ToggleNumberTiles => check(settings.editor_number_tiles),
            Action::ToggleMap => check(settings.editor_map),
            Action::ToggleKeyboardButtons => check(settings.keyboard_buttons),
            Action::ToggleDebugOpenEditor => check(settings.debug_open_editor),
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
            Action::Continue | Action::StepInto | Action::StepOver | Action::StepOut => {
                context.paused
            }
            Action::RunToCursor | Action::JumpToCursor => context.paused && context.editing,
            Action::ToggleBreakpoint => context.editing,
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
            | Action::Projects
            | Action::SaveSlot
            | Action::LoadSlot
            | Action::Run
            | Action::Help
            | Action::GoToLine
            | Action::Find
            | Action::Replace
            | Action::Index => !context.running,
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
            item("Upper-case letters", Action::ToggleUpperCase),
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
        ],
    },
    Menu {
        title: "Run",
        items: &[
            keyed("Run PRG0", "F5", Action::Run),
            keyed("Stop", "Pause", Action::Stop),
            keyed("Pause/Resume", "F8", Action::Pause),
            LINE,
            item("Keys press buttons", Action::ToggleKeyboardButtons),
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
            item("Clear breakpoints", Action::ClearBreakpoints),
            LINE,
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
fn item_text(item: &Item, settings: &Settings, marks: bool) -> String {
    match item.entry {
        _ if !marks => item.label.to_string(),
        Entry::Do(action) => format!("{} {}", action.mark(settings), item.label),
        Entry::Sub(_) => format!("    {}", item.label),
        Entry::Line => String::new(),
    }
}

/// Whether a list has check boxes or radio buttons, which leave room
/// before every label.
fn has_marks(items: &[Item]) -> bool {
    let settings = Settings::default();
    (items.iter()).any(|i| matches!(i.entry, Entry::Do(a) if a.mark(&settings) != "   "))
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
    let settings = Settings::default();
    let widest = |text: &dyn Fn(&Item) -> u32| items.iter().map(text).max().unwrap_or(0);
    let marks = has_marks(items);
    let labels = widest(&|i| text_width(&item_text(i, &settings, marks)));
    let keys = widest(&|i| text_width(item_keys(i)));
    labels + if keys > 0 { keys + 4 * PAD } else { 0 } + 3 * PAD
}

fn item_rect(menu: usize, item: usize) -> Rect {
    let items = MENUS[menu].items;
    Rect {
        x: title_rect(menu).x,
        y: ROW_HEIGHT + item_top(items, item),
        w: list_width(items),
        h: item_height(&items[item]),
    }
}

/// An item of the submenu that item `parent` of `menu` opens.
fn sub_rect(menu: usize, parent: usize, item: usize) -> Rect {
    let p = item_rect(menu, parent);
    let Entry::Sub(items) = MENUS[menu].items[parent].entry else {
        unreachable!("only submenu items have submenus")
    };
    Rect {
        x: p.x + p.w,
        y: p.y + item_top(items, item),
        w: list_width(items),
        h: item_height(&items[item]),
    }
}

#[derive(Debug, Default)]
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
    /// What the emulator is doing, on the bar's right: the project, the
    /// slot, a running or paused program.
    pub status: String,
}

impl MenuBar {
    pub fn visible(&self) -> bool {
        self.pinned
            || self.shown
            || self.open.is_some()
            || self
                .mouse
                .is_some_and(|(_, y)| y >= 0 && y < ROW_HEIGHT as i32)
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
        if let Some(i) = (0..MENUS[menu].items.len()).find(|&i| item_rect(menu, i).contains(at)) {
            self.sub = matches!(MENUS[menu].items[i].entry, Entry::Sub(_)).then_some(i);
        }
    }

    fn close(&mut self) {
        self.open = None;
        self.sub = None;
    }

    /// A left click at a display position. `context` enables and
    /// disables items.
    pub fn click(&mut self, at: (i32, i32), context: Context) -> Click {
        if let Some((menu, parent, items)) = self.submenu() {
            for (i, item) in items.iter().enumerate() {
                if let Entry::Do(action) = item.entry
                    && sub_rect(menu, parent, i).contains(at)
                {
                    if !action.enabled(context) {
                        return Click::Consumed;
                    }
                    self.close();
                    return Click::Chosen(action);
                }
            }
        }
        if let Some(menu) = self.open {
            for (i, item) in MENUS[menu].items.iter().enumerate() {
                if !item_rect(menu, i).contains(at) {
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

    /// Draws the bar, and the open menus, onto a transparent overlay.
    pub fn draw(&self, fb: &mut Framebuffer, font: &Font, settings: &Settings, context: Context) {
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
        let x = fb.width().saturating_sub(text_width(&self.status) + PAD);
        if x > last.x + last.w + PAD {
            draw_text(fb, font, x, 4, &self.status, TEXT);
        }
        let Some(open) = self.open else { return };
        let items = MENUS[open].items;
        let rects: Vec<Rect> = (0..items.len()).map(|i| item_rect(open, i)).collect();
        self.draw_list(fb, font, settings, context, items, &rects, self.sub);
        if let Some((menu, parent, sub)) = self.submenu() {
            let rects: Vec<Rect> = (0..sub.len()).map(|i| sub_rect(menu, parent, i)).collect();
            self.draw_list(fb, font, settings, context, sub, &rects, None);
        }
    }

    /// Draws a dropdown list with a border; `selected` stays highlighted
    /// while its submenu is open.
    #[allow(clippy::too_many_arguments)]
    fn draw_list(
        &self,
        fb: &mut Framebuffer,
        font: &Font,
        settings: &Settings,
        context: Context,
        items: &[Item],
        rects: &[Rect],
        selected: Option<usize>,
    ) {
        let marks = has_marks(items);
        for (i, (item, &r)) in items.iter().zip(rects).enumerate() {
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
                &item_text(item, settings, marks),
                color,
            );
            let keys = item_keys(item);
            let x = r.x + r.w - PAD - text_width(keys);
            draw_text(fb, font, x, r.y + 4, keys, color);
        }
        if let (Some(&first), Some(&last)) = (rects.first(), rects.last()) {
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
    };
    const RUNNING: Context = Context {
        running: true,
        editing: false,
        paused: false,
    };
    const EDITING: Context = Context {
        running: false,
        editing: true,
        paused: false,
    };

    fn menu(title: &str) -> usize {
        MENUS.iter().position(|m| m.title == title).unwrap()
    }

    fn at(r: Rect) -> (i32, i32) {
        ((r.x + 1) as i32, (r.y + 1) as i32)
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
    fn open_menu_and_choose() {
        let mut bar = MenuBar::default();
        bar.mouse_move(Some((10, 5)));
        let run = menu("Run");
        let keys = (MENUS[run].items.iter())
            .position(|i| i.label == "Keys press buttons")
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
        assert_eq!(Action::ToggleKeyboardButtons.mark(&settings), "[ ]");
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
        assert_eq!(Action::ScreenFilter(None).mark(&settings), "(*)");
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
        assert_eq!(Action::ColorPreset(2).mark(&settings), "(*)");
        assert_eq!(Action::ColorPreset(0).mark(&settings), "( )");
        Action::EditorSize(EditorSize::Small).toggle(&mut settings);
        assert_eq!(settings.editor_size, EditorSize::Small);
    }
}
