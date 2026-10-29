//! Flowcharts: the graph read from the source (plan section 4.5), and the
//! steps that parse, prepare and draw it.
//!
//! Every list is in declaration order, and ties are always broken by that
//! order. `ids` is only for lookups and is never iterated.

pub mod draw;
pub mod parse;
pub mod prep;

use std::collections::{BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

use super::Step;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Dir {
    #[default]
    TD,
    LR,
    BT,
    RL,
}

impl Dir {
    /// The direction it is drawn in until mirroring is built (M3): BT as TD
    /// and RL as LR.
    pub fn base(self) -> Dir {
        match self {
            Dir::TD | Dir::BT => Dir::TD,
            Dir::LR | Dir::RL => Dir::LR,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Shape {
    Rect,
    Round,
    Stadium,
    Subroutine,
    Cylinder,
    Circle,
    DCircle,
    Decision,
    Hexagon,
    /// Parallelograms and trapezoids; the side glyphs are in [`Node::slant`].
    Slant,
    /// `>text]`.
    Asym,
    /// No border (`@{ shape: text }`).
    Bare,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    /// Decoded label; lines are separated by `\n`.
    pub label: String,
    pub shape: Shape,
    /// Index into [`FlowGraph::clusters`] of the subgraph that took the node:
    /// the first one closed whose body names it, so an inner one wins.
    pub cluster: Option<usize>,
    /// Left and right side glyphs for [`Shape::Slant`] and [`Shape::Asym`].
    pub slant: Option<(char, char)>,
    /// The node was given a shape or label, not just named.
    pub explicit: bool,
    /// A subgraph id used as an edge end; its edges were moved to a member
    /// and it is not drawn.
    pub dead: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Stroke {
    Solid,
    Dotted,
    Thick,
    Invisible,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Head {
    None,
    Arrow,
    Circle,
    Cross,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    /// Node indexes.
    pub from: usize,
    pub to: usize,
    pub stroke: Stroke,
    /// The head at `from` (`<--`) and at `to` (`-->`).
    pub head_from: Head,
    pub head_to: Head,
    pub label: Option<String>,
    /// Minimum rank distance, from the link's length (`--->` is 2).
    pub min_len: usize,
    /// The subgraph (index into [`FlowGraph::clusters`]) that this end
    /// named before it was attached to one of the subgraph's members.
    /// Outlines show the subgraph instead of that member.
    #[serde(default)]
    pub from_cluster: Option<usize>,
    #[serde(default)]
    pub to_cluster: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cluster {
    pub id: String,
    pub title: String,
    pub parent: Option<usize>,
    /// Node indexes, in the order the subgraph's body first names them.
    pub members: Vec<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowGraph {
    pub dir: Dir,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub clusters: Vec<Cluster>,
    /// Features drawn in a simpler form, for notes ("subgraph", "shape", …).
    pub simplified: BTreeSet<String>,
    /// Node id → index, for lookups only.
    #[serde(skip)]
    pub ids: HashMap<String, usize>,
}

/// Spacing and label width for one ladder step (plan section 7.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Profile {
    /// Labels wrap at this many columns.
    pub label_w: usize,
    /// Columns between nodes in one rank.
    pub node_gap: usize,
    /// Columns of each channel between ranks in a left-to-right drawing.
    pub lr_chan: usize,
    /// [`Dir::TD`] or [`Dir::LR`].
    pub dir: Dir,
}

impl Profile {
    /// The profile for `step` of a diagram whose base direction is `base`.
    pub fn new(step: Step, base: Dir) -> Profile {
        let (label_w, node_gap, lr_chan, dir) = match step {
            Step::Normal => (28, 3, 4, base),
            Step::Compact => (28, 1, 2, base),
            Step::LrLabels16 => (16, 1, 2, Dir::LR),
            Step::LrLabels8 => (8, 1, 2, Dir::LR),
            Step::Turned => (28, 3, 4, Dir::TD),
            Step::TurnedCompact => (28, 1, 2, Dir::TD),
            Step::Labels16 => (16, 1, 2, Dir::TD),
            Step::Labels8 => (8, 1, 2, Dir::TD),
        };
        Profile { label_w, node_gap, lr_chan, dir }
    }
}
