//! The canvas flowcharts are painted on (plan section 6.3): line cells with
//! owners, so the checks can tell which edge drew what, plus text and
//! glyphs. Ported from `Canvas` in the prototype.
//!
//! Text wins over glyphs, and glyphs win over lines. Cells are kept in the
//! order they were first touched; the hash map is only for lookups.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

use unicode_segmentation::UnicodeSegmentation;

use super::Role;
use super::flow::Stroke;
use super::pre::width;

/// Direction bits of a line cell.
pub const N: u8 = 1;
pub const S: u8 = 2;
pub const W: u8 = 4;
pub const E: u8 = 8;

/// Box-drawing glyphs by weight per direction. The index is
/// `n + 3s + 9w + 27e`, each 0 (none), 1 (light) or 2 (heavy). Built from
/// the Unicode names of U+2500–U+257F, as the prototype's `BOX` is.
const BOX: [char; 81] = [
    ' ', '╵', '╹', '╷', '│', '╿', '╻', '╽', '┃', '╴', '┘', '┚', '┐', '┤', '┦', '┒', '┧', '┨', '╸', '┙', '┛', '┑', '┥',
    '┩', '┓', '┪', '┫', '╶', '└', '┖', '┌', '├', '┞', '┎', '┟', '┠', '─', '┴', '┸', '┬', '┼', '╀', '┰', '╁', '╂', '╾',
    '┵', '┹', '┭', '┽', '╃', '┱', '╅', '╉', '╺', '┕', '┗', '┍', '┝', '┡', '┏', '┢', '┣', '╼', '┶', '┺', '┮', '┾', '╄',
    '┲', '╆', '╊', '━', '┷', '┻', '┯', '┿', '╇', '┳', '╈', '╋',
];

/// What a run of line cells belongs to. Two owners count as one bus when
/// they share a bus or an edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bus {
    /// Segment `index` of the channel below rank `rank`.
    Segment { rank: usize, index: usize },
    /// A dummy element's run through a thick rank.
    Dummy(usize),
    /// A node's self-loop, named by its first self-loop edge.
    SelfLoop(usize),
    /// The lanes arriving at a node.
    Lane(usize),
}

/// The bus and edge a line cell was drawn for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Owner {
    pub bus: Bus,
    pub edge: Option<usize>,
}

#[derive(Clone, Debug)]
struct Own {
    owner: Owner,
    bits: u8,
    /// The owner's last line here was dotted.
    dotted: bool,
}

/// Text in one cell.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Text {
    /// A space: a node's interior or padding.
    Blank,
    /// The second cell of a wide character.
    Cont,
    Char(char),
    /// A grapheme of more than one character.
    Cluster(Box<str>),
}

#[derive(Clone, Debug, Default)]
struct Cell {
    /// A line was drawn here, even one with no bits.
    line: bool,
    light: u8,
    heavy: u8,
    solid: u32,
    dotted: u32,
    /// A line with no owner (a box border) was drawn here.
    border: bool,
    glyph: Option<(char, Role)>,
    text: Option<(Text, Role)>,
    /// Text placed over a line on purpose (lane labels): the owner whose
    /// line the text may cover. Any other line under it is still an error.
    intended: Option<Owner>,
    owners: Vec<Own>,
}

/// One owner group of a cell: owners merged because they share a bus or
/// an edge.
#[derive(Clone, Copy, Debug)]
pub struct Group {
    pub bits: u8,
    /// Every owner in the group drew its last line here dotted.
    pub dotted: bool,
}

/// A small fast hasher for cell coordinates (FxHash). Never iterated.
#[derive(Default)]
struct CellHasher(u64);

impl Hasher for CellHasher {
    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.add(u64::from(b));
        }
    }

    fn write_i64(&mut self, i: i64) {
        self.add(i as u64);
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

impl CellHasher {
    fn add(&mut self, w: u64) {
        self.0 = (self.0.rotate_left(5) ^ w).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
}

pub struct Canvas {
    index: HashMap<(i64, i64), usize, BuildHasherDefault<CellHasher>>,
    cells: Vec<Cell>,
    at: Vec<(i64, i64)>,
    /// The owner recorded for lines drawn now; `None` for box borders.
    pub owner: Option<Owner>,
    /// At a crossing, the group whose bits all lie within these is in
    /// front (the lines along the flow).
    pub front_bits: u8,
    /// Router constraints that could not all be met.
    pub hard_cycles: usize,
    /// Things that could not be drawn.
    pub dropped: usize,
    /// Cells that would have been drawn at a negative coordinate.
    pub off_canvas: usize,
    /// Steps taken drawing: one per cell written, plus the owners looked
    /// through. The caller charges them to its work limit.
    pub steps: u64,
}

impl Canvas {
    pub fn new(front_bits: u8) -> Canvas {
        Canvas {
            index: HashMap::default(),
            cells: Vec::new(),
            at: Vec::new(),
            owner: None,
            front_bits,
            hard_cycles: 0,
            dropped: 0,
            off_canvas: 0,
            steps: 0,
        }
    }

    fn cell(&mut self, y: i64, x: i64) -> Option<&mut Cell> {
        self.steps += 1;
        if y < 0 || x < 0 {
            self.off_canvas += 1;
            return None;
        }
        let i = *self.index.entry((y, x)).or_insert_with(|| {
            self.cells.push(Cell::default());
            self.at.push((y, x));
            self.cells.len() - 1
        });
        Some(&mut self.cells[i])
    }

    /// Draw line bits in one cell for the current owner.
    pub fn line(&mut self, y: i64, x: i64, bits: u8, stroke: Stroke) {
        let owner = self.owner;
        let Some(c) = self.cell(y, x) else { return };
        c.line = true;
        if stroke == Stroke::Thick {
            c.heavy |= bits;
        } else {
            c.light |= bits;
        }
        let dotted = stroke == Stroke::Dotted;
        if dotted {
            c.dotted += 1;
        } else {
            c.solid += 1;
        }
        let looked = c.owners.len() as u64;
        match owner {
            None => c.border = true,
            Some(owner) => match c.owners.iter_mut().find(|o| o.owner == owner) {
                Some(o) => {
                    o.bits |= bits;
                    o.dotted = dotted;
                }
                None => c.owners.push(Own { owner, bits, dotted }),
            },
        }
        self.steps += looked;
    }

    pub fn glyph(&mut self, y: i64, x: i64, ch: char, role: Role) {
        if let Some(c) = self.cell(y, x) {
            c.glyph = Some((ch, role));
        }
    }

    /// Put text at (y, x), one grapheme per cell and an empty cell after
    /// each wide one. A grapheme of no width joins the cell before it (or
    /// is left out at the start), so the cells match the columns a terminal
    /// prints. With `over_line`, the text may cover the current owner's
    /// line, and no other. Returns the column after the text.
    pub fn put_text(&mut self, y: i64, mut x: i64, s: &str, role: Role, over_line: bool) -> i64 {
        let intended = if over_line { self.owner } else { None };
        // The cell of the last grapheme written.
        let mut last: Option<i64> = None;
        for g in s.graphemes(true) {
            let gw = width(g) as i64;
            if gw == 0 {
                if let Some(lx) = last
                    && let Some(c) = self.cell(y, lx)
                    && let Some((t, _)) = &mut c.text
                {
                    let mut joined = match t {
                        Text::Blank => " ".to_string(),
                        Text::Cont => String::new(),
                        Text::Char(ch) => ch.to_string(),
                        Text::Cluster(s) => s.to_string(),
                    };
                    joined.push_str(g);
                    *t = Text::Cluster(joined.into());
                }
                continue;
            }
            let mut chars = g.chars();
            let text = match (chars.next(), chars.next()) {
                (Some(' '), None) => Text::Blank,
                (Some(ch), None) => Text::Char(ch),
                _ => Text::Cluster(g.into()),
            };
            if let Some(c) = self.cell(y, x) {
                c.text = Some((text, role));
                c.intended = intended;
            }
            for k in 1..gw {
                if let Some(c) = self.cell(y, x + k) {
                    c.text = Some((Text::Cont, role));
                    c.intended = intended;
                }
            }
            last = Some(x);
            x += gw;
        }
        x
    }

    /// Whether cell (y, x) holds nothing but `owner`'s line: no text, no
    /// glyph, no box border and no other owner's line.
    pub fn only_line_of(&self, y: i64, x: i64, owner: Owner) -> bool {
        let Some(&i) = self.index.get(&(y, x)) else { return true };
        let c = &self.cells[i];
        c.text.is_none() && c.glyph.is_none() && !c.border && c.owners.iter().all(|o| o.owner == owner)
    }

    /// Every touched cell's coordinates, in the order first touched.
    pub fn positions(&self) -> &[(i64, i64)] {
        &self.at
    }

    fn front(&self, bits: u8) -> u8 {
        u8::from(bits & !self.front_bits == 0)
    }

    /// The owners of a cell, merged when they share a bus or an edge.
    ///
    /// The prototype merges owner by owner and moves each touched group to
    /// the end, so its groups end up ordered by their last owner. Joining
    /// each owner to the first earlier one with the same bus and the first
    /// with the same edge gives the same groups in the same order.
    fn groups_of(c: &Cell) -> Vec<Group> {
        let os = &c.owners;
        if let [o] = os.as_slice() {
            return vec![Group { bits: o.bits, dotted: o.dotted }];
        }
        let mut root: Vec<usize> = (0..os.len()).collect();
        fn find(root: &mut [usize], mut i: usize) -> usize {
            while root[i] != i {
                root[i] = root[root[i]];
                i = root[i];
            }
            i
        }
        for (i, o) in os.iter().enumerate() {
            let same_bus = os[..i].iter().position(|p| p.owner.bus == o.owner.bus);
            let same_edge = o.owner.edge.and_then(|k| os[..i].iter().position(|p| p.owner.edge == Some(k)));
            for j in [same_bus, same_edge].into_iter().flatten() {
                let (a, b) = (find(&mut root, i), find(&mut root, j));
                root[a] = b;
            }
        }
        // (last owner, bits, dotted) per group.
        let mut gs: Vec<(usize, usize, Group)> = Vec::new();
        for (i, o) in os.iter().enumerate() {
            let r = find(&mut root, i);
            match gs.iter_mut().find(|g| g.0 == r) {
                Some(g) => {
                    g.1 = i;
                    g.2.bits |= o.bits;
                    g.2.dotted &= o.dotted;
                }
                None => gs.push((r, i, Group { bits: o.bits, dotted: o.dotted })),
            }
        }
        gs.sort_by_key(|g| g.1);
        gs.into_iter().map(|g| g.2).collect()
    }

    /// The crossing rule for one cell: the bits left visible (those of the
    /// groups behind are hidden, except where the front group has them too),
    /// and, at a crossing, whether the line in front is dotted.
    fn crossing(&self, c: &Cell) -> (u8, Option<bool>) {
        if c.owners.len() < 2 {
            return (0xF, None);
        }
        let gs = Self::groups_of(c);
        if gs.len() < 2 {
            return (0xF, None);
        }
        let top = gs.iter().map(|g| self.front(g.bits)).max().unwrap_or(0);
        let (mut front, mut back) = (0, 0);
        // The first group in front decides whether the crossing is dotted.
        let mut dotted = None;
        for g in &gs {
            if self.front(g.bits) == top {
                front |= g.bits;
                dotted.get_or_insert(g.dotted);
            } else {
                back |= g.bits;
            }
        }
        (0xF & !(back & !front), dotted)
    }

    /// The visible line bits of a cell after the crossing rule.
    fn vis_of(&self, c: &Cell) -> u8 {
        (c.light | c.heavy) & self.crossing(c).0
    }

    /// The character a line cell shows.
    fn line_char(&self, c: &Cell) -> Option<char> {
        let (m, front_dotted) = self.crossing(c);
        let (mut lb, mut hb) = (c.light & m, c.heavy & m);
        let bits = lb | hb;
        if bits == 0 {
            return None;
        }
        let dashed = match front_dotted {
            Some(d) => d,
            None => c.dotted > 0 && c.solid == 0,
        };
        if dashed {
            if matches!(bits, 3 | N | S) {
                return Some(if hb != 0 { '┇' } else { '┆' });
            }
            if matches!(bits, 12 | W | E) {
                return Some(if hb != 0 { '┅' } else { '┄' });
            }
        }
        if bits == N || bits == S {
            lb = if lb != 0 { N | S } else { 0 };
            hb = if hb != 0 { N | S } else { 0 };
        }
        if bits == W || bits == E {
            lb = if lb != 0 { W | E } else { 0 };
            hb = if hb != 0 { W | E } else { 0 };
        }
        let weight = |b: u8| -> usize {
            if hb & b != 0 {
                2
            } else if lb & b != 0 {
                1
            } else {
                0
            }
        };
        Some(BOX[weight(N) + 3 * weight(S) + 9 * weight(W) + 27 * weight(E)])
    }

    /// The rows as runs of (role, text), with no trailing spaces.
    pub fn render(&self) -> Vec<Vec<(Role, String)>> {
        if self.at.is_empty() {
            return Vec::new();
        }
        let h = self.at.iter().map(|&(y, _)| y).max().unwrap_or(0) + 1;
        let mut order: Vec<usize> = (0..self.at.len()).collect();
        order.sort_unstable_by_key(|&i| self.at[i]);
        let mut rows: Vec<Vec<(Role, String)>> = (0..h).map(|_| Vec::new()).collect();
        let mut k = 0;
        for (y, row) in rows.iter_mut().enumerate() {
            let y = y as i64;
            let mut runs: Vec<(Role, String)> = Vec::new();
            let mut lead = String::new();
            let mut next_x = 0;
            while k < order.len() && self.at[order[k]].0 == y {
                let i = order[k];
                k += 1;
                let x = self.at[i].1;
                let gap = (x - next_x).max(0) as usize;
                if gap > 0 {
                    let s = runs.last_mut().map_or(&mut lead, |r| &mut r.1);
                    s.extend(std::iter::repeat_n(' ', gap));
                }
                next_x = x + 1;
                let c = &self.cells[i];
                let mut buf = [0u8; 4];
                let (role, s): (Option<Role>, &str) = if let Some((t, role)) = &c.text {
                    match t {
                        Text::Blank => (Some(*role), " "),
                        Text::Cont => (Some(*role), ""),
                        Text::Char(ch) => (Some(*role), ch.encode_utf8(&mut buf)),
                        Text::Cluster(s) => (Some(*role), s),
                    }
                } else if let Some((ch, role)) = c.glyph {
                    (Some(role), ch.encode_utf8(&mut buf))
                } else {
                    match self.line_char(c) {
                        Some(ch) => (Some(if c.border { Role::Border } else { Role::Edge }), ch.encode_utf8(&mut buf)),
                        None => (None, " "),
                    }
                };
                push_run(&mut runs, &mut lead, role, s);
            }
            while let Some(last) = runs.last_mut() {
                let t = last.1.trim_end().len();
                if t == 0 {
                    runs.pop();
                } else {
                    last.1.truncate(t);
                    break;
                }
            }
            *row = runs;
        }
        rows
    }

    /// Cells drawn by more than one owner: the number of owners, and their
    /// owner groups.
    pub fn shared_cells(&self) -> impl Iterator<Item = (usize, Vec<Group>)> + '_ {
        self.cells.iter().filter(|c| c.owners.len() >= 2).map(|c| (c.owners.len(), Self::groups_of(c)))
    }

    /// The number of owners of cell `i`.
    pub fn owner_count(&self, i: usize) -> usize {
        self.cells[i].owners.len()
    }

    /// The number of touched cells. Cells are numbered in the order first
    /// touched.
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// The number of the cell at (y, x), if it was touched.
    pub fn index(&self, y: i64, x: i64) -> Option<usize> {
        self.index.get(&(y, x)).copied()
    }

    /// The visible line bits of cell `i`, or `None` if no owner drew there.
    pub fn owned_vis(&self, i: usize) -> Option<u8> {
        let c = &self.cells[i];
        (!c.owners.is_empty()).then(|| self.vis_of(c))
    }

    /// The edges of cell `i`'s owners that have any of the bits `vis`.
    pub fn edges_with(&self, i: usize, vis: u8) -> impl Iterator<Item = usize> + '_ {
        self.cells[i].owners.iter().filter(move |o| o.bits & vis != 0).filter_map(|o| o.owner.edge)
    }

    /// Cells where text (a blank or the second half of a wide character
    /// too) hides a line it wasn't placed over on purpose: a box border, or
    /// a line of any owner but the one the text was meant to cover.
    pub fn text_over_line(&self) -> usize {
        self.cells
            .iter()
            .filter(|c| {
                c.line
                    && c.text.is_some()
                    && !c.intended.is_some_and(|o| !c.border && c.owners.iter().all(|x| x.owner == o))
            })
            .count()
    }
}

/// Append text to a row's runs; `None` (a gap) joins the run before it.
fn push_run(runs: &mut Vec<(Role, String)>, lead: &mut String, role: Option<Role>, s: &str) {
    if s.is_empty() && role.is_none() {
        return;
    }
    match role {
        None => match runs.last_mut() {
            Some(last) => last.1.push_str(s),
            None => lead.push_str(s),
        },
        Some(r) => match runs.last_mut() {
            Some(last) if last.0 == r => last.1.push_str(s),
            _ => {
                let mut t = std::mem::take(lead);
                t.push_str(s);
                runs.push((r, t));
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(cv: &Canvas) -> Vec<String> {
        cv.render().iter().map(|r| r.iter().map(|(_, s)| s.as_str()).collect()).collect()
    }

    #[test]
    fn lines_join_into_box_glyphs() {
        let mut cv = Canvas::new(N | S);
        cv.line(0, 0, S | E, Stroke::Solid);
        cv.line(0, 1, W | E, Stroke::Solid);
        cv.line(0, 2, W | S, Stroke::Thick);
        cv.line(1, 0, N, Stroke::Dotted);
        assert_eq!(text(&cv), ["┌─┓", "┆"]);
    }

    #[test]
    fn crossing_hides_the_track() {
        let mut cv = Canvas::new(N | S);
        cv.owner = Some(Owner { bus: Bus::Lane(0), edge: Some(0) });
        cv.line(0, 1, N | S, Stroke::Solid);
        cv.owner = Some(Owner { bus: Bus::Lane(1), edge: Some(1) });
        cv.line(0, 0, W | E, Stroke::Solid);
        cv.line(0, 1, W | E, Stroke::Solid);
        cv.line(0, 2, W | E, Stroke::Solid);
        assert_eq!(text(&cv), ["─│─"]);
    }

    #[test]
    fn text_wins_and_wide_text_takes_two_cells() {
        let mut cv = Canvas::new(N | S);
        cv.owner = Some(Owner { bus: Bus::Lane(0), edge: Some(0) });
        cv.line(0, 0, W | E, Stroke::Solid);
        cv.glyph(0, 1, '▾', Role::Arrow);
        assert_eq!(cv.put_text(0, 2, "語a ", Role::NodeText, false), 6);
        cv.line(0, 0, W | E, Stroke::Solid);
        let rows = cv.render();
        assert_eq!(rows, [vec![(Role::Edge, "─".into()), (Role::Arrow, "▾".into()), (Role::NodeText, "語a".into())]]);
        assert_eq!(cv.text_over_line(), 0);
        cv.put_text(0, 0, "x", Role::EdgeLabel, false);
        assert_eq!(cv.text_over_line(), 1);
    }

    #[test]
    fn text_meant_for_a_line_covers_only_that_line() {
        let lane = |n| Some(Owner { bus: Bus::Lane(0), edge: Some(n) });
        let mut cv = Canvas::new(N | S);
        cv.owner = lane(0);
        for x in 0..5 {
            cv.line(0, x, W | E, Stroke::Solid);
        }
        cv.put_text(0, 1, " a ", Role::EdgeLabel, true);
        assert_eq!(cv.text_over_line(), 0);
        // Another edge's line under the text, even under a blank, and a
        // box border under the text, are errors.
        cv.owner = lane(1);
        cv.line(0, 3, N | S, Stroke::Solid);
        assert_eq!(cv.text_over_line(), 1);
        cv.owner = None;
        cv.line(0, 2, N | S, Stroke::Solid);
        assert_eq!(cv.text_over_line(), 2);
        // A wide character's second cell hides a line too.
        let mut cv = Canvas::new(N | S);
        cv.line(0, 1, N | S, Stroke::Solid);
        cv.put_text(0, 0, "語", Role::NodeText, false);
        assert_eq!(cv.text_over_line(), 1);
    }

    #[test]
    fn a_zero_width_character_joins_the_cell_before_it() {
        let mut cv = Canvas::new(N | S);
        assert_eq!(cv.put_text(0, 0, "\u{200b}a\u{200b}b\u{ad}c", Role::NodeText, false), 3);
        cv.line(0, 3, W | E, Stroke::Solid);
        assert_eq!(text(&cv), ["a\u{200b}b\u{ad}c─"]);
        assert_eq!(cv.len(), 4);
    }
}
