//! Sequence diagrams: the parsed form (plan section 4.5). They are shown as
//! transcripts until M4 draws them.
//!
//! Every list is in declaration order. `ids` is only for lookups and is
//! never iterated.

pub mod parse;

use std::collections::{BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use super::flow::Stroke;
use super::{MAX_EVENTS, MAX_PARTICIPANTS};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PartKind {
    Participant,
    Actor,
}

impl PartKind {
    pub fn name(self) -> &'static str {
        match self {
            PartKind::Participant => "participant",
            PartKind::Actor => "actor",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Participant {
    pub id: String,
    /// The `as` text, decoded, or else the id.
    pub label: String,
    pub kind: PartKind,
    /// Added by `create`: the number of events before that statement, so
    /// the transcript can show it in place.
    #[serde(default)]
    pub created: Option<usize>,
}

/// The head at the end of a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SeqHead {
    /// `->` and `-->`.
    None,
    /// `->>` and `-->>`.
    Arrow,
    /// `-x` and `--x`.
    Cross,
    /// `-)` and `--)`.
    Async,
}

/// A block that groups messages: `loop`, `alt`, … up to its `end`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Block {
    Loop,
    Alt,
    Opt,
    Par,
    /// `par_over`: a `par` whose parts overlap.
    ParOver,
    Critical,
    Break,
    Rect,
}

impl Block {
    pub fn name(self) -> &'static str {
        match self {
            Block::Loop => "loop",
            Block::Alt => "alt",
            Block::Opt => "opt",
            Block::Par => "par",
            Block::ParOver => "par_over",
            Block::Critical => "critical",
            Block::Break => "break",
            Block::Rect => "rect",
        }
    }
}

/// An `autonumber` statement. Mermaid reads these in order: each one
/// changes the numbering from where it stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Autonumber {
    /// The next message's number. `None` (no number, or 0) keeps counting.
    pub start: Option<u64>,
    /// The step. `None` (no step, or 0) keeps the last one.
    pub step: Option<u64>,
    /// Whether numbers are shown from here: `false` for `autonumber off`.
    pub visible: bool,
}

/// A `box` that groups participants.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeqBox {
    /// The text after `box`, decoded, without the colour in front. Empty
    /// when there is none.
    pub title: String,
    /// The participants declared inside the box, in order.
    pub members: Vec<usize>,
}

/// A line that divides a block into parts: `else`, `and` or `option`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Branch {
    Else,
    And,
    Option,
}

impl Branch {
    pub fn name(self) -> &'static str {
        match self {
            Branch::Else => "else",
            Branch::And => "and",
            Branch::Option => "option",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NotePlace {
    LeftOf,
    RightOf,
    Over,
}

impl NotePlace {
    pub fn name(self) -> &'static str {
        match self {
            NotePlace::LeftOf => "left of",
            NotePlace::RightOf => "right of",
            NotePlace::Over => "over",
        }
    }
}

/// One statement of the diagram, in order. Participant references are
/// indexes into [`SeqDiagram::participants`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Event {
    Message {
        from: usize,
        to: usize,
        /// [`Stroke::Solid`] or [`Stroke::Dotted`].
        stroke: Stroke,
        head: SeqHead,
        /// Heads at both ends (`<<->>`, `<<-->>`).
        both: bool,
        label: String,
        /// `+` activates the receiver (`Some(true)`); `-` deactivates the
        /// sender (`Some(false)`).
        act: Option<bool>,
    },
    Note {
        at: NotePlace,
        who: Vec<usize>,
        label: String,
    },
    Open {
        kind: Block,
        label: String,
    },
    Divider {
        kind: Branch,
        label: String,
    },
    Close,
    Activate(usize),
    Deactivate(usize),
    Destroy(usize),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeqDiagram {
    pub participants: Vec<Participant>,
    pub events: Vec<Event>,
    /// The last numbered `autonumber` statement's start and step (1 when
    /// left out), as the prototype records it. Only the parse comparison
    /// reads this; the numbers come from [`SeqDiagram::autonumbers`].
    pub autonumber: Option<(u32, u32)>,
    /// Every `autonumber` statement, in order, with the number of events
    /// before it.
    #[serde(default)]
    pub autonumbers: Vec<(usize, Autonumber)>,
    /// The `box` groups, in order.
    #[serde(default)]
    pub boxes: Vec<SeqBox>,
    /// The text of the last `title` statement, on one line. Mermaid shows
    /// it above the diagram.
    #[serde(default)]
    pub title: Option<String>,
    /// Features drawn in a simpler form, for notes ("box", "create").
    pub simplified: BTreeSet<String>,
    /// Participant id → index, for lookups only.
    #[serde(skip)]
    pub ids: HashMap<String, usize>,
}

impl SeqDiagram {
    /// Over the size caps, so it would get the transcript even once
    /// sequence diagrams are drawn.
    pub fn too_large(&self) -> bool {
        self.participants.len() > MAX_PARTICIPANTS || self.events.len() > MAX_EVENTS
    }
}
