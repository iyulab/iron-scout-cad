//! Which outlines close: the straight and circular pieces of a drawing, grouped
//! by where their ends meet.
//!
//! Two pieces are joined when an end of one lies within the join gap of an end
//! of the other. A group of joined pieces is a [`ContourKind::Loop`] when every
//! end has exactly one partner -- a single closed outline --, a
//! [`ContourKind::Branched`] one when every end has a partner but some have
//! several (a side drawn twice, a T), and [`ContourKind::Open`] when some end
//! has none. A circle is closed on its own. For each open group the free ends
//! are listed with the distance to the nearest end of another piece, so a
//! caller that wants a wider gap can see how much wider.
//!
//! Two pieces that meet at both ends close only when they are not the same
//! curve: drawn on top of each other they are a side drawn twice
//! ([`ContourKind::Branched`]). An arc whose own ends meet closes on itself, as
//! a circle does; a piece shorter than the gap whose ends meet (a repeated
//! vertex) is left out and counted.
//!
//! This says nothing about what an outline is for: which layer cuts and which
//! folds is not read here. It reads the model space's own lines, arcs, circles
//! and polylines in the plan -- the height of a point is not read, so two ends
//! at different heights above the same place are joined; what it does not read
//! is counted in [`Contours::not_read`].

use crate::hit_test::spaces;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uncad_model::bulge;
use uncad_model::model::{Entity, EntityId, Point2D, Ref};
use uncad_model::{CadDatabase, Ocs};

/// How a group of joined pieces closes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[non_exhaustive]
pub enum ContourKind {
    /// Every end has exactly one partner: one closed outline (a circle is one).
    Loop,
    /// Every end has a partner and some have several.
    Branched,
    /// Some end has no partner.
    Open,
}

/// An end with no partner within the gap.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct FreeEnd {
    /// Where the end lies, in the drawing's units.
    pub at: Point2D,
    /// The distance, in millimetres, to the nearest end of another piece of
    /// the drawing; `None` when the drawing has no other end or the search
    /// ran out of its budget.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nearest_gap_mm: Option<f64>,
}

/// One group of joined pieces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Contour {
    pub kind: ContourKind,
    /// The entities whose pieces are in the group, each once, by ID.
    pub entities: Vec<EntityId>,
    /// How many pieces: a line or an arc, each segment of a polyline, a circle.
    pub pieces: u32,
    /// The pieces' lengths added, in millimetres.
    pub length_mm: f64,
    /// The same lengths added by the layer of the piece's entity: a group
    /// that spans layers says how much of its length each one holds.
    #[serde(default)]
    pub length_mm_by_layer: BTreeMap<String, f64>,
    /// The layers of the group's entities, by name, each once.
    pub layers: Vec<String>,
    /// The ends with no partner; empty unless the group is open.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub free_ends: Vec<FreeEnd>,
}

/// The groups of a drawing's pieces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Contours {
    /// Two ends this close (micrometres) are joined.
    pub join_gap_um: u64,
    pub loops: u32,
    pub branched: u32,
    pub open: u32,
    /// By the first entity of each group.
    pub components: Vec<Contour>,
    /// What the drawing holds that is not read here, by name and how many:
    /// `INSERT` (its block is not entered), `ELLIPSE`, `SPLINE` and
    /// `POLYLINE_3D` (not straight or circular pieces in the plan),
    /// `TILTED_PLANE` (an entity whose plane is not the world's plan),
    /// `ARC_SWEEP_UNDEFINED` (an arc whose two angles are equal),
    /// `UNPLACEABLE` (a piece with a number that is not finite or is too large,
    /// or a radius that is not positive), `DEGENERATE` (a piece shorter than
    /// the gap whose ends meet -- a repeated vertex), `NEAREST_GAP_BUDGET`
    /// (free ends left without a nearest distance because the search ran out
    /// of its budget). When the drawing holds more pieces than a group can be
    /// told from (`PIECE_LIMIT`) or too many ends crowd together
    /// (`PARTNER_BUDGET`), no group is told and that name says how many pieces
    /// or ends there were.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub not_read: BTreeMap<String, u32>,
}

/// What [`Contours`] comes to, in a size that does not grow with the
/// drawing: the counts, the lengths by layer and how many ends are free.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ContourTotals {
    /// Two ends this close (micrometres) are joined.
    pub join_gap_um: u64,
    pub loops: u32,
    pub branched: u32,
    pub open: u32,
    /// Ends with no partner, over all open groups.
    pub free_ends: u32,
    /// The lengths of the pieces, added by layer.
    pub length_mm_by_layer: BTreeMap<String, f64>,
    /// The lengths of the closed groups (loops and branched), added.
    pub closed_length_mm: f64,
    /// As [`Contours::not_read`].
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub not_read: BTreeMap<String, u32>,
}

impl Contours {
    /// The totals. A layer's length is the length of the pieces on it, so the
    /// layers' lengths add up to the drawing's pieces even where a group
    /// spans layers.
    pub fn totals(&self) -> ContourTotals {
        let mut by_layer: BTreeMap<String, Vec<f64>> = BTreeMap::new();
        let mut closed = Vec::new();
        let mut free = 0u32;
        for c in &self.components {
            for (layer, length) in &c.length_mm_by_layer {
                by_layer.entry(layer.clone()).or_default().push(*length);
            }
            if c.kind != ContourKind::Open {
                closed.push(c.length_mm);
            }
            free += c.free_ends.len() as u32;
        }
        let by_layer = by_layer
            .into_iter()
            .map(|(layer, lengths)| (layer, sum(lengths)))
            .collect();
        let closed = sum(closed);
        ContourTotals {
            join_gap_um: self.join_gap_um,
            loops: self.loops,
            branched: self.branched,
            open: self.open,
            free_ends: free,
            length_mm_by_layer: by_layer,
            closed_length_mm: closed,
            not_read: self.not_read.clone(),
        }
    }
}

/// The most distance comparisons the nearest-end search may spend.
const SEARCH_BUDGET: u64 = 50_000_000;
/// The most end-to-end comparisons the grouping may spend: a heap of ends at
/// one point joins every pair of them. Past it no group is told.
const PAIR_BUDGET: u64 = 100_000_000;
/// The most pieces a drawing may hold for its groups to be told (the same
/// bound the signature's pattern counts keep).
const MAX_PIECES: usize = crate::wl::MAX_TOTAL_PIECES;
/// Coordinates, in millimetres, past which a piece is not placed: a number
/// that large is a sentinel or an error, and a cell index made from it would
/// not fit.
const MAX_COORD_MM: f64 = 1e12;
const EPS_MM: f64 = 1e-9;

struct Piece {
    entity: usize,
    /// The two ends in the plan, in millimetres; `None` for a piece that
    /// closes on itself (a circle, an arc whose ends meet).
    ends: Option<[[f64; 2]; 2]>,
    /// A point halfway along the piece, in millimetres: what tells two pieces
    /// drawn on top of each other from two that enclose something.
    mid: [f64; 2],
    length_mm: f64,
}

enum Placed {
    Piece(Piece),
    /// A number that is not finite or is too large, a radius that is not
    /// positive: the piece is not placed.
    Unplaceable,
    /// A piece with no length to speak of whose ends meet: a dot.
    Degenerate,
}

fn place(entity: usize, ends: [[f64; 2]; 2], mid: [f64; 2], length_mm: f64, gap_mm: f64) -> Placed {
    let sane = |p: [f64; 2]| p.iter().all(|c| c.is_finite() && c.abs() < MAX_COORD_MM);
    if !(sane(ends[0]) && sane(ends[1]) && sane(mid) && length_mm.is_finite() && length_mm >= 0.0) {
        return Placed::Unplaceable;
    }
    let chord = (ends[0][0] - ends[1][0]).hypot(ends[0][1] - ends[1][1]);
    if chord <= gap_mm + EPS_MM {
        // The ends meet. With no length the piece is a dot (a repeated
        // vertex); with some it closes on itself, as a circle does (an arc
        // drawn from 0 to 360 degrees).
        if length_mm <= gap_mm + EPS_MM {
            return Placed::Degenerate;
        }
        return Placed::Piece(Piece {
            entity,
            ends: None,
            mid,
            length_mm,
        });
    }
    Placed::Piece(Piece {
        entity,
        ends: Some(ends),
        mid,
        length_mm,
    })
}

/// Where an end stands with the ends of other pieces within the gap.
#[derive(Clone, Copy, PartialEq)]
enum Partners {
    None,
    /// One other piece.
    One(usize),
    Several,
}

/// A sum that does not depend on the order the terms came in.
fn sum(mut terms: Vec<f64>) -> f64 {
    terms.sort_by(f64::total_cmp);
    terms.iter().sum()
}

fn kind_rank(k: ContourKind) -> u8 {
    match k {
        ContourKind::Loop => 0,
        ContourKind::Branched => 1,
        ContourKind::Open => 2,
    }
}

/// How the search for the nearest other end ended.
enum Nearest {
    Found(f64),
    /// The drawing has no end of another piece.
    NoOther,
    /// The search ran out of its budget.
    OutOfBudget,
}

/// The groups of the model space's pieces at `join_gap_um`, or `None` when the
/// drawing states no unit (a gap in micrometres means nothing then).
pub fn contours(db: &CadDatabase, join_gap_um: u64) -> Option<Contours> {
    contours_within(db, join_gap_um, PAIR_BUDGET, SEARCH_BUDGET)
}

/// [`contours`] with the budgets it spends given (the tests spend small ones).
fn contours_within(
    db: &CadDatabase,
    join_gap_um: u64,
    pair_budget: u64,
    search_budget: u64,
) -> Option<Contours> {
    let k = db.header.units().and_then(|u| u.to_mm)?;
    let spaces = spaces(db);
    let gap_mm = join_gap_um as f64 / 1000.0;

    let mut owners: Vec<(EntityId, String)> = Vec::new();
    let mut pieces: Vec<Piece> = Vec::new();
    let mut not_read: BTreeMap<String, u32> = BTreeMap::new();
    let skip = |name: &str, not_read: &mut BTreeMap<String, u32>| {
        *not_read.entry(name.to_string()).or_insert(0) += 1;
    };
    // A piece placed, or counted under what it was left out for.
    let keep = |placed: Placed, mine: &mut Vec<Piece>, not_read: &mut BTreeMap<String, u32>| {
        match placed {
            Placed::Piece(p) => mine.push(p),
            Placed::Unplaceable => skip("UNPLACEABLE", not_read),
            Placed::Degenerate => skip("DEGENERATE", not_read),
        }
    };

    for e in &db.entities {
        let id = e.common().id;
        if spaces
            .get(&id)
            .is_some_and(|s| !s.eq_ignore_ascii_case("*Model_Space"))
        {
            continue;
        }
        let mut mine: Vec<Piece> = Vec::new();
        let owner = owners.len();
        match e {
            Entity::Line(l) => {
                let from = [l.start_point.x * k, l.start_point.y * k];
                let to = [l.end_point.x * k, l.end_point.y * k];
                let mid = [(from[0] + to[0]) / 2.0, (from[1] + to[1]) / 2.0];
                let length = (to[0] - from[0]).hypot(to[1] - from[1]);
                keep(
                    place(owner, [from, to], mid, length, gap_mm),
                    &mut mine,
                    &mut not_read,
                );
            }
            Entity::Circle(c) => match Ocs::of(c.extrusion).and_then(|o| o.flat_map()) {
                Some(plan) => {
                    let length = std::f64::consts::TAU * c.radius * k;
                    let ctr = plan.apply(Point2D {
                        x: c.center.x,
                        y: c.center.y,
                    });
                    let at = [ctr.x * k, ctr.y * k];
                    if c.radius.is_finite()
                        && c.radius > 0.0
                        && length.is_finite()
                        && at.iter().all(|v| v.is_finite() && v.abs() < MAX_COORD_MM)
                    {
                        mine.push(Piece {
                            entity: owner,
                            ends: None,
                            mid: at,
                            length_mm: length,
                        });
                    } else {
                        skip("UNPLACEABLE", &mut not_read);
                    }
                }
                None => skip("TILTED_PLANE", &mut not_read),
            },
            Entity::Arc(a) => match (Ocs::of(a.extrusion).and_then(|o| o.flat_map()), a.sweep()) {
                (Some(plan), Some(sweep)) => {
                    let at = |t: f64| {
                        plan.apply(Point2D {
                            x: a.center.x + a.radius * t.cos(),
                            y: a.center.y + a.radius * t.sin(),
                        })
                    };
                    let (p, q, m) = (
                        at(a.start_angle),
                        at(a.start_angle + sweep),
                        at(a.start_angle + sweep / 2.0),
                    );
                    let length = a.radius * sweep.abs() * k;
                    let placed = if a.radius.is_finite() && a.radius > 0.0 {
                        place(
                            owner,
                            [[p.x * k, p.y * k], [q.x * k, q.y * k]],
                            [m.x * k, m.y * k],
                            length,
                            gap_mm,
                        )
                    } else {
                        Placed::Unplaceable
                    };
                    keep(placed, &mut mine, &mut not_read);
                }
                (None, _) => skip("TILTED_PLANE", &mut not_read),
                // An arc with equal angles has no ends to join.
                (_, None) => skip("ARC_SWEEP_UNDEFINED", &mut not_read),
            },
            Entity::LwPolyline(p) | Entity::Polyline2D(p) => {
                match Ocs::of(p.extrusion).and_then(|o| o.flat_map()) {
                    Some(plan) => {
                        for s in bulge::segments(&p.vertices, p.closed) {
                            let (from, to) = (plan.apply(s.from), plan.apply(s.to));
                            let (length, mid) = match s.arc {
                                Some(arc) => (
                                    arc.radius * arc.sweep.abs(),
                                    plan.apply(arc.at(arc.start_angle + arc.sweep / 2.0)),
                                ),
                                None => (
                                    (s.to.x - s.from.x).hypot(s.to.y - s.from.y),
                                    Point2D {
                                        x: (from.x + to.x) / 2.0,
                                        y: (from.y + to.y) / 2.0,
                                    },
                                ),
                            };
                            keep(
                                place(
                                    owner,
                                    [[from.x * k, from.y * k], [to.x * k, to.y * k]],
                                    [mid.x * k, mid.y * k],
                                    length * k,
                                    gap_mm,
                                ),
                                &mut mine,
                                &mut not_read,
                            );
                        }
                    }
                    None => skip("TILTED_PLANE", &mut not_read),
                }
            }
            Entity::Polyline3D(_) => skip("POLYLINE_3D", &mut not_read),
            Entity::Insert(_) => skip("INSERT", &mut not_read),
            Entity::Ellipse(_) => skip("ELLIPSE", &mut not_read),
            Entity::Spline(_) => skip("SPLINE", &mut not_read),
            _ => {}
        }
        if !mine.is_empty() {
            let layer = match &e.common().layer {
                Ref::Resolved(name) | Ref::Unresolved(name) => name.clone(),
                Ref::Absent => String::new(),
            };
            owners.push((id, layer));
            pieces.extend(mine);
        }
    }

    // Past these bounds no group is told, and the drawing says so.
    let untold = |name: &str, how_many: usize, mut not_read: BTreeMap<String, u32>| {
        not_read.insert(name.to_string(), how_many.min(u32::MAX as usize) as u32);
        Some(Contours {
            join_gap_um,
            loops: 0,
            branched: 0,
            open: 0,
            components: Vec::new(),
            not_read,
        })
    };
    if pieces.len() > MAX_PIECES {
        return untold("PIECE_LIMIT", pieces.len(), not_read);
    }

    // The ends that can be joined, with the piece each belongs to.
    let mut ends: Vec<([f64; 2], usize)> = Vec::new();
    for (i, p) in pieces.iter().enumerate() {
        if let Some([a, b]) = p.ends {
            ends.push((a, i));
            ends.push((b, i));
        }
    }

    // Partners of each end: the ends of other pieces within the gap. The cell
    // is wider than the distance that joins, so partners lie in the cell's
    // neighbours.
    let reach = gap_mm + EPS_MM;
    let cell = reach.max(1e-6);
    let key = |p: [f64; 2]| ((p[0] / cell).floor() as i64, (p[1] / cell).floor() as i64);
    let mut grid: BTreeMap<(i64, i64), Vec<usize>> = BTreeMap::new();
    for (n, (at, _)) in ends.iter().enumerate() {
        grid.entry(key(*at)).or_default().push(n);
    }
    let mut parent: Vec<usize> = (0..pieces.len()).collect();
    fn find(parent: &mut [usize], mut a: usize) -> usize {
        while parent[a] != a {
            parent[a] = parent[parent[a]];
            a = parent[a];
        }
        a
    }
    let mut partners = vec![Partners::None; ends.len()];
    let mut comparisons = 0u64;
    for (n, (at, piece)) in ends.iter().enumerate() {
        let (cx, cy) = key(*at);
        for dx in -1..=1 {
            for dy in -1..=1 {
                let Some(cellmates) = grid.get(&(cx + dx, cy + dy)) else {
                    continue;
                };
                comparisons += cellmates.len() as u64;
                if comparisons > pair_budget {
                    return untold("PARTNER_BUDGET", ends.len(), not_read);
                }
                for &m in cellmates {
                    let (other, other_piece) = &ends[m];
                    if m == n || other_piece == piece {
                        continue;
                    }
                    if (at[0] - other[0]).hypot(at[1] - other[1]) <= reach {
                        partners[n] = match partners[n] {
                            Partners::None => Partners::One(*other_piece),
                            Partners::One(p) if p == *other_piece => Partners::One(p),
                            _ => Partners::Several,
                        };
                        let (ra, rb) = (find(&mut parent, *piece), find(&mut parent, *other_piece));
                        if ra != rb {
                            parent[ra] = rb;
                        }
                    }
                }
            }
        }
    }

    // Group the pieces.
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..pieces.len() {
        let r = find(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }
    let mut end_of: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (n, (_, piece)) in ends.iter().enumerate() {
        end_of.entry(*piece).or_default().push(n);
    }

    // How each group closes, and its free ends.
    struct Group {
        members: Vec<usize>,
        kind: ContourKind,
        free: Vec<usize>,
    }
    let mut told: Vec<Group> = Vec::new();
    for members in groups.into_values() {
        let mut kind = ContourKind::Loop;
        let mut free: Vec<usize> = Vec::new();
        for &i in &members {
            for &n in end_of.get(&i).map(Vec::as_slice).unwrap_or_default() {
                match partners[n] {
                    Partners::None => {
                        kind = ContourKind::Open;
                        free.push(n);
                    }
                    Partners::One(_) => {}
                    Partners::Several => {
                        if kind == ContourKind::Loop {
                            kind = ContourKind::Branched;
                        }
                    }
                }
            }
        }
        // Two pieces that meet at both ends enclose something only when they
        // are not the same curve: drawn on top of each other they are a side
        // drawn twice.
        if kind == ContourKind::Loop && members.len() == 2 {
            let (a, b) = (pieces[members[0]].mid, pieces[members[1]].mid);
            if (a[0] - b[0]).hypot(a[1] - b[1]) <= reach {
                kind = ContourKind::Branched;
            }
        }
        told.push(Group {
            members,
            kind,
            free,
        });
    }

    // Nearest other end for the free ends, scanning outward in x. The free
    // ends are taken in the order of their places, so which of them a spent
    // budget leaves without an answer does not depend on the file's order.
    let mut by_x: Vec<usize> = (0..ends.len()).collect();
    by_x.sort_by(|a, b| {
        ends[*a].0[0]
            .total_cmp(&ends[*b].0[0])
            .then(ends[*a].0[1].total_cmp(&ends[*b].0[1]))
    });
    let rank: BTreeMap<usize, usize> = by_x.iter().enumerate().map(|(r, n)| (*n, r)).collect();
    let mut budget = search_budget;
    let mut nearest = |n: usize| -> Nearest {
        let (at, piece) = (ends[n].0, ends[n].1);
        let r = rank[&n];
        let mut best = f64::INFINITY;
        for direction in [-1i64, 1] {
            let mut i = r as i64 + direction;
            while i >= 0 && (i as usize) < by_x.len() {
                let m = by_x[i as usize];
                let (other, other_piece) = (ends[m].0, ends[m].1);
                if (other[0] - at[0]).abs() >= best {
                    break;
                }
                if budget == 0 {
                    return Nearest::OutOfBudget;
                }
                budget -= 1;
                if other_piece != piece {
                    best = best.min((other[0] - at[0]).hypot(other[1] - at[1]));
                }
                i += direction;
            }
        }
        if best.is_finite() {
            Nearest::Found(best)
        } else {
            Nearest::NoOther
        }
    };
    let mut free_in_place_order: Vec<usize> =
        told.iter().flat_map(|g| g.free.iter().copied()).collect();
    free_in_place_order.sort_by(|a, b| {
        ends[*a].0[0]
            .total_cmp(&ends[*b].0[0])
            .then(ends[*a].0[1].total_cmp(&ends[*b].0[1]))
            .then(a.cmp(b))
    });
    let mut answers: BTreeMap<usize, Option<f64>> = BTreeMap::new();
    let mut budget_left_out = 0u32;
    for n in free_in_place_order {
        let answer = match nearest(n) {
            Nearest::Found(d) => Some(d),
            Nearest::NoOther => None,
            Nearest::OutOfBudget => {
                budget_left_out += 1;
                None
            }
        };
        answers.insert(n, answer);
    }

    let mut components = Vec::new();
    let (mut loops, mut branched, mut open) = (0u32, 0u32, 0u32);
    for g in told {
        match g.kind {
            ContourKind::Loop => loops += 1,
            ContourKind::Branched => branched += 1,
            ContourKind::Open => open += 1,
        }
        let ids: BTreeSet<EntityId> = g
            .members
            .iter()
            .map(|i| owners[pieces[*i].entity].0)
            .collect();
        let layers: BTreeSet<String> = g
            .members
            .iter()
            .map(|i| owners[pieces[*i].entity].1.clone())
            .collect();
        let mut by_layer: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
        for i in &g.members {
            by_layer
                .entry(owners[pieces[*i].entity].1.as_str())
                .or_default()
                .push(pieces[*i].length_mm);
        }
        let mut free = g.free;
        free.sort_by(|a, b| {
            ends[*a].0[0]
                .total_cmp(&ends[*b].0[0])
                .then(ends[*a].0[1].total_cmp(&ends[*b].0[1]))
        });
        let free_ends = free
            .iter()
            .map(|&n| FreeEnd {
                at: Point2D {
                    x: ends[n].0[0] / k,
                    y: ends[n].0[1] / k,
                },
                nearest_gap_mm: answers.get(&n).copied().flatten(),
            })
            .collect();
        components.push(Contour {
            kind: g.kind,
            entities: ids.into_iter().collect(),
            pieces: g.members.len() as u32,
            length_mm: sum(g.members.iter().map(|i| pieces[*i].length_mm).collect()),
            length_mm_by_layer: by_layer
                .into_iter()
                .map(|(layer, lengths)| (layer.to_string(), sum(lengths)))
                .collect(),
            layers: layers.into_iter().collect(),
            free_ends,
        });
    }
    components.sort_by(|a, b| {
        a.entities
            .first()
            .cmp(&b.entities.first())
            .then(kind_rank(a.kind).cmp(&kind_rank(b.kind)))
            .then(a.pieces.cmp(&b.pieces))
            .then(a.length_mm.total_cmp(&b.length_mm))
    });
    if budget_left_out > 0 {
        not_read.insert("NEAREST_GAP_BUDGET".to_string(), budget_left_out);
    }
    Some(Contours {
        join_gap_um,
        loops,
        branched,
        open,
        components,
        not_read,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// G26's drawing with its entities replaced by lines drawn from `points`
    /// to the next point, all on layer `CUT`.
    fn lines_through_one_point(n: usize) -> CadDatabase {
        let mut v: serde_json::Value =
            serde_json::from_str(include_str!("../tests/golden/g26.expected.json")).unwrap();
        let template = v["entities"][0].clone();
        let entities: Vec<serde_json::Value> = (0..n)
            .map(|i| {
                let mut e = template.clone();
                e["common"]["id"] = (1000 + i as u64).into();
                e["start_point"] = serde_json::json!({"x": 0.0, "y": 0.0, "z": 0.0});
                e["end_point"] =
                    serde_json::json!({"x": 10.0 + i as f64, "y": (i % 7) as f64, "z": 0.0});
                e
            })
            .collect();
        v["entities"] = entities.into();
        serde_json::from_value(v).unwrap()
    }

    /// Ends crowded at one point spend the grouping's budget: no group is
    /// told, and the drawing says how many ends there were.
    #[test]
    fn a_crowd_of_ends_at_one_point_spends_the_budget_and_no_group_is_told() {
        let db = lines_through_one_point(300);
        let c = contours_within(&db, 100, 10_000, SEARCH_BUDGET).unwrap();
        assert_eq!((c.loops, c.branched, c.open), (0, 0, 0));
        assert!(c.components.is_empty());
        assert_eq!(c.not_read.get("PARTNER_BUDGET"), Some(&600));
        // With the budget it has, the same drawing is one branched group.
        let c = contours(&db, 100).unwrap();
        assert_eq!(c.components.len(), 1);
        assert_eq!(c.components[0].pieces, 300);
        assert!(c.not_read.is_empty(), "{:?}", c.not_read);
    }

    /// A spent nearest-end search leaves the same free ends without an answer
    /// whatever order the file lists them in.
    #[test]
    fn the_free_ends_a_spent_search_leaves_do_not_depend_on_the_files_order() {
        let db = lines_through_one_point(40);
        let mut reversed = db.clone();
        reversed.entities.reverse();
        let a = contours_within(&db, 100, PAIR_BUDGET, 30).unwrap();
        let b = contours_within(&reversed, 100, PAIR_BUDGET, 30).unwrap();
        assert_eq!(a.not_read, b.not_read);
        assert_eq!(a, b);
    }
}
