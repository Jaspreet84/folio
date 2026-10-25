//! The interactive pager: scrolling, headings, contents pane, search, link
//! and code focus, history, marks, the file picker and live reload.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block as Panel, Clear, Widget};
use ratatui::{DefaultTerminal, Frame};

use crate::ansi;
use crate::doc::{self, Document};
use crate::highlight::Highlighter;
use crate::layout::{self, Rendered, TargetKind};
use crate::links::{self, LinkAction};
use crate::picker::{self, Picker, fuzzy_match};
use crate::search::{self, Match};
use crate::state::{self, GlobalMark, Pos, State};
use crate::text::{truncate, width};
use crate::theme::Theme;

const WORDS_PER_MINUTE: u32 = 230;

/// How long to wait for a key before checking the file for changes.
const WATCH_INTERVAL: Duration = Duration::from_millis(500);

/// How often to redraw the file picker while it is still scanning.
const SCAN_REFRESH: Duration = Duration::from_millis(50);

/// How many places the back list keeps.
const MAX_HISTORY: usize = 200;

const HELP: &[(&str, &str)] = &[
    ("j k  ↓ ↑", "scroll one line"),
    ("space f  b", "page down / up"),
    ("d u", "half page down / up"),
    ("g G", "top / bottom"),
    ("50%", "go to a percentage"),
    ("]] [[  } {", "next / previous heading"),
    ("t", "contents pane"),
    ("Ctrl-P", "jump to a heading by name"),
    ("/  n N", "search, next / previous match"),
    ("Tab  Shift-Tab", "next / previous link or code block"),
    ("Enter", "open the link, or copy the code"),
    ("y", "copy the link or code"),
    ("H  L", "back / forward"),
    ("m a   ' a", "set mark a / go to mark a"),
    ("m A   ' A", "marks A-Z work across files"),
    ("o", "file picker"),
    ("r  R", "raw Markdown / reload"),
    ("Esc", "clear focus and highlights"),
    ("5j  3]]", "a number repeats a motion"),
    ("q", "quit"),
];

const PICKER_HELP: &[(&str, &str)] = &[
    ("j k  ↓ ↑", "move"),
    ("g G", "first / last file"),
    ("Enter", "open the file"),
    ("/  Ctrl-P", "filter by name or title"),
    ("H  Esc", "back"),
    ("q", "quit"),
];

/// A loaded document.
pub struct Page {
    /// Canonical path, or `None` for stdin.
    path: Option<PathBuf>,
    name: String,
    source: String,
    doc: Document,
    /// Modified time and size when loaded, to notice changes.
    stamp: Option<(SystemTime, u64)>,
}

impl Page {
    pub fn open(path: &Path) -> io::Result<Page> {
        let path = path.canonicalize()?;
        let bytes = std::fs::read(&path)?;
        let source = String::from_utf8_lossy(&bytes).into_owned();
        let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
        Ok(Page { stamp: stamp(&path), doc: doc::parse(&source), path: Some(path), name, source })
    }

    pub fn from_text(name: String, source: String) -> Page {
        Page { path: None, name, doc: doc::parse(&source), source, stamp: None }
    }

    pub fn document(&self) -> &Document {
        &self.doc
    }

    /// The folder relative links resolve against.
    pub fn dir(&self) -> Option<&Path> {
        self.path.as_deref().and_then(Path::parent)
    }

    fn key(&self) -> Option<String> {
        self.path.as_ref().map(|p| p.to_string_lossy().into_owned())
    }
}

fn stamp(path: &Path) -> Option<(SystemTime, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

pub enum Start {
    Page(Page),
    Picker(PathBuf),
}

pub struct Options {
    pub max_width: usize,
    /// Reload the document when its file changes.
    pub watch: bool,
    /// Where reading positions and marks are kept; `None` keeps nothing.
    pub state_file: Option<PathBuf>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Doc,
    Picker,
}

/// A place in the back/forward history.
#[derive(Clone, Debug)]
enum Place {
    Picker(PathBuf),
    Doc { path: Option<PathBuf>, pos: Pos },
}

/// Where to put the view once a document is laid out.
enum At {
    Top,
    Pos(Pos),
    Anchor(String),
    /// Where the reader left off last time.
    Resume,
}

pub struct Pager {
    page: Page,
    /// The text read from stdin, so history can return to it.
    stdin_source: Option<(String, String)>,
    theme: Theme,
    hl: Highlighter,
    max_width: usize,
    raw: bool,
    view: Rendered,
    /// OSC 8 targets for `view.links`.
    link_urls: Vec<Option<String>>,
    /// `words_after[i]` = words from line `i` to the end.
    words_after: Vec<u32>,
    top: usize,
    body_h: usize,
    toc: Toc,
    search: Search,
    /// Index into `view.targets`.
    focus: Option<usize>,
    count: Option<usize>,
    pending: Option<(char, Option<usize>)>,
    help: bool,
    message: Option<String>,
    quit: bool,

    screen: Screen,
    picker: Option<Picker>,
    picker_root: PathBuf,
    back: Vec<Place>,
    forward: Vec<Place>,
    pending_at: Option<At>,

    state: State,
    state_file: Option<PathBuf>,
    dirty_files: HashSet<String>,
    dirty_marks: HashSet<char>,
    watch: bool,
    last_check: Instant,
}

#[derive(Default)]
struct Toc {
    open: bool,
    focused: bool,
    /// Index into the visible (possibly filtered) entries.
    selected: usize,
    scroll: usize,
    filter: Option<String>,
    /// The pane was opened by Ctrl-P and closes again after the jump.
    from_filter: bool,
}

#[derive(Default)]
struct Search {
    /// The query being typed, while the prompt is open.
    input: Option<String>,
    query: String,
    matches: Vec<Match>,
    current: Option<usize>,
    /// State to restore if the prompt is cancelled.
    saved_top: usize,
    saved: Option<(String, Option<usize>)>,
}

struct Geometry {
    area: Rect,
    header: Rect,
    footer: Rect,
    body: Rect,
    content_x: u16,
    content_w: u16,
    toc: Option<Rect>,
    toc_overlay: bool,
}

/// A link to re-print as an OSC 8 hyperlink: x, y, width, URL.
type ScreenLink = (u16, u16, u16, String);

impl Pager {
    pub fn new(start: Start, theme: Theme, hl: Highlighter, options: Options) -> Pager {
        let state = options.state_file.as_deref().map(State::load).unwrap_or_default();
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let (page, screen, picker, picker_root, pending_at) = match start {
            Start::Page(page) => {
                let root = page.dir().map_or(cwd, Path::to_path_buf);
                (page, Screen::Doc, None, root, Some(At::Resume))
            }
            Start::Picker(root) => {
                let picker = Picker::start(&root);
                (Page::from_text(String::new(), String::new()), Screen::Picker, Some(picker), root, None)
            }
        };
        let stdin_source = (page.path.is_none() && screen == Screen::Doc).then(|| (page.name.clone(), page.source.clone()));
        Pager {
            page,
            stdin_source,
            theme,
            hl,
            max_width: options.max_width.max(10),
            raw: false,
            view: Rendered::default(),
            link_urls: Vec::new(),
            words_after: vec![0],
            top: 0,
            body_h: 1,
            toc: Toc::default(),
            search: Search::default(),
            focus: None,
            count: None,
            pending: None,
            help: false,
            message: None,
            quit: false,
            screen,
            picker,
            picker_root,
            back: Vec::new(),
            forward: Vec::new(),
            pending_at,
            state,
            state_file: options.state_file,
            dirty_files: HashSet::new(),
            dirty_marks: HashSet::new(),
            watch: options.watch,
            last_check: Instant::now(),
        }
    }

    pub fn run(mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        while !self.quit {
            let size = terminal.size()?;
            let area = Rect::new(0, 0, size.width, size.height);
            if self.screen == Screen::Picker {
                self.body_h = usize::from(size.height.saturating_sub(3)).max(1);
                let (help, theme, message) = (self.help, &self.theme, self.message.as_deref());
                if let Some(picker) = self.picker.as_mut() {
                    picker.pull();
                    terminal.draw(|f| {
                        let cursor = picker.draw(f.buffer_mut(), area, theme, message);
                        if help {
                            draw_help(f.buffer_mut(), area, theme, PICKER_HELP);
                        } else if let Some(c) = cursor {
                            f.set_cursor_position(c);
                        }
                    })?;
                }
            } else {
                let geo = self.geometry(area);
                self.body_h = usize::from(geo.body.height).max(1);
                if geo.toc_overlay && self.toc.open && !self.toc.focused {
                    self.toc = Toc::default();
                    continue;
                }
                if self.view.width != usize::from(geo.content_w) {
                    self.rebuild(usize::from(geo.content_w));
                }
                self.top = self.top.min(self.view.lines.len().saturating_sub(1));
                self.scroll_toc(&geo);
                let links = self.screen_links(&geo);
                let mut cursor = None;
                let frame = terminal.draw(|f| cursor = self.draw(f, &geo))?;
                write_hyperlinks(frame.buffer, &links, cursor)?;
            }
            // While the picker is still scanning, redraw often so files appear as they are found.
            let scanning = self.screen == Screen::Picker && self.picker.as_ref().is_some_and(Picker::scanning);
            if event::poll(if scanning { SCAN_REFRESH } else { WATCH_INTERVAL })?
                && let Event::Key(key) = event::read()?
                && key.kind != KeyEventKind::Release
            {
                self.on_key(key);
            }
            self.check_watch();
        }
        self.remember();
        self.save_state();
        Ok(())
    }

    // ----- layout -------------------------------------------------------

    fn geometry(&self, area: Rect) -> Geometry {
        let (w, h) = (area.width, area.height);
        let header = Rect::new(0, 0, w, h.min(1));
        let footer = Rect::new(0, h.saturating_sub(1), w, u16::from(h >= 2));
        let body = Rect::new(0, 1, w, h.saturating_sub(2));
        let (toc, toc_overlay, text) = if self.toc.open && w >= 70 {
            let tw = (w / 3).clamp(24, 36);
            let pane = Rect::new(0, body.y, tw, body.height);
            (Some(pane), false, Rect::new(tw + 1, body.y, w - tw - 1, body.height))
        } else if self.toc.open {
            (Some(body), true, body)
        } else {
            (None, false, body)
        };
        let avail = usize::from(text.width);
        let content = if avail > 12 { (avail - 2).min(self.max_width) } else { avail.max(1) };
        let content_x = text.x + ((avail.saturating_sub(content)) / 2) as u16;
        Geometry { area, header, footer, body, content_x, content_w: content as u16, toc, toc_overlay }
    }

    /// Lay out again at `width`, keeping the reader's place (or going to
    /// the place a newly opened document asked for).
    fn rebuild(&mut self, width: usize) {
        let anchor = (!self.view.lines.is_empty()).then(|| self.view.anchor_at(self.top));
        self.view = if self.raw {
            let outline = layout::outline(&self.page.doc);
            layout::layout_raw(&self.page.source, &outline, width, &self.theme, &mut self.hl)
        } else {
            layout::layout(&self.page.doc, width, &self.theme, &mut self.hl)
        };
        self.view.width = width;
        self.link_urls = if self.theme.hyperlinks {
            self.view.links.iter().map(|u| links::osc8_target(u, self.page.dir())).collect()
        } else {
            Vec::new()
        };
        let mut words = vec![0u32; self.view.lines.len() + 1];
        for i in (0..self.view.lines.len()).rev() {
            words[i] = words[i + 1] + self.view.lines[i].words;
        }
        self.words_after = words;
        self.focus = None;
        self.top = 0;
        if let Some(at) = self.pending_at.take() {
            self.apply_at(at);
        } else if let Some(a) = anchor {
            self.top = self.view.line_for(a);
        }
        if !self.search.query.is_empty() {
            let had_current = self.search.current.is_some();
            self.search.matches = search::find(&self.view, &self.search.query);
            let i = self.search.matches.partition_point(|m| m.line() < self.top);
            self.search.current = (had_current && i < self.search.matches.len()).then_some(i);
        }
    }

    fn toggle_raw(&mut self) {
        self.raw = !self.raw;
        let width = self.view.width;
        self.rebuild(width);
    }

    // ----- movement -----------------------------------------------------

    fn max_top(&self) -> usize {
        self.view.lines.len().saturating_sub(self.body_h)
    }

    fn scroll_down(&mut self, n: usize) {
        let max = self.max_top();
        if self.top < max {
            self.top = (self.top + n).min(max);
        }
    }

    fn scroll_up(&mut self, n: usize) {
        self.top = self.top.saturating_sub(n);
    }

    /// Put `line` at the top, even past the last full page.
    fn jump(&mut self, line: usize) {
        self.top = line.min(self.view.lines.len().saturating_sub(1));
    }

    /// Jump to `line`, remembering where we were for Back.
    fn jump_with_history(&mut self, line: usize) {
        let from = self.here();
        self.jump(line);
        self.push_history(from);
    }

    /// Scroll just enough to show `line`, a third of the way down.
    fn reveal(&mut self, line: usize) {
        if line < self.top || line >= self.top + self.body_h {
            self.top = line.saturating_sub(self.body_h / 3).min(self.max_top());
        }
    }

    fn goto_percent(&mut self, percent: usize) {
        let len = self.view.lines.len();
        self.top = (len * percent.min(100) / 100).min(self.max_top());
    }

    fn current_heading(&self) -> Option<usize> {
        self.view.headings.partition_point(|h| h.line <= self.top).checked_sub(1)
    }

    fn next_heading(&mut self, n: usize) {
        for _ in 0..n {
            match self.view.headings.iter().find(|h| h.line > self.top).map(|h| h.line) {
                Some(line) => self.jump(line),
                None => {
                    self.message = Some("No more headings".into());
                    break;
                }
            }
        }
    }

    fn prev_heading(&mut self, n: usize) {
        for _ in 0..n {
            match self.view.headings.iter().rev().find(|h| h.line < self.top).map(|h| h.line) {
                Some(line) => self.jump(line),
                None => {
                    self.top = 0;
                    break;
                }
            }
        }
    }

    // ----- documents and history ----------------------------------------

    /// Where the reader is now, for the history.
    fn here(&self) -> Option<Place> {
        match self.screen {
            Screen::Picker => self.picker.as_ref().map(|p| Place::Picker(p.root.clone())),
            Screen::Doc if !self.view.lines.is_empty() => {
                Some(Place::Doc { path: self.page.path.clone(), pos: self.view.anchor_at(self.top) })
            }
            Screen::Doc => None,
        }
    }

    fn push_history(&mut self, from: Option<Place>) {
        if let Some(place) = from {
            self.back.push(place);
            if self.back.len() > MAX_HISTORY {
                self.back.remove(0);
            }
            self.forward.clear();
        }
    }

    fn navigate_doc(&mut self, path: Option<PathBuf>, at: At) {
        let from = self.here();
        if self.open_doc(path, at) {
            self.push_history(from);
        }
    }

    fn navigate_picker(&mut self, root: PathBuf) {
        let from = self.here();
        if self.show_picker(root) {
            self.push_history(from);
        }
    }

    fn go_back(&mut self) {
        let Some(place) = self.back.pop() else {
            self.message = Some("Nothing to go back to".into());
            return;
        };
        let here = self.here();
        if self.go_to(place) {
            self.forward.extend(here);
        }
    }

    fn go_forward(&mut self) {
        let Some(place) = self.forward.pop() else {
            self.message = Some("Nothing to go forward to".into());
            return;
        };
        let here = self.here();
        if self.go_to(place) {
            self.back.extend(here);
        }
    }

    fn go_to(&mut self, place: Place) -> bool {
        match place {
            Place::Picker(root) => self.show_picker(root),
            Place::Doc { path, pos } => self.open_doc(path, At::Pos(pos)),
        }
    }

    /// Show a document. A different document is loaded from disk; the same
    /// one just moves to `at`. Returns false (with a message) on failure.
    fn open_doc(&mut self, path: Option<PathBuf>, at: At) -> bool {
        let path = path.map(|p| p.canonicalize().unwrap_or(p));
        if self.screen == Screen::Doc && path == self.page.path && !self.view.lines.is_empty() {
            self.apply_at(at);
            return true;
        }
        let page = match &path {
            Some(p) => match Page::open(p) {
                Ok(page) => page,
                Err(e) => {
                    self.message = Some(format!("Can't open {}: {e}", p.display()));
                    return false;
                }
            },
            None => match &self.stdin_source {
                Some((name, source)) => Page::from_text(name.clone(), source.clone()),
                None => return false,
            },
        };
        self.remember();
        self.page = page;
        self.screen = Screen::Doc;
        // Dropping the picker stops its scan, if one is still running.
        self.picker = None;
        self.view = Rendered::default();
        self.raw = false;
        self.search = Search::default();
        self.focus = None;
        self.toc.filter = None;
        self.toc.focused = false;
        self.pending_at = Some(at);
        self.save_state();
        true
    }

    fn apply_at(&mut self, at: At) {
        match at {
            At::Top => self.top = 0,
            At::Pos(pos) => self.top = self.view.line_for(pos),
            At::Anchor(name) => match self.view.find_anchor(&name) {
                Some(line) => self.jump(line),
                None => self.message = Some(format!("No heading called #{name}")),
            },
            At::Resume => {
                let saved = self.page.key().and_then(|k| self.state.files.get(&k)).map(|f| f.pos);
                self.top = saved.map_or(0, |pos| self.view.line_for(pos));
                if self.top > 0 {
                    self.message = Some("Back where you left off · g goes to the top".into());
                }
            }
        }
    }

    fn show_picker(&mut self, root: PathBuf) -> bool {
        self.remember();
        self.save_state();
        let mut picker = Picker::start(&root);
        if let Some(path) = &self.page.path {
            picker.select_path(path);
        }
        self.picker = Some(picker);
        self.screen = Screen::Picker;
        self.toc = Toc::default();
        true
    }

    fn picker_key(&mut self, k: KeyEvent) {
        let page = self.body_h.saturating_sub(2).max(1);
        let Some(picker) = self.picker.as_mut() else { return };
        match picker.key(k, page) {
            picker::Action::None => {}
            picker::Action::Open(path) => self.navigate_doc(Some(path), At::Resume),
            picker::Action::Back => self.go_back(),
            picker::Action::Help => self.help = true,
            picker::Action::Quit => self.quit = true,
        }
    }

    // ----- reading positions and marks ----------------------------------

    /// Record the position in the current document.
    fn remember(&mut self) {
        if self.screen != Screen::Doc || self.view.lines.is_empty() {
            return;
        }
        let Some(key) = self.page.key() else { return };
        let pos = self.view.anchor_at(self.top);
        let entry = self.state.files.entry(key.clone()).or_default();
        entry.pos = pos;
        entry.seen = state::now();
        self.dirty_files.insert(key);
    }

    fn save_state(&mut self) {
        let Some(path) = &self.state_file else { return };
        match self.state.save(path, &self.dirty_files, &self.dirty_marks) {
            Ok(()) => {
                self.dirty_files.clear();
                self.dirty_marks.clear();
            }
            Err(e) => self.message = Some(format!("Can't save reading positions: {e}")),
        }
    }

    fn set_mark(&mut self, c: char) {
        let Some(key) = self.page.key() else {
            self.message = Some("Marks need a file (this is stdin)".into());
            return;
        };
        let pos = self.view.anchor_at(self.top);
        if c.is_ascii_lowercase() {
            self.state.files.entry(key.clone()).or_default().marks.insert(c, pos);
            self.dirty_files.insert(key);
        } else {
            self.state.marks.insert(c, GlobalMark { path: key, pos });
            self.dirty_marks.insert(c);
        }
        self.message = Some(format!("Mark {c} set"));
        self.save_state();
    }

    fn jump_mark(&mut self, c: char) {
        if c == '\'' {
            return self.go_back();
        }
        if c.is_ascii_lowercase() {
            let key = self.page.key();
            let pos = key.and_then(|k| self.state.files.get(&k)).and_then(|f| f.marks.get(&c)).copied();
            match pos {
                Some(pos) => self.navigate_doc(self.page.path.clone(), At::Pos(pos)),
                None => self.message = Some(format!("No mark {c} in this file")),
            }
        } else {
            match self.state.marks.get(&c).cloned() {
                Some(GlobalMark { path, pos }) => self.navigate_doc(Some(PathBuf::from(path)), At::Pos(pos)),
                None => self.message = Some(format!("No mark {c}")),
            }
        }
    }

    /// The marks that can be jumped to from here, for the prompt.
    fn mark_names(&self) -> String {
        let local = self.page.key().and_then(|k| self.state.files.get(&k)).map(|f| f.marks.keys().collect::<String>());
        let global: String = self.state.marks.keys().collect();
        let names: String = local.unwrap_or_default() + &global;
        if names.is_empty() { "no marks yet".into() } else { names.chars().map(String::from).collect::<Vec<_>>().join(" ") }
    }

    // ----- watching -----------------------------------------------------

    fn check_watch(&mut self) {
        if !self.watch || self.screen != Screen::Doc || self.last_check.elapsed() < WATCH_INTERVAL {
            return;
        }
        self.last_check = Instant::now();
        let Some(path) = self.page.path.clone() else { return };
        match stamp(&path) {
            Some(now) if Some(now) != self.page.stamp => self.reload(),
            None if self.page.stamp.is_some() => {
                self.page.stamp = None;
                self.message = Some(format!("{} was deleted or moved", self.page.name));
            }
            _ => {}
        }
    }

    fn reload(&mut self) {
        let Some(path) = self.page.path.clone() else {
            self.message = Some("Can't reload stdin".into());
            return;
        };
        match Page::open(&path) {
            Ok(page) => {
                let pos = (!self.view.lines.is_empty()).then(|| self.view.anchor_at(self.top));
                self.page = page;
                self.view = Rendered::default();
                self.pending_at = pos.map(At::Pos);
                self.message = Some(format!("Reloaded {}", self.page.name));
            }
            Err(e) => self.message = Some(format!("Can't reload: {e}")),
        }
    }

    // ----- focus, links and copying -------------------------------------

    fn target_visible(&self, i: usize) -> bool {
        let t = &self.view.targets[i];
        t.last_line() >= self.top && t.first_line() < self.top + self.body_h
    }

    /// Move focus to the next (or previous) link or code block. From an
    /// off-screen focus, start from what is on screen.
    fn next_target(&mut self, forward: bool) {
        let n = self.view.targets.len();
        if n == 0 {
            self.message = Some(if self.raw {
                "Links and code blocks are in the rendered view (press r)".into()
            } else {
                "No links or code blocks in this document".into()
            });
            return;
        }
        let bottom = self.top + self.body_h;
        let next = match self.focus {
            Some(i) if self.target_visible(i) => {
                if forward { (i + 1) % n } else { (i + n - 1) % n }
            }
            _ if forward => {
                let i = self.view.targets.iter().position(|t| t.last_line() >= self.top);
                i.unwrap_or(0)
            }
            _ => {
                let i = self.view.targets.iter().rposition(|t| t.first_line() < bottom);
                i.unwrap_or(n - 1)
            }
        };
        if let Some(i) = self.focus
            && self.target_visible(i)
            && (forward && next < i || !forward && next > i)
        {
            self.message = Some(if forward { "Back to the first link" } else { "On to the last link" }.into());
        }
        self.focus = Some(next);
        let first = self.view.targets[next].first_line();
        if first < self.top || first >= bottom {
            self.top = first.saturating_sub(self.body_h / 3).min(self.max_top());
        }
    }

    fn focused(&self) -> Option<usize> {
        self.focus.filter(|&i| i < self.view.targets.len() && self.target_visible(i))
    }

    /// Enter: follow the focused link or copy the focused code block. With
    /// nothing focused, scroll a line like less.
    fn activate(&mut self, n: usize) {
        let Some(i) = self.focused() else { return self.scroll_down(n) };
        match self.view.targets[i].kind.clone() {
            TargetKind::Code { code, .. } => self.copy_text(&code, "code block"),
            TargetKind::Link(id) => {
                let url = self.view.links[id as usize].clone();
                self.follow(&url);
            }
        }
    }

    fn follow(&mut self, url: &str) {
        match links::classify(url, self.page.dir()) {
            LinkAction::Anchor(name) => match self.view.find_anchor(&name) {
                Some(line) => self.jump_with_history(line),
                None => self.message = Some(format!("No heading called #{name}")),
            },
            LinkAction::File { path, fragment } => self.navigate_doc(Some(path), fragment.map_or(At::Top, At::Anchor)),
            LinkAction::Dir(path) => self.navigate_picker(path),
            LinkAction::External(url) => {
                self.message = Some(match links::open_external(&url) {
                    Ok(()) => format!("Opened {url}"),
                    Err(e) => e,
                });
            }
            LinkAction::Refused(reason) => self.message = Some(reason),
        }
    }

    fn copy_focused(&mut self) {
        let Some(i) = self.focused() else {
            self.message = Some("Press Tab to pick a link or code block, then y".into());
            return;
        };
        match self.view.targets[i].kind.clone() {
            TargetKind::Code { code, .. } => self.copy_text(&code, "code block"),
            TargetKind::Link(id) => {
                let url = self.view.links[id as usize].clone();
                self.copy_text(&url, "link");
            }
        }
    }

    fn copy_text(&mut self, text: &str, what: &str) {
        self.message = Some(match links::copy(text) {
            Ok(how) => format!("Copied the {what} to the {how}"),
            Err(e) => format!("Can't copy: {e}"),
        });
    }

    // ----- keys ---------------------------------------------------------

    fn on_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        self.message = None;
        if self.help {
            self.help = false;
            if !(ctrl && k.code == KeyCode::Char('c')) {
                return;
            }
        }
        if self.screen == Screen::Picker {
            return self.picker_key(k);
        }
        if self.search.input.is_some() {
            return self.search_key(k, ctrl);
        }
        if ctrl && k.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        if self.toc.open && self.toc.focused {
            return self.toc_key(k, ctrl);
        }
        if let Some((p, count)) = self.pending.take() {
            let n = count.unwrap_or(1);
            match (p, k.code) {
                (']', KeyCode::Char(']')) => return self.next_heading(n),
                ('[', KeyCode::Char('[')) => return self.prev_heading(n),
                ('m', KeyCode::Char(c)) if c.is_ascii_alphabetic() => return self.set_mark(c),
                ('\'', KeyCode::Char(c)) if c.is_ascii_alphabetic() || c == '\'' => return self.jump_mark(c),
                ('m' | '\'', _) => return,
                _ => {}
            }
        }
        let count = self.count.take();
        let n = count.unwrap_or(1);
        let page = self.body_h.saturating_sub(2).max(1);
        let half = (self.body_h / 2).max(1);
        match k.code {
            KeyCode::Char(c) if c.is_ascii_digit() && !ctrl && (c != '0' || count.is_some()) => {
                let digit = c as usize - '0' as usize;
                self.count = Some(count.unwrap_or(0).saturating_mul(10).saturating_add(digit).min(1_000_000));
            }
            KeyCode::Char('e' | 'n') if ctrl => self.scroll_down(n),
            KeyCode::Char('y') if ctrl => self.scroll_up(n),
            KeyCode::Char('f') if ctrl => self.scroll_down(page * n),
            KeyCode::Char('b') if ctrl => self.scroll_up(page * n),
            KeyCode::Char('d') if ctrl => self.scroll_down(half * n),
            KeyCode::Char('u') if ctrl => self.scroll_up(half * n),
            KeyCode::Char('p') if ctrl => self.open_filter(),
            KeyCode::Char('o') if ctrl => self.go_back(),
            KeyCode::Char('%') => {
                if let Some(p) = count {
                    self.goto_percent(p);
                }
            }
            KeyCode::Char('j') | KeyCode::Down => self.scroll_down(n),
            KeyCode::Enter => self.activate(n),
            KeyCode::Char('k') | KeyCode::Up => self.scroll_up(n),
            KeyCode::Char(' ' | 'f') | KeyCode::PageDown => self.scroll_down(page * n),
            KeyCode::Char('b') | KeyCode::PageUp => self.scroll_up(page * n),
            KeyCode::Char('d') => self.scroll_down(half * n),
            KeyCode::Char('u') => self.scroll_up(half * n),
            KeyCode::Char('g' | '<') | KeyCode::Home => self.top = 0,
            KeyCode::Char('G' | '>') | KeyCode::End => self.top = self.max_top(),
            KeyCode::Char(c @ (']' | '[' | 'm' | '\'')) => self.pending = Some((c, count)),
            KeyCode::Char('}') => self.next_heading(n),
            KeyCode::Char('{') => self.prev_heading(n),
            KeyCode::Char('/') => self.start_search(),
            KeyCode::Char('n') => self.next_match(n, true),
            KeyCode::Char('N') => self.next_match(n, false),
            KeyCode::Char('t') => self.toggle_toc(),
            KeyCode::Tab => self.next_target(true),
            KeyCode::BackTab => self.next_target(false),
            KeyCode::Char('y') => self.copy_focused(),
            KeyCode::Char('H') | KeyCode::Backspace => self.go_back(),
            KeyCode::Char('L') => self.go_forward(),
            KeyCode::Char('o') => {
                let root = self.picker_root.clone();
                self.navigate_picker(root);
            }
            KeyCode::Char('r') => self.toggle_raw(),
            KeyCode::Char('R') => self.reload(),
            KeyCode::Char('?') | KeyCode::F(1) => self.help = true,
            KeyCode::Esc => {
                self.focus = None;
                self.search.query.clear();
                self.search.matches.clear();
                self.search.current = None;
            }
            KeyCode::Char('q') => self.quit = true,
            _ => {}
        }
    }

    // ----- search -------------------------------------------------------

    fn start_search(&mut self) {
        self.search.saved = Some((self.search.query.clone(), self.search.current));
        self.search.saved_top = self.top;
        self.search.input = Some(String::new());
    }

    fn search_key(&mut self, k: KeyEvent, ctrl: bool) {
        let Some(input) = self.search.input.as_mut() else { return };
        match k.code {
            KeyCode::Esc => self.cancel_search(),
            KeyCode::Char('c') if ctrl => self.cancel_search(),
            KeyCode::Char('u') if ctrl => {
                input.clear();
                self.update_search();
            }
            KeyCode::Backspace if input.is_empty() => self.cancel_search(),
            KeyCode::Backspace => {
                input.pop();
                self.update_search();
            }
            KeyCode::Enter => {
                let typed = input.clone();
                self.search.input = None;
                if typed.is_empty() {
                    // An empty search repeats the previous one, like less.
                    let (query, current) = self.search.saved.take().unwrap_or_default();
                    self.search.query = query;
                    self.search.matches = search::find(&self.view, &self.search.query);
                    self.search.current = current.filter(|&c| c < self.search.matches.len());
                    if !self.search.query.is_empty() {
                        self.next_match(1, true);
                    }
                } else if self.search.matches.is_empty() {
                    self.message = Some(format!("Not found: {typed}"));
                } else if self.top != self.search.saved_top {
                    let pos = self.view.anchor_at(self.search.saved_top);
                    self.push_history(Some(Place::Doc { path: self.page.path.clone(), pos }));
                }
            }
            KeyCode::Char(c) if !ctrl => {
                input.push(c);
                self.update_search();
            }
            _ => {}
        }
    }

    fn cancel_search(&mut self) {
        self.search.input = None;
        self.top = self.search.saved_top;
        let (query, current) = self.search.saved.take().unwrap_or_default();
        self.search.matches = search::find(&self.view, &query);
        self.search.query = query;
        self.search.current = current.filter(|&c| c < self.search.matches.len());
    }

    /// Re-run the search as the query is typed and show the first match at
    /// or after where the search started.
    fn update_search(&mut self) {
        let query = self.search.input.clone().unwrap_or_default();
        self.search.matches = search::find(&self.view, &query);
        self.search.query = query;
        self.top = self.search.saved_top;
        if self.search.matches.is_empty() {
            self.search.current = None;
            return;
        }
        let i = self.search.matches.partition_point(|m| m.line() < self.search.saved_top);
        let i = if i == self.search.matches.len() { 0 } else { i };
        self.search.current = Some(i);
        self.reveal(self.search.matches[i].line());
    }

    fn next_match(&mut self, n: usize, forward: bool) {
        let total = self.search.matches.len();
        if total == 0 {
            self.message = Some(if self.search.query.is_empty() {
                "Press / to search".into()
            } else {
                format!("Not found: {}", self.search.query)
            });
            return;
        }
        let (top, bottom) = (self.top, self.top + self.body_h);
        let visible = |m: &Match| m.line() >= top && m.line() < bottom;
        // Continue from the current match if it's on screen; otherwise
        // start from the screen.
        let start: isize = match self.search.current {
            Some(c) if visible(&self.search.matches[c]) => c as isize,
            _ if forward => self.search.matches.partition_point(|m| m.line() < top) as isize - 1,
            _ => self.search.matches.partition_point(|m| m.line() < bottom) as isize,
        };
        let target = if forward { start + n as isize } else { start - n as isize };
        let wrapped = target < 0 || target >= total as isize;
        let i = target.rem_euclid(total as isize) as usize;
        self.search.current = Some(i);
        self.reveal(self.search.matches[i].line());
        if wrapped {
            self.message = Some(if forward { "Search wrapped to the top" } else { "Search wrapped to the bottom" }.into());
        }
    }

    // ----- contents pane ------------------------------------------------

    fn toc_entries(&self) -> Vec<usize> {
        let headings = &self.view.headings;
        match self.toc.filter.as_deref() {
            Some(f) if !f.trim().is_empty() => {
                (0..headings.len()).filter(|&i| fuzzy_match(f, &headings[i].text)).collect()
            }
            _ => (0..headings.len()).collect(),
        }
    }

    fn select_current(&mut self) {
        let entries = self.toc_entries();
        let current = self.current_heading();
        self.toc.selected = current.and_then(|c| entries.iter().position(|&e| e == c)).unwrap_or(0);
    }

    fn toggle_toc(&mut self) {
        if !self.toc.open {
            self.toc.open = true;
            self.toc.focused = true;
            self.select_current();
        } else if self.toc.focused {
            self.toc = Toc::default();
        } else {
            self.toc.focused = true;
            self.select_current();
        }
    }

    fn open_filter(&mut self) {
        let was_open = self.toc.open;
        self.toc.open = true;
        self.toc.focused = true;
        self.toc.filter = Some(String::new());
        self.toc.from_filter = !was_open;
        self.toc.selected = 0;
    }

    /// Leave the filter (or the pane, if Ctrl-P opened it).
    fn close_filter(&mut self) {
        if self.toc.from_filter {
            self.toc = Toc::default();
        } else {
            self.toc.filter = None;
            self.select_current();
        }
    }

    fn toc_jump(&mut self, entries: &[usize]) {
        if let Some(&h) = entries.get(self.toc.selected) {
            let line = self.view.headings[h].line;
            self.jump_with_history(line);
        }
    }

    fn toc_key(&mut self, k: KeyEvent, ctrl: bool) {
        let entries = self.toc_entries();
        let last = entries.len().saturating_sub(1);
        let sel = self.toc.selected;
        if let Some(filter) = self.toc.filter.as_mut() {
            match k.code {
                KeyCode::Esc => self.close_filter(),
                KeyCode::Backspace if filter.is_empty() => self.close_filter(),
                KeyCode::Backspace => {
                    filter.pop();
                    self.toc.selected = 0;
                }
                KeyCode::Char('u') if ctrl => {
                    filter.clear();
                    self.toc.selected = 0;
                }
                KeyCode::Enter => {
                    self.toc_jump(&entries);
                    if self.toc.from_filter {
                        self.toc = Toc::default();
                    } else {
                        self.toc.filter = None;
                        self.toc.focused = false;
                    }
                }
                KeyCode::Up => self.toc.selected = sel.saturating_sub(1),
                KeyCode::Char('p' | 'k') if ctrl => self.toc.selected = sel.saturating_sub(1),
                KeyCode::Down => self.toc.selected = (sel + 1).min(last),
                KeyCode::Char('n' | 'j') if ctrl => self.toc.selected = (sel + 1).min(last),
                KeyCode::Char(c) if !ctrl => {
                    filter.push(c);
                    self.toc.selected = 0;
                }
                _ => {}
            }
            return;
        }
        let page = self.body_h.saturating_sub(2).max(1);
        match k.code {
            KeyCode::Char('j') | KeyCode::Down => self.toc.selected = (sel + 1).min(last),
            KeyCode::Char('k') | KeyCode::Up => self.toc.selected = sel.saturating_sub(1),
            KeyCode::Char('g') | KeyCode::Home => self.toc.selected = 0,
            KeyCode::Char('G') | KeyCode::End => self.toc.selected = last,
            KeyCode::PageDown => self.toc.selected = (sel + page).min(last),
            KeyCode::PageUp => self.toc.selected = sel.saturating_sub(page),
            KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => {
                self.toc_jump(&entries);
                self.toc.focused = false;
            }
            KeyCode::Char('/') => {
                self.toc.filter = Some(String::new());
                self.toc.from_filter = false;
                self.toc.selected = 0;
            }
            KeyCode::Char('p') if ctrl => {
                self.toc.filter = Some(String::new());
                self.toc.from_filter = false;
                self.toc.selected = 0;
            }
            KeyCode::Esc | KeyCode::Tab | KeyCode::Char('h') | KeyCode::Left => self.toc.focused = false,
            KeyCode::Char('t' | 'q') => self.toc = Toc::default(),
            KeyCode::Char('r') => self.toggle_raw(),
            KeyCode::Char('?') | KeyCode::F(1) => self.help = true,
            _ => {}
        }
    }

    /// Keep the selected entry (or, when the document has focus, the current
    /// section) inside the visible part of the list.
    fn scroll_toc(&mut self, geo: &Geometry) {
        let Some(rect) = geo.toc else { return };
        let list_h = usize::from(rect.height).saturating_sub(1 + usize::from(self.toc.filter.is_some())).max(1);
        let entries = self.toc_entries();
        self.toc.selected = self.toc.selected.min(entries.len().saturating_sub(1));
        let target = if self.toc.focused {
            self.toc.selected
        } else {
            let current = self.current_heading();
            current.and_then(|c| entries.iter().position(|&e| e == c)).unwrap_or(0)
        };
        if target < self.toc.scroll {
            self.toc.scroll = target;
        } else if target >= self.toc.scroll + list_h {
            self.toc.scroll = target + 1 - list_h;
        }
        self.toc.scroll = self.toc.scroll.min(entries.len().saturating_sub(list_h));
    }

    // ----- drawing ------------------------------------------------------

    /// Draw the document screen. Returns where the cursor goes, if shown.
    fn draw(&self, f: &mut Frame, geo: &Geometry) -> Option<(u16, u16)> {
        let cursor = {
            let buf = f.buffer_mut();
            if geo.area.height < 3 || geo.area.width < 10 {
                buf.set_string(0, 0, truncate("Terminal too small", usize::from(geo.area.width)), Style::new());
                return None;
            }
            self.draw_body(buf, geo);
            self.draw_header(buf, geo.header);
            let mut cursor = self.draw_footer(buf, geo.footer);
            if let Some(rect) = geo.toc
                && let Some(c) = self.draw_toc(buf, rect, geo.toc_overlay)
            {
                cursor = Some(c);
            }
            if self.help {
                draw_help(buf, geo.area, &self.theme, HELP);
                cursor = None;
            }
            cursor
        };
        if let Some(pos) = cursor {
            f.set_cursor_position(pos);
        }
        cursor
    }

    fn draw_body(&self, buf: &mut Buffer, geo: &Geometry) {
        let height = usize::from(geo.body.height);
        for row in 0..height {
            let Some(line) = self.view.lines.get(self.top + row) else { break };
            let spans: Vec<Span> = line.segs.iter().map(|s| Span::styled(s.text.as_str(), s.style)).collect();
            buf.set_line(geo.content_x, geo.body.y + row as u16, &Line::from(spans), geo.content_w);
        }
        let paint = |buf: &mut Buffer, spans: &[(usize, usize, usize)], style: Style| {
            let right = usize::from(geo.content_x) + usize::from(geo.content_w);
            for &(line, col, w) in spans {
                if line < self.top || line >= self.top + height {
                    continue;
                }
                let y = geo.body.y + (line - self.top) as u16;
                for x in (usize::from(geo.content_x) + col..).take(w).filter(|&x| x < right) {
                    if let Some(cell) = buf.cell_mut((x as u16, y)) {
                        cell.set_style(style);
                    }
                }
            }
        };
        // Search highlights.
        let matches = &self.search.matches;
        let first = matches.partition_point(|m| m.line() + 4 < self.top);
        for (i, m) in matches.iter().enumerate().skip(first) {
            if m.line() >= self.top + height {
                break;
            }
            let style = if Some(i) == self.search.current { self.theme.search_current } else { self.theme.search_match };
            paint(buf, &m.spans, style);
        }
        // Focus.
        if let Some(i) = self.focused() {
            let target = &self.view.targets[i];
            let style = match target.kind {
                TargetKind::Link(_) => self.theme.focus,
                TargetKind::Code { .. } => self.theme.focus_border,
            };
            paint(buf, &target.spans, style);
        }
    }

    /// Visible links, in screen coordinates, to re-print as hyperlinks.
    fn screen_links(&self, geo: &Geometry) -> Vec<ScreenLink> {
        if self.link_urls.is_empty() || self.help || (geo.toc_overlay && self.toc.open) {
            return Vec::new();
        }
        let bottom = self.top + usize::from(geo.body.height);
        let right = usize::from(geo.content_x) + usize::from(geo.content_w);
        let mut out = Vec::new();
        for t in &self.view.targets {
            let TargetKind::Link(id) = t.kind else { continue };
            if t.last_line() < self.top || t.first_line() >= bottom {
                continue;
            }
            let Some(Some(url)) = self.link_urls.get(id as usize) else { continue };
            for &(line, col, w) in &t.spans {
                let x = usize::from(geo.content_x) + col;
                if line < self.top || line >= bottom || x >= right {
                    continue;
                }
                let w = w.min(right - x);
                let y = geo.body.y + (line - self.top) as u16;
                out.push((x as u16, y, w as u16, url.clone()));
            }
        }
        out
    }

    fn position_label(&self) -> String {
        let len = self.view.lines.len();
        if len <= self.body_h {
            return "All".into();
        }
        let place = if self.top == 0 {
            "Top".to_string()
        } else if self.top >= self.max_top() {
            return "End".into();
        } else {
            format!("{}%", ((self.top + self.body_h) * 100 / len).min(99))
        };
        let words = self.words_after.get(self.top).copied().unwrap_or(0);
        if words == 0 {
            return place;
        }
        let minutes = words.div_ceil(WORDS_PER_MINUTE);
        format!("{minutes} min left · {place}")
    }

    fn breadcrumb(&self) -> String {
        let headings = &self.view.headings;
        let Some(mut i) = self.current_heading() else { return String::new() };
        let mut parts = vec![headings[i].text.as_str()];
        let mut level = headings[i].level;
        while i > 0 {
            i -= 1;
            if headings[i].level < level {
                parts.push(headings[i].text.as_str());
                level = headings[i].level;
            }
        }
        parts.reverse();
        parts.join(" › ")
    }

    fn draw_header(&self, buf: &mut Buffer, rect: Rect) {
        let t = &self.theme;
        buf.set_style(rect, t.bar);
        let total = usize::from(rect.width);
        let right = format!(" {} ", self.position_label());
        let right_w = width(&right);
        if right_w + 4 < total {
            buf.set_string(rect.x + (total - right_w) as u16, rect.y, &right, t.bar);
        }
        let room = total.saturating_sub(right_w + 1);
        let mut name = format!(" {}", self.page.name);
        if self.raw {
            name.push_str(" [raw]");
        }
        let name = truncate(&name, room);
        let mut spans = vec![Span::styled(name.clone(), t.bar_accent)];
        let crumbs = self.breadcrumb();
        let left = room.saturating_sub(width(&name) + 3);
        if !crumbs.is_empty() && left > 4 {
            spans.push(Span::styled("  ·  ", t.bar));
            spans.push(Span::styled(truncate_left(&crumbs, left.saturating_sub(2)), t.bar));
        }
        buf.set_line(rect.x, rect.y, &Line::from(spans), room as u16);
    }

    /// Draw the bottom bar; returns where the cursor goes, if anywhere.
    fn draw_footer(&self, buf: &mut Buffer, rect: Rect) -> Option<(u16, u16)> {
        let t = &self.theme;
        buf.set_style(rect, t.bar);
        let total = usize::from(rect.width);
        let key = t.bar.add_modifier(Modifier::BOLD);
        let mut right = String::new();
        if !self.search.matches.is_empty() {
            right = match self.search.current {
                Some(c) => format!(" {}/{} ", c + 1, self.search.matches.len()),
                None => format!(" {} matches ", self.search.matches.len()),
            };
        } else if self.search.input.as_deref().is_some_and(|q| !q.is_empty()) {
            right = " no matches ".into();
        }
        let right_w = width(&right);
        if !right.is_empty() && right_w + 10 < total {
            buf.set_string(rect.x + (total - right_w) as u16, rect.y, &right, t.bar);
        }
        let room = total.saturating_sub(right_w + 1);
        let text = |buf: &mut Buffer, s: &str, style: Style| {
            buf.set_stringn(rect.x + 1, rect.y, s, room.saturating_sub(1), style);
        };
        if let Some(input) = &self.search.input {
            let shown = truncate_left(&format!("/{input}"), room.saturating_sub(1));
            buf.set_string(rect.x, rect.y, &shown, t.bar);
            return Some((rect.x + width(&shown) as u16, rect.y));
        }
        let hints: &[(&str, &str)] = if self.toc.open && self.toc.focused {
            if self.toc.filter.is_some() {
                &[("type", "to filter"), ("↑↓", "select"), ("Enter", "jump"), ("Esc", "cancel")]
            } else {
                &[("↑↓", "move"), ("Enter", "jump"), ("/", "filter"), ("Esc", "back to text"), ("t", "close")]
            }
        } else if let Some(msg) = &self.message {
            text(buf, msg, t.bar_accent);
            return None;
        } else if let Some((p @ ('m' | '\''), _)) = self.pending {
            let prompt = if p == 'm' {
                "Set mark: press a letter (a-z for this file, A-Z for any file)".to_string()
            } else {
                format!("Go to mark: {}   (' goes back)", self.mark_names())
            };
            text(buf, &prompt, t.bar_accent);
            return None;
        } else if let Some(n) = self.count {
            text(buf, &n.to_string(), t.bar);
            return None;
        } else if let Some(i) = self.focused() {
            self.draw_target_info(buf, rect, room, i);
            return None;
        } else {
            &[("/", "search"), ("t", "contents"), ("Tab", "links"), ("H", "back"), ("o", "files"), ("?", "help"), ("q", "quit")]
        };
        draw_hints(buf, rect, room, hints, key, t.bar);
        None
    }

    /// The footer while a link or code block has focus.
    fn draw_target_info(&self, buf: &mut Buffer, rect: Rect, room: usize, i: usize) {
        let t = &self.theme;
        let key = t.bar.add_modifier(Modifier::BOLD);
        let (label, actions): (String, &[(&str, &str)]) = match &self.view.targets[i].kind {
            TargetKind::Link(id) => {
                let url = &self.view.links[*id as usize];
                (format!("→ {url}"), &[("Enter", "open"), ("y", "copy")])
            }
            TargetKind::Code { lang, code } => {
                let lines = code.lines().count().max(1);
                let lang = if lang.is_empty() { String::new() } else { format!("{lang} · ") };
                (format!("Code block · {lang}{lines} line{}", if lines == 1 { "" } else { "s" }), &[("Enter", "copy")])
            }
        };
        let actions_w: usize = actions.iter().map(|(k, l)| width(k) + width(l) + 4).sum();
        let label = truncate(&label, room.saturating_sub(actions_w + 3).max(10));
        let mut spans = vec![Span::raw(" "), Span::styled(label, t.bar_accent), Span::raw("   ")];
        for (k, l) in actions {
            spans.push(Span::styled(*k, key));
            spans.push(Span::styled(format!(" {l}   "), t.bar));
        }
        buf.set_line(rect.x, rect.y, &Line::from(spans), room as u16);
    }

    fn draw_toc(&self, buf: &mut Buffer, rect: Rect, overlay: bool) -> Option<(u16, u16)> {
        let t = &self.theme;
        Clear.render(rect, buf);
        let w = usize::from(rect.width);
        let title_style = if self.toc.focused { t.toc_current } else { t.dim.add_modifier(Modifier::BOLD) };
        buf.set_string(rect.x + 1, rect.y, truncate("Contents", w.saturating_sub(1)), title_style);
        let mut y = rect.y + 1;
        let mut cursor = None;
        if let Some(filter) = &self.toc.filter {
            let shown = truncate_left(&format!("› {filter}"), w.saturating_sub(3));
            buf.set_string(rect.x + 1, y, &shown, Style::new());
            cursor = Some((rect.x + 1 + width(&shown) as u16, y));
            y += 1;
        }
        let headings = &self.view.headings;
        let entries = self.toc_entries();
        if headings.is_empty() || entries.is_empty() {
            let msg = if headings.is_empty() { "No headings" } else { "No matches" };
            buf.set_string(rect.x + 1, y, truncate(msg, w.saturating_sub(1)), t.dim);
        }
        let min_level = headings.iter().map(|h| h.level).min().unwrap_or(1);
        let current = self.current_heading();
        for (row, (i, &h)) in entries.iter().enumerate().skip(self.toc.scroll).enumerate() {
            let yy = y + row as u16;
            if yy >= rect.bottom() {
                break;
            }
            let heading = &headings[h];
            let indent = 2 * usize::from(heading.level - min_level).min(6);
            let is_current = Some(h) == current;
            let marker = if is_current { "▸ " } else { "  " };
            let room = w.saturating_sub(2 + indent + 2);
            let text = format!(" {}{marker}{}", " ".repeat(indent), truncate(&heading.text, room));
            let mut style = if is_current {
                t.toc_current
            } else if heading.level == min_level {
                Style::new().add_modifier(Modifier::BOLD)
            } else {
                Style::new()
            };
            if self.toc.focused && i == self.toc.selected {
                buf.set_style(Rect::new(rect.x, yy, rect.width, 1), t.toc_selected);
                style = style.patch(t.toc_selected);
            }
            buf.set_string(rect.x, yy, &text, style);
        }
        if !overlay {
            for yy in rect.y..rect.bottom() {
                buf.set_string(rect.right(), yy, "│", t.rule);
            }
        }
        cursor
    }
}

fn draw_hints(buf: &mut Buffer, rect: Rect, room: usize, hints: &[(&str, &str)], key: Style, bar: Style) {
    let mut spans = vec![Span::raw(" ")];
    let mut used = 1;
    for (k, label) in hints {
        let need = width(k) + width(label) + 4;
        if used + need > room {
            break;
        }
        spans.push(Span::styled(*k, key));
        spans.push(Span::styled(format!(" {label}   "), bar));
        used += need;
    }
    buf.set_line(rect.x, rect.y, &Line::from(spans), room as u16);
}

fn draw_help(buf: &mut Buffer, area: Rect, theme: &Theme, items: &[(&str, &str)]) {
    let key_w = items.iter().map(|(k, _)| width(k)).max().unwrap_or(0);
    let text_w = items.iter().map(|(_, d)| width(d)).max().unwrap_or(0);
    let w = (key_w + text_w + 7).min(usize::from(area.width)) as u16;
    let h = (items.len() + 2).min(usize::from(area.height)) as u16;
    let rect = Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h);
    Clear.render(rect, buf);
    Panel::bordered()
        .title(" Keys ")
        .border_style(theme.rule)
        .title_style(theme.toc_current)
        .render(rect, buf);
    for (i, (k, d)) in items.iter().enumerate() {
        let y = rect.y + 1 + i as u16;
        if y + 1 >= rect.bottom() {
            break;
        }
        let line = Line::from(vec![Span::styled(format!(" {k:<key_w$}  "), theme.toc_current), Span::raw(*d)]);
        buf.set_line(rect.x + 1, y, &line, w.saturating_sub(2));
    }
}

/// Re-print visible link text wrapped in OSC 8 sequences, so the terminal
/// makes it clickable. This is written after ratatui's frame (whose cells
/// can't carry escape sequences), using the same symbols and styles, so the
/// screen looks the same.
fn write_hyperlinks(buf: &Buffer, links: &[ScreenLink], cursor: Option<(u16, u16)>) -> io::Result<()> {
    if links.is_empty() {
        return Ok(());
    }
    let mut out = String::new();
    for (x, y, w, url) in links {
        let _ = write!(out, "\x1b[{};{}H", y + 1, x + 1);
        out.push_str(&ansi::osc8_open(url));
        let mut cx = *x;
        while cx < x + w {
            let Some(cell) = buf.cell((cx, *y)) else { break };
            let codes = ansi::sgr(cell.style());
            out.push_str("\x1b[0m");
            if !codes.is_empty() {
                let _ = write!(out, "\x1b[{codes}m");
            }
            let symbol = if cell.symbol().is_empty() { " " } else { cell.symbol() };
            out.push_str(symbol);
            cx += width(symbol).max(1) as u16;
        }
        out.push_str(ansi::OSC8_CLOSE);
        out.push_str("\x1b[0m");
    }
    if let Some((x, y)) = cursor {
        let _ = write!(out, "\x1b[{};{}H", y + 1, x + 1);
    }
    let mut stdout = io::stdout();
    stdout.write_all(out.as_bytes())?;
    stdout.flush()
}

/// Keep the end of `s`, cutting from the left with an ellipsis.
fn truncate_left(s: &str, w: usize) -> String {
    if width(s) <= w {
        return s.to_string();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut used = 1;
    for &c in chars.iter().rev() {
        let cw = width(&c.to_string());
        if used + cw > w {
            break;
        }
        out.insert(0, c);
        used += cw;
    }
    format!("…{out}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_from_the_left() {
        assert_eq!(truncate_left("Guide › Install › Linux", 10), "…l › Linux");
    }
}
