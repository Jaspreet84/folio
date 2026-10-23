//! The interactive pager: scrolling, headings, contents pane, search.

use std::io;

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block as Panel, Clear, Widget};
use ratatui::{DefaultTerminal, Frame};

use crate::doc::Document;
use crate::highlight::Highlighter;
use crate::layout::{self, HeadingRef, Rendered};
use crate::search::{self, Match};
use crate::text::{truncate, width};
use crate::theme::Theme;

const WORDS_PER_MINUTE: u32 = 230;

const HELP: &[(&str, &str)] = &[
    ("j k  ↓ ↑  Enter", "scroll one line"),
    ("space f  b", "page down / up"),
    ("d u", "half page down / up"),
    ("g G  Home End", "top / bottom"),
    ("50%", "go to a percentage"),
    ("]] [[  } {", "next / previous heading"),
    ("t  Tab", "contents pane"),
    ("Ctrl-P", "jump to a heading by name"),
    ("/  n N", "search, next / previous match"),
    ("Esc", "clear search highlights"),
    ("r", "raw Markdown on / off"),
    ("5j  3]]", "a number repeats a motion"),
    ("?", "this help"),
    ("q", "quit"),
];

pub struct Pager {
    name: String,
    source: String,
    doc: Document,
    theme: Theme,
    hl: Highlighter,
    max_width: usize,
    raw: bool,
    view: Rendered,
    /// Headings of the rendered view, kept for mapping onto the raw view.
    doc_headings: Vec<HeadingRef>,
    /// `words_after[i]` = words from line `i` to the end.
    words_after: Vec<u32>,
    top: usize,
    body_h: usize,
    toc: Toc,
    search: Search,
    count: Option<usize>,
    pending: Option<(char, Option<usize>)>,
    help: bool,
    message: Option<String>,
    quit: bool,
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

impl Pager {
    pub fn new(name: String, source: String, doc: Document, theme: Theme, hl: Highlighter, max_width: usize) -> Pager {
        Pager {
            name,
            source,
            doc,
            theme,
            hl,
            max_width: max_width.max(10),
            raw: false,
            view: Rendered::default(),
            doc_headings: Vec::new(),
            words_after: vec![0],
            top: 0,
            body_h: 1,
            toc: Toc::default(),
            search: Search::default(),
            count: None,
            pending: None,
            help: false,
            message: None,
            quit: false,
        }
    }

    pub fn run(mut self, terminal: &mut DefaultTerminal) -> io::Result<()> {
        while !self.quit {
            let size = terminal.size()?;
            let geo = self.geometry(Rect::new(0, 0, size.width, size.height));
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
            terminal.draw(|f| self.draw(f, &geo))?;
            if let Event::Key(key) = event::read()?
                && key.kind != KeyEventKind::Release {
                    self.on_key(key);
                }
        }
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

    /// Lay out again at `width`, keeping the reader's place.
    fn rebuild(&mut self, width: usize) {
        let anchor = (!self.view.lines.is_empty()).then(|| self.view.anchor_at(self.top));
        self.view = if self.raw {
            layout::layout_raw(&self.source, &self.doc_headings, width, &self.theme, &mut self.hl)
        } else {
            layout::layout(&self.doc, width, &self.theme, &mut self.hl)
        };
        self.view.width = width;
        self.top = anchor.map_or(0, |a| self.view.line_for(a));
        let mut words = vec![0u32; self.view.lines.len() + 1];
        for i in (0..self.view.lines.len()).rev() {
            words[i] = words[i + 1] + self.view.lines[i].words;
        }
        self.words_after = words;
        if !self.search.query.is_empty() {
            let had_current = self.search.current.is_some();
            self.search.matches = search::find(&self.view, &self.search.query);
            let i = self.search.matches.partition_point(|m| m.line() < self.top);
            self.search.current = (had_current && i < self.search.matches.len()).then_some(i);
        }
    }

    fn toggle_raw(&mut self) {
        if !self.raw {
            self.doc_headings = self.view.headings.clone();
        }
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
        if self.search.input.is_some() {
            self.search_key(k, ctrl);
            return;
        }
        if ctrl && k.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        if self.toc.open && self.toc.focused {
            self.toc_key(k, ctrl);
            return;
        }
        if let Some((p, count)) = self.pending.take() {
            let n = count.unwrap_or(1);
            match (p, k.code) {
                (']', KeyCode::Char(']')) => return self.next_heading(n),
                ('[', KeyCode::Char('[')) => return self.prev_heading(n),
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
            KeyCode::Char('%') => {
                if let Some(p) = count {
                    self.goto_percent(p);
                }
            }
            KeyCode::Char('j') | KeyCode::Down | KeyCode::Enter => self.scroll_down(n),
            KeyCode::Char('k') | KeyCode::Up => self.scroll_up(n),
            KeyCode::Char(' ' | 'f') | KeyCode::PageDown => self.scroll_down(page * n),
            KeyCode::Char('b') | KeyCode::PageUp => self.scroll_up(page * n),
            KeyCode::Char('d') => self.scroll_down(half * n),
            KeyCode::Char('u') => self.scroll_up(half * n),
            KeyCode::Char('g' | '<') | KeyCode::Home => self.top = 0,
            KeyCode::Char('G' | '>') | KeyCode::End => self.top = self.max_top(),
            KeyCode::Char(c @ (']' | '[')) => self.pending = Some((c, count)),
            KeyCode::Char('}') => self.next_heading(n),
            KeyCode::Char('{') => self.prev_heading(n),
            KeyCode::Char('/') => self.start_search(),
            KeyCode::Char('n') => self.next_match(n, true),
            KeyCode::Char('N') => self.next_match(n, false),
            KeyCode::Char('t') => self.toggle_toc(),
            KeyCode::Tab if self.toc.open => {
                self.toc.focused = true;
                self.select_current();
            }
            KeyCode::Char('r') => self.toggle_raw(),
            KeyCode::Char('?') | KeyCode::F(1) => self.help = true,
            KeyCode::Esc => {
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

    fn toc_key(&mut self, k: KeyEvent, ctrl: bool) {
        let entries = self.toc_entries();
        let last = entries.len().saturating_sub(1);
        let sel = self.toc.selected;
        let jump = |pager: &mut Pager| {
            if let Some(&h) = entries.get(pager.toc.selected) {
                let line = pager.view.headings[h].line;
                pager.jump(line);
            }
        };
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
                    jump(self);
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
                jump(self);
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

    fn draw(&self, f: &mut Frame, geo: &Geometry) {
        let cursor = {
            let buf = f.buffer_mut();
            if geo.area.height < 3 || geo.area.width < 10 {
                buf.set_string(0, 0, truncate("Terminal too small", usize::from(geo.area.width)), Style::new());
                return;
            }
            self.draw_body(buf, geo);
            self.draw_header(buf, geo.header);
            let mut cursor = self.draw_footer(buf, geo.footer);
            if let Some(rect) = geo.toc
                && let Some(c) = self.draw_toc(buf, rect, geo.toc_overlay) {
                    cursor = Some(c);
                }
            if self.help {
                self.draw_help(buf, geo.area);
            }
            cursor
        };
        if let Some(pos) = cursor {
            f.set_cursor_position(pos);
        }
    }

    fn draw_body(&self, buf: &mut Buffer, geo: &Geometry) {
        let height = usize::from(geo.body.height);
        for row in 0..height {
            let Some(line) = self.view.lines.get(self.top + row) else { break };
            let spans: Vec<Span> = line.segs.iter().map(|s| Span::styled(s.text.as_str(), s.style)).collect();
            buf.set_line(geo.content_x, geo.body.y + row as u16, &Line::from(spans), geo.content_w);
        }
        // Search highlights.
        let matches = &self.search.matches;
        let first = matches.partition_point(|m| m.line() + 4 < self.top);
        let right = usize::from(geo.content_x) + usize::from(geo.content_w);
        for (i, m) in matches.iter().enumerate().skip(first) {
            if m.line() >= self.top + height {
                break;
            }
            let style = if Some(i) == self.search.current { self.theme.search_current } else { self.theme.search_match };
            for &(line, col, w) in &m.spans {
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
        }
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
        let mut name = format!(" {}", self.name);
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
        let room = total.saturating_sub(right_w + 1) as u16;
        if let Some(input) = &self.search.input {
            let prompt = format!("/{input}");
            let shown = truncate_left(&prompt, usize::from(room).saturating_sub(1));
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
            buf.set_stringn(rect.x + 1, rect.y, msg, usize::from(room), t.bar_accent);
            return None;
        } else if let Some(n) = self.count {
            buf.set_string(rect.x + 1, rect.y, n.to_string(), t.bar);
            return None;
        } else {
            &[("/", "search"), ("t", "contents"), ("]]", "next heading"), ("r", "raw"), ("?", "help"), ("q", "quit")]
        };
        let mut spans = vec![Span::raw(" ")];
        let mut used = 1;
        for (k, label) in hints {
            let w = width(k) + width(label) + 3;
            if used + w > usize::from(room) {
                break;
            }
            spans.push(Span::styled(*k, key));
            spans.push(Span::styled(format!(" {label}   "), t.bar));
            used += w + 2;
        }
        buf.set_line(rect.x, rect.y, &Line::from(spans), room);
        None
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

    fn draw_help(&self, buf: &mut Buffer, area: Rect) {
        let key_w = HELP.iter().map(|(k, _)| width(k)).max().unwrap_or(0);
        let text_w = HELP.iter().map(|(_, d)| width(d)).max().unwrap_or(0);
        let w = (key_w + text_w + 7).min(usize::from(area.width)) as u16;
        let h = (HELP.len() + 2).min(usize::from(area.height)) as u16;
        let rect = Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h);
        Clear.render(rect, buf);
        Panel::bordered()
            .title(" Keys ")
            .border_style(self.theme.rule)
            .title_style(self.theme.toc_current)
            .render(rect, buf);
        let key = self.theme.toc_current;
        for (i, (k, d)) in HELP.iter().enumerate() {
            let y = rect.y + 1 + i as u16;
            if y + 1 >= rect.bottom() {
                break;
            }
            let line = Line::from(vec![
                Span::styled(format!(" {k:<key_w$}  "), key),
                Span::raw(*d),
            ]);
            buf.set_line(rect.x + 1, y, &line, w.saturating_sub(2));
        }
    }
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

/// Case-insensitive subsequence match, ignoring spaces in the query.
fn fuzzy_match(query: &str, text: &str) -> bool {
    let mut hay = text.chars().flat_map(char::to_lowercase);
    query
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .all(|q| hay.any(|h| h == q))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_matches_subsequences() {
        assert!(fuzzy_match("inst lin", "Install on Linux"));
        assert!(fuzzy_match("API", "api reference"));
        assert!(!fuzzy_match("xyz", "Install"));
    }

    #[test]
    fn truncates_from_the_left() {
        assert_eq!(truncate_left("Guide › Install › Linux", 10), "…l › Linux");
    }
}
