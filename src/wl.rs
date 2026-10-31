//! Layer 3 of the shape signature: how the pieces of a view are joined,
//! counted as local patterns.
//!
//! A view is read as a graph. Its nodes are the straight and circular pieces
//! it draws -- every line, every arc, every straight and arc segment of a
//! polyline, every circle -- and two pieces are joined by an edge when an end
//! of one lies within [`ENDPOINT_GAP_UM`] of an end of the other. A circle has
//! no end and stands alone. Ellipses and splines are not nodes (the outline
//! moments of layer 2 leave them out too).
//!
//! Each node starts with a label made of what it is and a size bin; each edge
//! has a label made of how the two pieces meet. A round of Weisfeiler--Lehman
//! refinement then gives every node the label of its own label together with
//! the sorted labels of its edges and of the nodes on their other side, and
//! the signature counts how many nodes carry each label after each round. A
//! label is the readable text of the pattern, so two views are compared by
//! which patterns they share and how often, and the difference is read off by
//! name.
//!
//! Nothing here depends on where a piece lies or which way it points, or on
//! the order the entities are stored in: a label is built from lengths,
//! radii, swept angles and the angle at which two pieces meet. Turning,
//! moving or mirroring a view changes nothing; scaling it does (the bins are
//! of real lengths).
//!
//! # Labels (`signature_version` 3)
//!
//! - A line is `LINE:b<k>` for its length bin `k`; an arc `ARC:r<k>:a<n>` for
//!   its radius bin `k` and its swept angle in 45-degree steps `n` (rounded
//!   to the nearest, so a quarter circle is 2 whatever the last digits
//!   of a reader's angle); a circle `CIRCLE:b<k>` for its diameter's bin.
//!   Bins are the length bins of layer 1.
//! - Where two pieces meet, the angle between their directions leaving the
//!   meeting point is read as `smooth` (a straight continuation, within
//!   [`ANGLE_TOLERANCE_DEG`]), `orthogonal` (a right angle, within the same
//!   tolerance) or `other`. An arc is taken by its tangent at the end.
//! - Round 0 counts the node labels. Round `r + 1` labels a node
//!   `<its round-r label>[<edge> <neighbour's round-r label>, ...]` with the
//!   entries sorted as text. A label longer than [`LABEL_HASH_OVER`]
//!   characters is replaced by `#` and the 16 hexadecimal digits of the
//!   FNV-1a 64-bit hash of its bytes.
//! - Each round keeps its [`MAX_KEYS`] most frequent labels (ties by label
//!   text) and says when it cut.
//! - A view of more than [`MAX_NODES`] pieces, or with more than [`MAX_EDGES`]
//!   edges, has no counts and says why; so does every view of a drawing that
//!   holds more than [`MAX_TOTAL_PIECES`] pieces. An end whose coordinates are
//!   not finite, or past a trillion millimetres, joins nothing.
//!
//! A line drawn as one piece and the same line drawn as two are different
//! patterns: the signature does not merge pieces.

use crate::signature::bin;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// How near, in micrometres, an end of one piece must lie to an end of another
/// for the two to be joined. A parameter of the quantization.
pub const ENDPOINT_GAP_UM: u64 = 100;

/// How far, in degrees, the angle at which two pieces meet may lie from a
/// straight continuation or a right angle and still be read as one.
pub const ANGLE_TOLERANCE_DEG: u32 = 1;

/// The rounds of refinement after round 0.
pub const ROUNDS: u32 = 2;

/// The most pieces a view may hold for its patterns to be counted. A view
/// with more says so (`wl_note`) and has no counts.
pub const MAX_NODES: usize = 200_000;

/// The most edges a view may have for its patterns to be counted: a heap of
/// pieces ending at one point joins every pair of them. A view with more says
/// so (`wl_note`) and has no counts.
pub const MAX_EDGES: usize = 8 * MAX_NODES;

/// The most pieces a whole drawing may hold for any view's patterns to be
/// counted. Holding them is what costs memory, so past this the drawing's
/// views all say `too_large` and no more pieces are kept.
pub const MAX_TOTAL_PIECES: usize = 2_000_000;

/// The most labels a round keeps.
pub const MAX_KEYS: usize = 512;

/// Coordinates, in millimetres, past which an end is not placed: a number
/// that large is a sentinel or an error, and a cell index made from it would
/// not fit.
const MAX_COORD_MM: f64 = 1e12;

/// A label longer than this many characters is replaced by its hash.
pub const LABEL_HASH_OVER: usize = 120;

/// The parameters layer 3 follows, as the signature states them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct WlParams {
    /// See [`ENDPOINT_GAP_UM`].
    pub endpoint_gap_um: u64,
    /// See [`ANGLE_TOLERANCE_DEG`].
    pub angle_tolerance_deg: u32,
    /// See [`ROUNDS`].
    pub rounds: u32,
    /// See [`MAX_NODES`].
    pub max_nodes: u64,
    /// See [`MAX_EDGES`].
    pub max_edges: u64,
    /// See [`MAX_TOTAL_PIECES`].
    pub max_total_pieces: u64,
    /// See [`MAX_KEYS`].
    pub max_keys: u64,
    /// See [`LABEL_HASH_OVER`].
    pub label_hash_over: u64,
    /// The hash a long label is replaced by.
    pub label_hash: String,
}

impl WlParams {
    pub(crate) fn current() -> Self {
        WlParams {
            endpoint_gap_um: ENDPOINT_GAP_UM,
            angle_tolerance_deg: ANGLE_TOLERANCE_DEG,
            rounds: ROUNDS,
            max_nodes: MAX_NODES as u64,
            max_edges: MAX_EDGES as u64,
            max_total_pieces: MAX_TOTAL_PIECES as u64,
            max_keys: MAX_KEYS as u64,
            label_hash_over: LABEL_HASH_OVER as u64,
            label_hash: "fnv1a-64".to_string(),
        }
    }
}

/// The patterns a view's pieces make: for each round (`r0`, `r1`, ...) the
/// labels and how many pieces carry each.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PatternCounts {
    pub rounds: BTreeMap<String, BTreeMap<String, u64>>,
    /// Whether a round had more labels than [`MAX_KEYS`] and was cut.
    pub truncated: bool,
}

/// One piece of a view, in millimetres in the world's plan.
#[derive(Debug, Clone)]
pub(crate) enum Piece {
    Line {
        from: [f64; 2],
        to: [f64; 2],
    },
    /// The arc `centre + u cos t + v sin t` for `t` from `start` through
    /// `sweep` (`u` and `v` are the images of the radius along x and y, so a
    /// mirror shows as a sweep read the other way).
    Arc {
        centre: [f64; 2],
        u: [f64; 2],
        v: [f64; 2],
        start: f64,
        sweep: f64,
    },
    Circle {
        diameter_um: u64,
    },
}

/// A piece's end: where it is and the direction leading from it into the
/// piece, as a unit vector.
struct End {
    at: [f64; 2],
    inward: [f64; 2],
}

fn unit(v: [f64; 2]) -> Option<[f64; 2]> {
    let n = v[0].hypot(v[1]);
    (n.is_finite() && n > 0.0).then(|| [v[0] / n, v[1] / n])
}

impl Piece {
    /// The node's first label, or `None` for a piece with no length.
    fn label(&self) -> Option<String> {
        match self {
            Piece::Line { from, to } => {
                let um = ((to[0] - from[0]).hypot(to[1] - from[1]) * 1000.0).round();
                (um.is_finite() && um > 0.0).then(|| format!("LINE:b{}", bin(um as u64)))
            }
            Piece::Arc { u, sweep, .. } => {
                let um = (u[0].hypot(u[1]) * 1000.0).round();
                let steps = (sweep.abs().to_degrees() / 45.0).round();
                (um.is_finite() && um > 0.0 && steps.is_finite())
                    .then(|| format!("ARC:r{}:a{}", bin(um as u64), steps as u64))
            }
            Piece::Circle { diameter_um } => {
                (*diameter_um > 0).then(|| format!("CIRCLE:b{}", bin(*diameter_um)))
            }
        }
    }

    /// The ends of the piece that can be placed on the grid.
    fn ends(&self) -> Vec<End> {
        let mut ends = self.ends_unchecked();
        ends.retain(|e| e.at.iter().all(|c| c.is_finite() && c.abs() < MAX_COORD_MM));
        ends
    }

    fn ends_unchecked(&self) -> Vec<End> {
        match self {
            Piece::Line { from, to } => {
                let along = [to[0] - from[0], to[1] - from[1]];
                match unit(along) {
                    Some(d) => vec![
                        End {
                            at: *from,
                            inward: d,
                        },
                        End {
                            at: *to,
                            inward: [-d[0], -d[1]],
                        },
                    ],
                    None => Vec::new(),
                }
            }
            Piece::Arc {
                centre,
                u,
                v,
                start,
                sweep,
            } => {
                let at = |t: f64| {
                    [
                        centre[0] + u[0] * t.cos() + v[0] * t.sin(),
                        centre[1] + u[1] * t.cos() + v[1] * t.sin(),
                    ]
                };
                // The direction the arc is travelled in at parameter `t`.
                let along = |t: f64| {
                    let s = sweep.signum();
                    [
                        s * (-u[0] * t.sin() + v[0] * t.cos()),
                        s * (-u[1] * t.sin() + v[1] * t.cos()),
                    ]
                };
                let end = start + sweep;
                match (unit(along(*start)), unit(along(end))) {
                    (Some(a), Some(b)) => vec![
                        End {
                            at: at(*start),
                            inward: a,
                        },
                        End {
                            at: at(end),
                            inward: [-b[0], -b[1]],
                        },
                    ],
                    _ => Vec::new(),
                }
            }
            Piece::Circle { .. } => Vec::new(),
        }
    }
}

/// How two pieces meet, from the unit directions leading from the meeting
/// point into each.
fn edge_label(a: [f64; 2], b: [f64; 2]) -> &'static str {
    let cos = a[0] * b[0] + a[1] * b[1];
    let tolerance = f64::from(ANGLE_TOLERANCE_DEG).to_radians();
    if cos <= -tolerance.cos() {
        "smooth"
    } else if cos.abs() <= tolerance.sin() {
        "orthogonal"
    } else {
        "other"
    }
}

fn fnv1a_64(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn short(label: String) -> String {
    if label.len() > LABEL_HASH_OVER {
        format!("#{:016x}", fnv1a_64(&label))
    } else {
        label
    }
}

/// The `max` most frequent labels of `counts` (ties by label text), and
/// whether any was left out.
fn most_frequent(counts: BTreeMap<&str, u64>, max: usize) -> (BTreeMap<String, u64>, bool) {
    let mut all: Vec<(&str, u64)> = counts.into_iter().collect();
    // `counts` is ordered by label, and the sort is stable: equal counts keep
    // the label order.
    all.sort_by_key(|a| std::cmp::Reverse(a.1));
    let cut = all.len() > max;
    all.truncate(max);
    (
        all.into_iter().map(|(k, n)| (k.to_string(), n)).collect(),
        cut,
    )
}

/// The patterns `pieces` make, or why they are not counted: `"too_large"`
/// (more than [`MAX_NODES`] pieces) or `"too_dense"` (more than [`MAX_EDGES`]
/// edges).
pub(crate) fn patterns(pieces: &[&Piece]) -> Result<PatternCounts, &'static str> {
    if pieces.len() > MAX_NODES {
        return Err("too_large");
    }
    // Nodes: the pieces that have a size.
    let mut labels: Vec<String> = Vec::new();
    let mut ends: Vec<(usize, End)> = Vec::new();
    for piece in pieces {
        let Some(label) = piece.label() else { continue };
        let node = labels.len();
        labels.push(label);
        ends.extend(piece.ends().into_iter().map(|e| (node, e)));
    }

    // Edges: ends of different pieces within the gap, found through a grid
    // of cells as wide as the gap.
    let gap = ENDPOINT_GAP_UM as f64 / 1000.0;
    let cell = |at: [f64; 2]| ((at[0] / gap).floor() as i64, (at[1] / gap).floor() as i64);
    let mut grid: BTreeMap<(i64, i64), Vec<usize>> = BTreeMap::new();
    for (i, (_, end)) in ends.iter().enumerate() {
        grid.entry(cell(end.at)).or_default().push(i);
    }
    let mut neighbours: Vec<Vec<(&'static str, usize)>> = vec![Vec::new(); labels.len()];
    let mut edges = 0usize;
    for (i, (a_node, a)) in ends.iter().enumerate() {
        let (cx, cy) = cell(a.at);
        for dx in -1..=1 {
            for dy in -1..=1 {
                let Some(candidates) = grid.get(&(cx + dx, cy + dy)) else {
                    continue;
                };
                for &j in candidates {
                    // Each pair of ends once, from its lower index.
                    if j <= i {
                        continue;
                    }
                    let (b_node, b) = &ends[j];
                    if b_node == a_node {
                        continue;
                    }
                    if (a.at[0] - b.at[0]).hypot(a.at[1] - b.at[1]) <= gap {
                        edges += 1;
                        if edges > MAX_EDGES {
                            return Err("too_dense");
                        }
                        let label = edge_label(a.inward, b.inward);
                        neighbours[*a_node].push((label, *b_node));
                        neighbours[*b_node].push((label, *a_node));
                    }
                }
            }
        }
    }

    let mut truncated = false;
    let mut rounds = BTreeMap::new();
    let mut current = labels;
    for round in 0..=ROUNDS {
        let mut counts: BTreeMap<&str, u64> = BTreeMap::new();
        for label in &current {
            *counts.entry(label.as_str()).or_insert(0) += 1;
        }
        let (kept, cut) = most_frequent(counts, MAX_KEYS);
        truncated |= cut;
        rounds.insert(format!("r{round}"), kept);
        if round == ROUNDS {
            break;
        }
        let next: Vec<String> = (0..current.len())
            .map(|node| {
                let mut around: Vec<String> = neighbours[node]
                    .iter()
                    .map(|(edge, other)| format!("{edge} {}", current[*other]))
                    .collect();
                around.sort();
                short(format!("{}[{}]", current[node], around.join(", ")))
            })
            .collect();
        current = next;
    }
    Ok(PatternCounts { rounds, truncated })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(from: [f64; 2], to: [f64; 2]) -> Piece {
        Piece::Line { from, to }
    }

    #[test]
    fn an_end_that_cannot_be_placed_joins_nothing_and_costs_nothing() {
        let pieces = [
            line([0.0, 0.0], [10.0, 0.0]),
            Piece::Arc {
                centre: [f64::NAN, 0.0],
                u: [5.0, 0.0],
                v: [0.0, 5.0],
                start: 0.0,
                sweep: 1.0,
            },
            line([1e30, 0.0], [1e30, 10.0]),
            line([f64::INFINITY, 0.0], [0.0, 3.0]),
        ];
        let refs: Vec<&Piece> = pieces.iter().collect();
        let counts = patterns(&refs).expect("counted");
        // Three are nodes (the line of infinite length has no size to
        // label); none is joined to another.
        assert_eq!(counts.rounds["r0"].values().sum::<u64>(), 3);
        assert!(counts.rounds["r1"].keys().all(|k| k.ends_with("[]")));
    }

    #[test]
    fn a_heap_of_pieces_ending_at_one_point_is_too_dense_not_a_memory_bomb() {
        // Every pair of the pieces meets at the origin: n(n-1)/2 edges.
        let n: u32 = 3_500;
        let pieces: Vec<Piece> = (0..n)
            .map(|i| line([0.0, 0.0], [10.0 + f64::from(i), 7.0]))
            .collect();
        let refs: Vec<&Piece> = pieces.iter().collect();
        assert!((n as usize) * (n as usize - 1) / 2 > MAX_EDGES);
        assert_eq!(patterns(&refs).err(), Some("too_dense"));
    }

    #[test]
    fn a_round_with_too_many_labels_keeps_the_most_frequent_and_says_it_cut() {
        let counts: BTreeMap<&str, u64> = [("a", 1), ("b", 5), ("c", 5), ("d", 3), ("e", 5)]
            .into_iter()
            .collect();
        let (kept, cut) = most_frequent(counts.clone(), 3);
        assert!(cut);
        assert_eq!(
            kept.into_iter().collect::<Vec<_>>(),
            [
                ("b".to_string(), 5),
                ("c".to_string(), 5),
                ("e".to_string(), 5)
            ],
            "the three that occur five times; ties resolved by label"
        );
        let (kept, cut) = most_frequent(counts, 5);
        assert!(!cut);
        assert_eq!(kept.len(), 5);
    }

    #[test]
    fn a_long_label_is_replaced_by_the_hash_of_its_text() {
        let long = "x".repeat(LABEL_HASH_OVER + 1);
        let short_one = short(long.clone());
        assert_eq!(short_one, format!("#{:016x}", fnv1a_64(&long)));
        assert_eq!(short("LINE:b5[]".to_string()), "LINE:b5[]");
        // The hash is FNV-1a 64: the empty string hashes to the offset basis.
        assert_eq!(fnv1a_64(""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a_64("a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn angles_are_read_with_the_tolerance_and_a_straight_continuation_is_smooth() {
        // Directions leading from the meeting point into each piece.
        assert_eq!(edge_label([1.0, 0.0], [-1.0, 0.0]), "smooth");
        assert_eq!(edge_label([1.0, 0.0], [0.0, 1.0]), "orthogonal");
        assert_eq!(edge_label([1.0, 0.0], [1.0, 0.0]), "other");
        let degrees = |d: f64| [d.to_radians().cos(), d.to_radians().sin()];
        assert_eq!(edge_label([1.0, 0.0], degrees(179.5)), "smooth");
        assert_eq!(edge_label([1.0, 0.0], degrees(178.0)), "other");
        assert_eq!(edge_label([1.0, 0.0], degrees(90.5)), "orthogonal");
        assert_eq!(edge_label([1.0, 0.0], degrees(92.0)), "other");
    }
}
