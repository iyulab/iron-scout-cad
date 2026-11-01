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
//! This says nothing about what an outline is for: which layer cuts and which
//! folds is not read here. It reads the model space's own lines, arcs, circles
//! and polylines; what it does not read is counted in [`Contours::not_read`].

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
    /// `INSERT` (its block is not entered), `ELLIPSE` and `SPLINE` (not
    /// straight or circular pieces), `TILTED_PLANE` (an entity whose plane is
    /// not the world's plan), `NEAREST_GAP_BUDGET` (free ends left without a
    /// nearest distance).
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
    /// The lengths of the groups, added by layer (see [`Contours::totals`]
    /// for a group that spans layers).
    pub length_mm_by_layer: BTreeMap<String, f64>,
    /// The lengths of the closed groups (loops and branched), added.
    pub closed_length_mm: f64,
    /// As [`Contours::not_read`].
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub not_read: BTreeMap<String, u32>,
}

impl Contours {
    /// The totals. A layer's length is the length of the pieces on it only
    /// when each group sits on one layer; a group that spans layers adds its
    /// whole length to each of them, so the layers' sum can exceed the drawing's.
    pub fn totals(&self) -> ContourTotals {
        let mut by_layer: BTreeMap<String, f64> = BTreeMap::new();
        let mut closed = 0.0;
        let mut free = 0u32;
        for c in &self.components {
            for layer in &c.layers {
                *by_layer.entry(layer.clone()).or_insert(0.0) += c.length_mm;
            }
            if c.kind != ContourKind::Open {
                closed += c.length_mm;
            }
            free += c.free_ends.len() as u32;
        }
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
const EPS_MM: f64 = 1e-9;

struct Piece {
    entity: usize,
    /// The two ends in the plan, in millimetres; `None` for a circle.
    ends: Option<[[f64; 2]; 2]>,
    length_mm: f64,
}

/// The groups of the model space's pieces at `join_gap_um`, or `None` when the
/// drawing states no unit (a gap in micrometres means nothing then).
pub fn contours(db: &CadDatabase, join_gap_um: u64) -> Option<Contours> {
    let k = db.header.units().and_then(|u| u.to_mm)?;
    let spaces = spaces(db);
    let gap_mm = join_gap_um as f64 / 1000.0;

    let mut owners: Vec<(EntityId, String)> = Vec::new();
    let mut pieces: Vec<Piece> = Vec::new();
    let mut not_read: BTreeMap<String, u32> = BTreeMap::new();
    let skip = |name: &str, not_read: &mut BTreeMap<String, u32>| {
        *not_read.entry(name.to_string()).or_insert(0) += 1;
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
                mine.push(Piece {
                    entity: owner,
                    ends: Some([from, to]),
                    length_mm: (to[0] - from[0]).hypot(to[1] - from[1]),
                });
            }
            Entity::Circle(c) => match Ocs::of(c.extrusion).and_then(|o| o.flat_map()) {
                Some(_) => mine.push(Piece {
                    entity: owner,
                    ends: None,
                    length_mm: std::f64::consts::TAU * c.radius * k,
                }),
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
                    let (p, q) = (at(a.start_angle), at(a.start_angle + sweep));
                    mine.push(Piece {
                        entity: owner,
                        ends: Some([[p.x * k, p.y * k], [q.x * k, q.y * k]]),
                        length_mm: a.radius * sweep.abs() * k,
                    });
                }
                (None, _) => skip("TILTED_PLANE", &mut not_read),
                // An arc with equal angles has no ends to join.
                (_, None) => {}
            },
            Entity::LwPolyline(p) | Entity::Polyline2D(p) => {
                match Ocs::of(p.extrusion).and_then(|o| o.flat_map()) {
                    Some(plan) => {
                        for s in bulge::segments(&p.vertices, p.closed) {
                            let (from, to) = (plan.apply(s.from), plan.apply(s.to));
                            let length = match s.arc {
                                Some(arc) => arc.radius * arc.sweep.abs(),
                                None => (s.to.x - s.from.x).hypot(s.to.y - s.from.y),
                            };
                            mine.push(Piece {
                                entity: owner,
                                ends: Some([[from.x * k, from.y * k], [to.x * k, to.y * k]]),
                                length_mm: length * k,
                            });
                        }
                    }
                    None => skip("TILTED_PLANE", &mut not_read),
                }
            }
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

    // The ends that can be joined, with the piece each belongs to.
    let mut ends: Vec<([f64; 2], usize)> = Vec::new();
    for (i, p) in pieces.iter().enumerate() {
        if let Some([a, b]) = p.ends {
            if a.iter().chain(b.iter()).all(|c| c.is_finite()) {
                ends.push((a, i));
                ends.push((b, i));
            }
        }
    }

    // Partners of each end: the ends of other pieces within the gap.
    let cell = gap_mm.max(1e-6);
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
    let mut partners = vec![0u32; ends.len()];
    for (n, (at, piece)) in ends.iter().enumerate() {
        let (cx, cy) = key(*at);
        for dx in -1..=1 {
            for dy in -1..=1 {
                let Some(cellmates) = grid.get(&(cx + dx, cy + dy)) else {
                    continue;
                };
                for &m in cellmates {
                    let (other, other_piece) = &ends[m];
                    if m == n || other_piece == piece {
                        continue;
                    }
                    if (at[0] - other[0]).hypot(at[1] - other[1]) <= gap_mm + EPS_MM {
                        partners[n] += 1;
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

    // Nearest other end for the free ends, by scanning outward in x.
    let mut by_x: Vec<usize> = (0..ends.len()).collect();
    by_x.sort_by(|a, b| ends[*a].0[0].total_cmp(&ends[*b].0[0]));
    let rank: BTreeMap<usize, usize> = by_x.iter().enumerate().map(|(r, n)| (*n, r)).collect();
    let mut budget = SEARCH_BUDGET;
    let mut nearest = |n: usize| -> Option<f64> {
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
                    return None;
                }
                budget -= 1;
                if other_piece != piece {
                    best = best.min((other[0] - at[0]).hypot(other[1] - at[1]));
                }
                i += direction;
            }
        }
        best.is_finite().then_some(best)
    };

    let mut components = Vec::new();
    let (mut loops, mut branched, mut open) = (0u32, 0u32, 0u32);
    let mut budget_left_out = 0u32;
    for members in groups.values() {
        let mut kind = ContourKind::Loop;
        let mut free: Vec<usize> = Vec::new();
        for &i in members {
            for &n in end_of.get(&i).map(Vec::as_slice).unwrap_or_default() {
                match partners[n] {
                    0 => {
                        kind = ContourKind::Open;
                        free.push(n);
                    }
                    1 => {}
                    _ => {
                        if kind == ContourKind::Loop {
                            kind = ContourKind::Branched;
                        }
                    }
                }
            }
            // A piece whose ends could not be placed has none to join: it is open.
            if pieces[i].ends.is_some() && !end_of.contains_key(&i) {
                kind = ContourKind::Open;
            }
        }
        match kind {
            ContourKind::Loop => loops += 1,
            ContourKind::Branched => branched += 1,
            ContourKind::Open => open += 1,
        }
        let ids: BTreeSet<EntityId> = members
            .iter()
            .map(|i| owners[pieces[*i].entity].0)
            .collect();
        let layers: BTreeSet<String> = members
            .iter()
            .map(|i| owners[pieces[*i].entity].1.clone())
            .collect();
        let free_ends = free
            .iter()
            .map(|&n| {
                let nearest_gap_mm = nearest(n);
                if nearest_gap_mm.is_none() && ends.len() > 1 {
                    budget_left_out += 1;
                }
                FreeEnd {
                    at: Point2D {
                        x: ends[n].0[0] / k,
                        y: ends[n].0[1] / k,
                    },
                    nearest_gap_mm,
                }
            })
            .collect();
        components.push(Contour {
            kind,
            entities: ids.into_iter().collect(),
            pieces: members.len() as u32,
            length_mm: members.iter().map(|i| pieces[*i].length_mm).sum(),
            layers: layers.into_iter().collect(),
            free_ends,
        });
    }
    components.sort_by_key(|c| c.entities.first().copied());
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
