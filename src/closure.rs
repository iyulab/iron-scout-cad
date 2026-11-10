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
//! folds is not read here. It reads the model space's lines, arcs, circles and
//! polylines in the plan -- the height of a point is not read, so two ends at
//! different heights above the same place are joined -- and those a block
//! reference draws, each instance of the block placed where the reference puts
//! it (a block's entity on layer `0` takes the layer of the reference). A
//! reference is followed only where the placement keeps a circle a circle: one
//! that scales the axes differently, sits on a tilted plane, names a block the
//! drawing does not hold, contains itself, sits inside too many others or
//! would take the walk past its budget is left out and counted under the
//! reason. What it does not read is counted in [`Contours::not_read`].
//!
//! A reference with a grid (a multiple insert) draws its block in every cell
//! of it, the spacing turned with the reference and not scaled by it; a
//! drawing whose reader does not state the grid holds one placement, which
//! is all that can be read of it. The model states no clip and no dynamic
//! variation, so neither is told here: a clipped block gives all its curves.
//! An entity a block marks invisible (a hidden state of a dynamic block) is
//! not drawn and is not read.

use crate::hit_test::spaces;
use crate::limits::{NotEntered, Walk};
use crate::outline::{self, is_sane, Part, Why};
use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use uncad_model::model::{Entity, EntityId, InsertEntity, Point2D, Ref};
use uncad_model::tables::Tables;
use uncad_model::{Affine2, CadDatabase};

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
    /// What the drawing holds that is not read here, by name and how many --
    /// an entity of a block once for each instance of the block:
    /// block references not followed, each counted once however many cells its
    /// grid has (`INSERT_UNRESOLVED`: the drawing holds
    /// no block of that name; `INSERT_CYCLE`: the block contains itself;
    /// `INSERT_TOO_DEEP`: inside 20 others; `INSERT_BUDGET_EXHAUSTED`: its
    /// block would take the walk past ten million entities met inside expanded
    /// blocks; `INSERT_TILTED_PLANE`: placed on a plane tilted out of the
    /// world's; `INSERT_NON_UNIFORM_SCALE`: its placement scales the axes
    /// differently, which turns a circle into an ellipse; `INSERT_EMPTY_GRID`:
    /// a grid with no cell),
    /// `INVISIBLE_IN_BLOCK` (an entity of a block that the file marks
    /// invisible), `ELLIPSE`, `SPLINE` and
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
const EPS_MM: f64 = 1e-9;

struct Piece {
    /// The index of the piece's owner (`Collected::owners`).
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

fn place(ends: [[f64; 2]; 2], mid: [f64; 2], length_mm: f64, gap_mm: f64) -> Placed {
    if !(is_sane(ends[0])
        && is_sane(ends[1])
        && is_sane(mid)
        && length_mm.is_finite()
        && length_mm >= 0.0)
    {
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
            entity: 0,
            ends: None,
            mid,
            length_mm,
        });
    }
    Placed::Piece(Piece {
        entity: 0,
        ends: Some(ends),
        mid,
        length_mm,
    })
}

/// A part of an outline as a piece of the contour search, in millimetres;
/// `scale` is the factor the placement scales every length by (a part's radius
/// is the one its entity states).
fn place_part(part: &Part, k: f64, scale: f64, gap_mm: f64) -> Placed {
    let mm = |p: Point2D| [p.x * k, p.y * k];
    match *part {
        Part::Line { from, to } => {
            let (a, b) = (mm(from), mm(to));
            let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
            place([a, b], mid, (b[0] - a[0]).hypot(b[1] - a[1]), gap_mm)
        }
        Part::Arc {
            radius,
            start,
            sweep,
            ..
        } => {
            let (Some(p), Some(q), Some(m)) = (
                part.at(start),
                part.at(start + sweep),
                part.at(start + sweep / 2.0),
            ) else {
                return Placed::Unplaceable;
            };
            place(
                [mm(p), mm(q)],
                mm(m),
                radius * sweep.abs() * scale * k,
                gap_mm,
            )
        }
        Part::Circle { centre, radius, .. } => {
            let at = mm(centre);
            let length = std::f64::consts::TAU * radius * scale * k;
            if length.is_finite() && is_sane(at) {
                Placed::Piece(Piece {
                    entity: 0,
                    ends: None,
                    mid: at,
                    length_mm: length,
                })
            } else {
                Placed::Unplaceable
            }
        }
    }
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
    let gap_mm = join_gap_um as f64 / 1000.0;

    let Collected {
        owners,
        pieces,
        seen,
        mut not_read,
    } = collect_pieces(db, k, gap_mm);

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
    if seen > MAX_PIECES {
        return untold("PIECE_LIMIT", seen, not_read);
    }

    // The ends that can be joined, with the piece each belongs to.
    let mut ends: Vec<([f64; 2], usize)> = Vec::new();
    for (i, p) in pieces.iter().enumerate() {
        if let Some([a, b]) = p.ends {
            ends.push((a, i));
            ends.push((b, i));
        }
    }

    let reach = gap_mm + EPS_MM;
    let Some((mut parent, partners)) = join_ends(&pieces, &ends, reach, pair_budget) else {
        return untold("PARTNER_BUDGET", ends.len(), not_read);
    };

    let told = tell_groups(&pieces, &ends, &partners, &mut parent, reach);

    let (answers, budget_left_out) = nearest_gaps(&ends, &told, search_budget);

    let (components, loops, branched, open) =
        components_of(told, &pieces, &owners, &ends, &answers, k);
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

/// What collecting the pieces found: each piece's owner, the pieces, how many
/// there were, and the kinds left out with how many of each.
struct Collected {
    /// The entity of the model space that draws a piece and the layer the
    /// piece is on -- a block reference gives one owner for each layer its
    /// block draws on.
    owners: Vec<(EntityId, String)>,
    /// The pieces, up to one past [`MAX_PIECES`].
    pieces: Vec<Piece>,
    /// How many pieces there were, whether or not they were kept.
    seen: usize,
    not_read: BTreeMap<String, u32>,
}

/// Where a block's own coordinates go in the world's plan, and the one factor
/// that placement scales every length by.
#[derive(Clone, Copy)]
struct Placement {
    map: Affine2,
    scale: f64,
}

/// The factor `m` scales every length by, when it is the same in every
/// direction (a mirror is: lengths keep what they were scaled to); `None`
/// when the axes are scaled differently or sheared, or the map flattens the
/// plane.
fn uniform_scale(m: &Affine2) -> Option<f64> {
    const TOLERANCE: f64 = 1e-9;
    let (la, lb) = (m.a.hypot(m.b), m.c.hypot(m.d));
    let dot = m.a * m.c + m.b * m.d;
    let same = la.is_finite()
        && lb.is_finite()
        && la > 0.0
        && (la - lb).abs() <= TOLERANCE * la.max(lb)
        && dot.abs() <= TOLERANCE * la * lb;
    same.then_some(la)
}

fn layer_of(e: &Entity) -> &str {
    match &e.common().layer {
        Ref::Resolved(name) | Ref::Unresolved(name) => name,
        Ref::Absent => "",
    }
}

/// One walk over the model space: the pieces of its lines, circles, arcs and
/// polylines and of those every block reference draws, placed in millimetres.
struct Collector<'a> {
    tables: &'a Tables,
    k: f64,
    gap_mm: f64,
    walk: Walk,
    /// The layer each open reference is effectively on, outermost first: an
    /// entity of the block on layer `0` takes it as its own.
    reference_layers: Vec<String>,
    /// The model space's entity being walked and the owner each layer its
    /// pieces are on has been given.
    top: Option<(EntityId, BTreeMap<String, usize>)>,
    out: Collected,
}

impl<'a> Collector<'a> {
    fn skip(&mut self, name: &str) {
        *self.out.not_read.entry(name.to_string()).or_insert(0) += 1;
    }

    /// The owner of a piece on `layer` in the model-space entity being walked.
    fn owner(&mut self, layer: &str) -> usize {
        let (id, by_layer) = self.top.as_mut().expect("an entity is being walked");
        if let Some(owner) = by_layer.get(layer) {
            return *owner;
        }
        let owner = self.out.owners.len();
        self.out.owners.push((*id, layer.to_string()));
        by_layer.insert(layer.to_string(), owner);
        owner
    }

    /// A piece placed, or counted under what it was left out for. Pieces past
    /// the limit are counted and not kept.
    fn keep(&mut self, placed: Placed, layer: &str) {
        match placed {
            Placed::Piece(mut piece) => {
                self.out.seen += 1;
                if self.out.pieces.len() <= MAX_PIECES {
                    piece.entity = self.owner(layer);
                    self.out.pieces.push(piece);
                }
            }
            Placed::Unplaceable => self.skip("UNPLACEABLE"),
            Placed::Degenerate => self.skip("DEGENERATE"),
        }
    }

    /// The pieces `e` draws -- those of a block reference included, instance
    /// by instance -- placed through `at`.
    fn entity(&mut self, e: &Entity, at: &Placement) {
        let own = layer_of(e);
        let layer = match self.reference_layers.last() {
            Some(reference) if own == "0" => reference.clone(),
            _ => own.to_string(),
        };
        match e {
            Entity::Line(_)
            | Entity::Circle(_)
            | Entity::Arc(_)
            | Entity::LwPolyline(_)
            | Entity::Polyline2D(_) => match outline::parts(e, &at.map) {
                Ok(parts) => {
                    for part in &parts {
                        let placed = place_part(part, self.k, at.scale, self.gap_mm);
                        self.keep(placed, &layer);
                    }
                }
                Err(Why::Tilted) => self.skip("TILTED_PLANE"),
                // An arc with equal angles has no ends to join.
                Err(Why::SweepUndefined) => self.skip("ARC_SWEEP_UNDEFINED"),
                Err(Why::BadRadius) => self.skip("UNPLACEABLE"),
            },
            Entity::Insert(insert) => self.insert(insert, at, &layer),
            Entity::Polyline3D(_) => self.skip("POLYLINE_3D"),
            Entity::Ellipse(_) => self.skip("ELLIPSE"),
            Entity::Spline(_) => self.skip("SPLINE"),
            _ => {}
        }
    }

    /// Walks the block `insert` places -- once for each cell of its grid, if
    /// it has one -- each of its entities through the reference's placement
    /// and then `at`; counts the reference under the reason when it cannot,
    /// once whatever the grid. A reason that holds for one cell holds for
    /// the rest (the block, the plane, the scale, the depth), except the
    /// budget, which the cells spend as they go: either way the cells after
    /// the first one left out are left out.
    fn insert(&mut self, insert: &InsertEntity, at: &Placement, layer: &str) {
        let mut any = false;
        for (column, row) in insert.cells() {
            any = true;
            if !self.insert_cell(insert, at, layer, column, row) {
                return;
            }
        }
        if !any {
            self.skip("INSERT_EMPTY_GRID");
        }
    }

    /// One placement of the block `insert` makes: the cell at `column` and
    /// `row`. `false` when the block was not entered, the reference counted
    /// under the reason.
    fn insert_cell(
        &mut self,
        insert: &InsertEntity,
        at: &Placement,
        layer: &str,
        column: u32,
        row: u32,
    ) -> bool {
        let tables: &'a Tables = self.tables;
        let differs = Cell::new(false);
        let entered = self.walk.enter(tables, &insert.block_name, |block| {
            let map = insert
                .world_transform_at(block.base_point, column, row)?
                .then(&at.map);
            match uniform_scale(&map) {
                Some(scale) => Some(Placement { map, scale }),
                None => {
                    differs.set(true);
                    None
                }
            }
        });
        let (block, inner) = match entered {
            Ok(entered) => entered,
            Err(why) => {
                self.skip(match why {
                    NotEntered::Absent | NotEntered::Unresolved | NotEntered::Undefined => {
                        "INSERT_UNRESOLVED"
                    }
                    NotEntered::Cycle => "INSERT_CYCLE",
                    NotEntered::TooDeep => "INSERT_TOO_DEEP",
                    NotEntered::Tilted if differs.get() => "INSERT_NON_UNIFORM_SCALE",
                    NotEntered::Tilted => "INSERT_TILTED_PLANE",
                    NotEntered::BudgetExhausted => "INSERT_BUDGET_EXHAUSTED",
                });
                return false;
            }
        };
        self.reference_layers.push(layer.to_string());
        for inner_entity in &block.entities {
            if inner_entity.common().invisible {
                self.skip("INVISIBLE_IN_BLOCK");
            } else {
                self.entity(inner_entity, &inner);
            }
        }
        self.reference_layers.pop();
        self.walk.leave();
        true
    }
}

/// The pieces of model space's lines, circles, arcs and polylines, and of the
/// blocks its references draw, placed in millimetres, with the owner of each;
/// what was left out is counted under what it was left out for.
fn collect_pieces(db: &CadDatabase, k: f64, gap_mm: f64) -> Collected {
    let spaces = spaces(db);
    let mut collector = Collector {
        tables: &db.tables,
        k,
        gap_mm,
        walk: Walk::default(),
        reference_layers: Vec::new(),
        top: None,
        out: Collected {
            owners: Vec::new(),
            pieces: Vec::new(),
            seen: 0,
            not_read: BTreeMap::new(),
        },
    };
    let top = Placement {
        map: Affine2::IDENTITY,
        scale: 1.0,
    };
    for e in &db.entities {
        let id = e.common().id;
        if spaces
            .get(&id)
            .is_some_and(|s| !s.eq_ignore_ascii_case("*Model_Space"))
        {
            continue;
        }
        collector.top = Some((id, BTreeMap::new()));
        collector.entity(e, &top);
    }
    collector.out
}

fn find(parent: &mut [usize], mut a: usize) -> usize {
    while parent[a] != a {
        parent[a] = parent[parent[a]];
        a = parent[a];
    }
    a
}

/// The partners of each end -- the ends of other pieces within `reach` -- and
/// the pieces those joins connect, as a forest of parents. `None` when the
/// search spends more than `pair_budget` comparisons.
fn join_ends(
    pieces: &[Piece],
    ends: &[([f64; 2], usize)],
    reach: f64,
    pair_budget: u64,
) -> Option<(Vec<usize>, Vec<Partners>)> {
    // Partners of each end: the ends of other pieces within the gap. The cell
    // is wider than the distance that joins, so partners lie in the cell's
    // neighbours.
    let cell = reach.max(1e-6);
    let key = |p: [f64; 2]| ((p[0] / cell).floor() as i64, (p[1] / cell).floor() as i64);
    let mut grid: BTreeMap<(i64, i64), Vec<usize>> = BTreeMap::new();
    for (n, (at, _)) in ends.iter().enumerate() {
        grid.entry(key(*at)).or_default().push(n);
    }
    let mut parent: Vec<usize> = (0..pieces.len()).collect();
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
                    return None;
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
    Some((parent, partners))
}

/// A group of pieces that meet, how it closes and its free ends.
struct Group {
    members: Vec<usize>,
    kind: ContourKind,
    free: Vec<usize>,
}

/// The groups of the pieces the joins connect, each with how it closes and
/// which of its ends are free.
fn tell_groups(
    pieces: &[Piece],
    ends: &[([f64; 2], usize)],
    partners: &[Partners],
    parent: &mut [usize],
    reach: f64,
) -> Vec<Group> {
    // Group the pieces.
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..pieces.len() {
        let r = find(parent, i);
        groups.entry(r).or_default().push(i);
    }
    let mut end_of: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (n, (_, piece)) in ends.iter().enumerate() {
        end_of.entry(*piece).or_default().push(n);
    }

    // How each group closes, and its free ends.
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
    told
}

/// For each free end, the distance to the nearest end of another piece (or
/// `None`), and how many ends the search budget left without an answer.
fn nearest_gaps(
    ends: &[([f64; 2], usize)],
    told: &[Group],
    search_budget: u64,
) -> (BTreeMap<usize, Option<f64>>, u32) {
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
    (answers, budget_left_out)
}

/// The contours the groups make, ordered, and how many of them are loops,
/// branched and open.
fn components_of(
    told: Vec<Group>,
    pieces: &[Piece],
    owners: &[(EntityId, String)],
    ends: &[([f64; 2], usize)],
    answers: &BTreeMap<usize, Option<f64>>,
    k: f64,
) -> (Vec<Contour>, u32, u32, u32) {
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
    (components, loops, branched, open)
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
                e["common"]["id"] = (1000 + i as u64).to_string().into();
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
