//! A drawing's shape signature: what its model space draws, counted into
//! integers whose every place has a stated meaning, so two drawings can be
//! compared component by component and the difference read off by name.
//!
//! The signature counts; it never weighs, ranks or compares -- how close two
//! signatures are, and which components matter, is the caller's to decide.
//!
//! # What is counted
//!
//! The entities of model space, with every block reference expanded into the
//! block it draws (recursively), except a block reference that carries
//! attributes: such a reference is not expanded, and is listed by block name
//! in [`Signature::excluded_inserts`] instead. Paper space is not counted --
//! its viewports show model space again, and its layouts hold sheet frames.
//! Text, hatches, leaders and the like are not geometry and are not counted;
//! dimensions and feature control frames are counted in their own
//! components. Anything else that draws but is not measured here is counted
//! by type in [`Signature::not_measured`].
//!
//! Expansion is bounded, as a drawing's block references may nest or fan out
//! without end: a reference inside 20 others is not followed, nor is one
//! whose block would take the walk past ten million entities met inside
//! expanded blocks. Each such reference is counted in `not_measured`, so a
//! signature short of what the drawing holds says so.
//!
//! # Quantization (`signature_version` 1)
//!
//! - A length is taken to millimetres through the unit the header states and
//!   rounded to the nearest micrometre (`f64::round`, halves away from zero).
//!   When the header states no unit with a millimetre factor, every length
//!   component is `None`: a unit is never assumed.
//! - A length bin is the integer `k` with `2^(k-1) mm <= length < 2^k mm`,
//!   bin 0 holding everything below 1 mm; the bins are taken on whole
//!   millimetres of the micrometre value, so the binning itself does no
//!   floating-point arithmetic.
//! - A circle's diameter is kept exactly, in micrometres.
//!
//! A block reference whose scale is the same in every axis scales lengths and
//! radii by that factor. Under a scale that differs between axes, a circle or
//! an arc is no longer one and is counted in `not_measured`; a straight line
//! is still measured, through the reference's full transform.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uncad_model::model::{DimensionKind, Entity, InsertEntity};
use uncad_model::tables::Tables;
use uncad_model::{CadDatabase, Ocs, Point2D, Point3D};

use crate::limits::{NotEntered, Walk};
use crate::summary::DimensionSummary;

/// The quantization this crate's signatures follow; see the module doc. A
/// change to any rule there is a new version.
pub const SIGNATURE_VERSION: u32 = 1;

/// A drawing's shape signature. See the module doc for what is counted and
/// how lengths are quantized.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Signature {
    /// The quantization these numbers follow ([`SIGNATURE_VERSION`]).
    pub signature_version: u32,
    /// Geometry by entity type name, block contents included. A polyline is
    /// counted as itself and its segments as `POLYLINE_LINE_SEGMENT` and
    /// `POLYLINE_ARC_SEGMENT`.
    pub entities: BTreeMap<String, u64>,
    /// Circle diameters in micrometres, each with how many circles have it.
    /// `None` when the drawing's unit is not stated.
    pub circle_diameters_um: Option<BTreeMap<u64, u64>>,
    /// Straight lengths (lines and polyline line segments) per length bin,
    /// from bin 0 up to the last bin that is not empty. `None` when the
    /// drawing's unit is not stated.
    pub line_length_bins: Option<Vec<u64>>,
    /// Arc radii (arcs and polyline arc segments) per length bin, as for
    /// lines. `None` when the drawing's unit is not stated.
    pub arc_radius_bins: Option<Vec<u64>>,
    /// Dimensions by kind (`ROTATED`, `DIAMETER`, ...; `UNSTATED` when the
    /// file gives none), block contents included.
    pub dimensions: BTreeMap<String, u64>,
    /// Where the counted dimensions state a tolerance, and how many feature
    /// control frames there are.
    pub tolerances: ToleranceCounts,
    /// Block references carrying attributes, which were not expanded: block
    /// name and how many references.
    pub excluded_inserts: BTreeMap<String, u64>,
    /// What draws but was not measured: an entity type the signature does
    /// not measure (`UNKNOWN` for every type the model has no shape for), a
    /// circle or arc under a scale that differs between axes
    /// (`CIRCLE_NON_UNIFORM_SCALE`, ...), a reference to a block the drawing
    /// does not hold or one that contains itself (`INSERT_UNRESOLVED`,
    /// `INSERT_CYCLE`), and a reference not expanded because it sits inside
    /// 20 others (`INSERT_TOO_DEEP`) or because its block would take the
    /// count past ten million entities met inside expanded blocks
    /// (`INSERT_BUDGET_EXHAUSTED`). Either of the last two means the other
    /// components are short of what the drawing holds.
    pub not_measured: BTreeMap<String, u64>,
}

/// How many counted dimensions state a tolerance, by where they state it --
/// a dimension stating it in more than one place is counted in each -- and
/// how many feature control frames (TOLERANCE) there are.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ToleranceCounts {
    /// Its dimension style shows tolerances or limits.
    pub style: u64,
    /// It overrides its style's tolerance variables.
    pub overrides: u64,
    /// Its text writes a stack (`\S+0.1^-0.05;`).
    pub text: u64,
    /// Feature control frames.
    pub frames: u64,
}

/// The signature of `db`'s model space.
pub fn signature(db: &CadDatabase) -> Signature {
    let to_mm = db.header.units().and_then(|u| u.to_mm);
    let mut counter = Counter {
        tables: &db.tables,
        to_mm,
        entities: BTreeMap::new(),
        diameters: BTreeMap::new(),
        line_bins: Vec::new(),
        arc_bins: Vec::new(),
        dimensions: BTreeMap::new(),
        tolerances: ToleranceCounts::default(),
        excluded: BTreeMap::new(),
        not_measured: BTreeMap::new(),
        blocks: Walk::default(),
    };
    let model = db
        .tables
        .block_records
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("*Model_Space"));
    if let Some((_, block)) = model {
        counter.walk(&block.entities, &Transform::identity());
    }
    let known = to_mm.is_some();
    Signature {
        signature_version: SIGNATURE_VERSION,
        entities: counter.entities,
        circle_diameters_um: known.then_some(counter.diameters),
        line_length_bins: known.then_some(counter.line_bins),
        arc_radius_bins: known.then_some(counter.arc_bins),
        dimensions: counter.dimensions,
        tolerances: counter.tolerances,
        excluded_inserts: counter.excluded,
        not_measured: counter.not_measured,
    }
}

/// The linear part of a block reference's placement, from a block's own
/// coordinates to model space, and the one factor it scales every length by
/// when it does (`None` under a scale that differs between axes).
#[derive(Debug, Clone, Copy)]
struct Transform {
    /// Columns: where the block's x, y and z unit vectors go.
    columns: [Point3D; 3],
    uniform: Option<f64>,
}

impl Transform {
    fn identity() -> Self {
        Transform {
            columns: [p3(1.0, 0.0, 0.0), p3(0.0, 1.0, 0.0), p3(0.0, 0.0, 1.0)],
            uniform: Some(1.0),
        }
    }

    fn apply(&self, v: Point3D) -> Point3D {
        let [a, b, c] = self.columns;
        p3(
            a.x * v.x + b.x * v.y + c.x * v.z,
            a.y * v.x + b.y * v.y + c.y * v.z,
            a.z * v.x + b.z * v.y + c.z * v.z,
        )
    }

    /// This transform followed by `insert`'s placement of its block: the
    /// block's axes scaled, rotated about its z, then laid in the plane
    /// its extrusion names.
    fn then(&self, insert: &InsertEntity) -> Option<Self> {
        let ocs = Ocs::of(insert.extrusion)?;
        let (s, c) = insert.rotation.sin_cos();
        let (x, y, z) = (ocs.x_axis(), ocs.y_axis(), ocs.z_axis());
        let rotated_x = add(scale(x, c), scale(y, s));
        let rotated_y = add(scale(x, -s), scale(y, c));
        let k = insert.scale;
        let local = [scale(rotated_x, k.x), scale(rotated_y, k.y), scale(z, k.z)];
        let columns = [
            self.apply(local[0]),
            self.apply(local[1]),
            self.apply(local[2]),
        ];
        let magnitudes = [k.x.abs(), k.y.abs(), k.z.abs()];
        let same = |a: f64, b: f64| (a - b).abs() <= 1e-9 * a.max(b);
        let uniform = match self.uniform {
            Some(u) if same(magnitudes[0], magnitudes[1]) && same(magnitudes[0], magnitudes[2]) => {
                Some(u * magnitudes[0])
            }
            _ => None,
        };
        Some(Transform { columns, uniform })
    }
}

struct Counter<'a> {
    tables: &'a Tables,
    to_mm: Option<f64>,
    entities: BTreeMap<String, u64>,
    diameters: BTreeMap<u64, u64>,
    line_bins: Vec<u64>,
    arc_bins: Vec<u64>,
    dimensions: BTreeMap<String, u64>,
    tolerances: ToleranceCounts,
    excluded: BTreeMap<String, u64>,
    not_measured: BTreeMap<String, u64>,
    /// The blocks being expanded, and what expanding them has cost.
    blocks: Walk,
}

fn bump(map: &mut BTreeMap<String, u64>, key: &str) {
    // A key is met once per entity and is almost always there already; only
    // the first meeting allocates it.
    match map.get_mut(key) {
        Some(n) => *n += 1,
        None => {
            map.insert(key.to_string(), 1);
        }
    }
}

impl Counter<'_> {
    fn walk(&mut self, entities: &[Entity], at: &Transform) {
        for e in entities {
            self.count(e, at);
        }
    }

    fn count(&mut self, e: &Entity, at: &Transform) {
        match e {
            Entity::Line(l) => {
                bump(&mut self.entities, "LINE");
                let v = sub(l.end_point, l.start_point);
                self.line(norm(at.apply(v)));
            }
            Entity::Circle(c) => {
                bump(&mut self.entities, "CIRCLE");
                match at.uniform {
                    Some(u) => {
                        if let Some(um) = self.micrometres(2.0 * c.radius * u) {
                            *self.diameters.entry(um).or_insert(0) += 1;
                        }
                    }
                    None => bump(&mut self.not_measured, "CIRCLE_NON_UNIFORM_SCALE"),
                }
            }
            Entity::Arc(a) => {
                bump(&mut self.entities, "ARC");
                match at.uniform {
                    Some(u) => self.arc(a.radius * u),
                    None => bump(&mut self.not_measured, "ARC_NON_UNIFORM_SCALE"),
                }
            }
            Entity::LwPolyline(p) | Entity::Polyline2D(p) => {
                bump(&mut self.entities, e.type_name());
                let Some(ocs) = Ocs::of(p.extrusion) else {
                    bump(&mut self.not_measured, e.type_name());
                    return;
                };
                let in_plane = |v: Point2D| add(scale(ocs.x_axis(), v.x), scale(ocs.y_axis(), v.y));
                for segment in uncad_model::bulge::segments(&p.vertices, p.closed) {
                    match segment.arc {
                        None => {
                            bump(&mut self.entities, "POLYLINE_LINE_SEGMENT");
                            let v = Point2D {
                                x: segment.to.x - segment.from.x,
                                y: segment.to.y - segment.from.y,
                            };
                            self.line(norm(at.apply(in_plane(v))));
                        }
                        Some(arc) => {
                            bump(&mut self.entities, "POLYLINE_ARC_SEGMENT");
                            match at.uniform {
                                Some(u) => self.arc(arc.radius * u),
                                None => bump(
                                    &mut self.not_measured,
                                    "POLYLINE_ARC_SEGMENT_NON_UNIFORM_SCALE",
                                ),
                            }
                        }
                    }
                }
            }
            Entity::Ellipse(_) | Entity::Spline(_) | Entity::Point(_) => {
                bump(&mut self.entities, e.type_name());
            }
            Entity::Insert(i) => self.insert(i, at),
            Entity::Dimension(d) => {
                let summary = DimensionSummary::of(d, self.tables);
                bump(&mut self.dimensions, kind_name(summary.kind));
                let style = summary
                    .style_tolerance
                    .is_some_and(|t| t.shown == Some(true) || t.limits == Some(true));
                let overrides = summary
                    .tolerance_overrides
                    .as_ref()
                    .is_some_and(|o| !o.is_empty());
                self.tolerances.style += u64::from(style);
                self.tolerances.overrides += u64::from(overrides);
                self.tolerances.text += u64::from(!summary.text_stacks.is_empty());
            }
            Entity::Tolerance(_) => self.tolerances.frames += 1,
            // Text, attributes, hatches, leaders, tables, images and the
            // like annotate or fill; they are not the shape.
            Entity::Text(_)
            | Entity::MText(_)
            | Entity::Attrib(_)
            | Entity::Attdef(_)
            | Entity::Hatch(_)
            | Entity::Leader(_)
            | Entity::MultiLeader(_)
            | Entity::AcadTable(_)
            | Entity::Wipeout(_)
            | Entity::Image(_)
            | Entity::Viewport(_)
            | Entity::Light(_) => {}
            // An entity the model has no shape for is counted under the
            // model's own word for it: the name the file gave it is the
            // reader's (one reader names a type another cannot), and the
            // signature must not depend on which reader read the drawing.
            Entity::Unknown { .. } => bump(&mut self.not_measured, "UNKNOWN"),
            _ => bump(&mut self.not_measured, e.type_name()),
        }
    }

    fn insert(&mut self, i: &InsertEntity, at: &Transform) {
        let Some(name) = i.block_name.resolved() else {
            bump(&mut self.not_measured, "INSERT_UNRESOLVED");
            return;
        };
        if !i.attribs.is_empty() {
            bump(&mut self.excluded, name);
            return;
        }
        let (block, inner) = match self
            .blocks
            .enter(self.tables, &i.block_name, |_| at.then(i))
        {
            Ok(entered) => entered,
            Err(why) => {
                let key = match why {
                    NotEntered::Absent | NotEntered::Unresolved | NotEntered::Undefined => {
                        "INSERT_UNRESOLVED"
                    }
                    NotEntered::Cycle => "INSERT_CYCLE",
                    NotEntered::TooDeep => "INSERT_TOO_DEEP",
                    NotEntered::Tilted => "INSERT",
                    NotEntered::BudgetExhausted => "INSERT_BUDGET_EXHAUSTED",
                };
                bump(&mut self.not_measured, key);
                return;
            }
        };
        self.walk(&block.entities, &inner);
        self.blocks.leave();
    }

    /// `length` (drawing units) in whole micrometres, when the unit is known
    /// and the length is a finite number.
    fn micrometres(&mut self, length: f64) -> Option<u64> {
        let um = (length * self.to_mm? * 1000.0).round();
        if um.is_finite() && um >= 0.0 {
            Some(um as u64)
        } else {
            bump(&mut self.not_measured, "NON_FINITE_LENGTH");
            None
        }
    }

    fn line(&mut self, length: f64) {
        if let Some(um) = self.micrometres(length) {
            add_to_bin(&mut self.line_bins, um);
        }
    }

    fn arc(&mut self, radius: f64) {
        if let Some(um) = self.micrometres(radius) {
            add_to_bin(&mut self.arc_bins, um);
        }
    }
}

/// A dimension kind's name as the model's JSON spells it.
fn kind_name(kind: Option<DimensionKind>) -> &'static str {
    match kind {
        None => "UNSTATED",
        Some(DimensionKind::Rotated) => "ROTATED",
        Some(DimensionKind::Aligned) => "ALIGNED",
        Some(DimensionKind::Angular2Line) => "ANGULAR2_LINE",
        Some(DimensionKind::Diameter) => "DIAMETER",
        Some(DimensionKind::Radius) => "RADIUS",
        Some(DimensionKind::Angular3Point) => "ANGULAR3_POINT",
        Some(DimensionKind::Ordinate) => "ORDINATE",
        Some(DimensionKind::ArcLength) => "ARC_LENGTH",
    }
}

/// The length bin of `um` micrometres (module doc).
pub(crate) fn bin(um: u64) -> usize {
    let mm = um / 1000;
    (u64::BITS - mm.leading_zeros()) as usize
}

fn add_to_bin(bins: &mut Vec<u64>, um: u64) {
    let k = bin(um);
    if bins.len() <= k {
        bins.resize(k + 1, 0);
    }
    bins[k] += 1;
}

fn p3(x: f64, y: f64, z: f64) -> Point3D {
    Point3D { x, y, z }
}

fn add(a: Point3D, b: Point3D) -> Point3D {
    p3(a.x + b.x, a.y + b.y, a.z + b.z)
}

fn sub(a: Point3D, b: Point3D) -> Point3D {
    p3(a.x - b.x, a.y - b.y, a.z - b.z)
}

fn scale(a: Point3D, k: f64) -> Point3D {
    p3(a.x * k, a.y * k, a.z * k)
}

fn norm(a: Point3D) -> f64 {
    (a.x * a.x + a.y * a.y + a.z * a.z).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_length_bin_is_the_power_of_two_of_its_whole_millimetres() {
        assert_eq!(bin(0), 0);
        assert_eq!(bin(999), 0);
        assert_eq!(bin(1000), 1);
        assert_eq!(bin(1999), 1);
        assert_eq!(bin(2000), 2);
        assert_eq!(bin(3999), 2);
        assert_eq!(bin(4000), 3);
        assert_eq!(bin(10_000), 4);
    }
}
