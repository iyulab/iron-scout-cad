//! Coincident ends: which other ends of the model space's lines, arcs and
//! polylines sit where a given end does.
//!
//! Moving one end of a drawing's outline leaves the neighbours that met it
//! behind unless they move too. This reads which ends those are -- it moves
//! nothing, and it does not solve for anything: the edit itself is the
//! editor's `set` once per end, and whether lines that were parallel or at
//! right angles stay so is not this reading's to say.
//!
//! An end is *at* another when the two are within the join gap the caller
//! gives (the same gap the shape signature and [`crate::contours`] join
//! pieces at). That is a distance from the asked end, not a chain: an end
//! that is within the gap of a neighbour but not of the asked end is not
//! listed. An end that cannot follow a straight move -- the end of an arc, or
//! a polyline vertex that a curved segment leaves or reaches -- is listed
//! with the reason, so the caller sees what a straight move would break.

use crate::hit_test::spaces;
use crate::outline::is_sane;

/// The slack `contours` joins ends with, so the two readings agree at the gap.
const EPS_MM: f64 = 1e-9;
use crate::relate::Anchor;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uncad_model::model::{Entity, EntityId, Point2D, PolylineVertex};
use uncad_model::{BulgeArc, CadDatabase, Ocs};

/// Why an end cannot follow a straight move of its neighbour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[non_exhaustive]
pub enum NoFollow {
    /// The end belongs to an arc, or to a polyline vertex that a curved
    /// segment leaves or reaches: moving it changes the arc's radius or
    /// centre, which is a constraint to solve and not a move.
    ArcEnd,
}

/// One end of the model space's outline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CoincidentEnd {
    pub entity: EntityId,
    pub anchor: Anchor,
    /// Where the editor's `set` writes this end: `start_point`, `end_point`
    /// or `vertices[i].point`. An arc's end has no field of its own to write
    /// (the angle gives it), so it has none.
    pub path: Option<String>,
    /// The end in the world's plan, in drawing units.
    pub at: Point2D,
    /// The end in the entity's own coordinates -- the value `set` takes. It
    /// differs from `at` only for a polyline drawn on a mirrored plane.
    pub local: Point2D,
    /// Whether a straight move of the asked end can carry this one along.
    pub can_follow: bool,
    pub why_not: Option<NoFollow>,
}

/// The ends at an asked end.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CoincidentEnds {
    /// The asked end itself.
    pub end: CoincidentEnd,
    /// The other ends within the join gap of it, by entity and then anchor.
    pub ends: Vec<CoincidentEnd>,
    /// What the model space holds that this did not read, by what it was left
    /// out for: ends of such entities may sit at the asked end and are not
    /// listed. `UNPLACEABLE` counts ends (not entities): a number that is not
    /// finite or is too large, or an arc whose radius is not positive.
    pub not_read: BTreeMap<String, u32>,
}

/// Why the ends could not be told.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[non_exhaustive]
pub enum Refusal {
    /// The drawing states no unit, so a gap in micrometres means nothing.
    NoUnit,
    /// The model space has no such end: no such entity, an anchor the entity
    /// does not have, or an entity this does not read.
    NoSuchEnd,
}

/// The ends of the model space within `join_gap_um` of the end of `entity`
/// named by `anchor` (a LINE or ARC's `Start` / `End`, a polyline's
/// `Vertex(i)`).
///
/// Pass [`ENDPOINT_GAP_UM`](crate::ENDPOINT_GAP_UM) to list the ends the
/// shape signature and the contour totals treat as touching, so that what is
/// listed here and what those count as joined agree.
pub fn coincident_ends(
    db: &CadDatabase,
    entity: EntityId,
    anchor: Anchor,
    join_gap_um: u64,
) -> Result<CoincidentEnds, Refusal> {
    let k = db
        .header
        .units()
        .and_then(|u| u.to_mm)
        .ok_or(Refusal::NoUnit)?;
    let spaces = spaces(db);
    let gap_mm = join_gap_um as f64 / 1000.0;

    let mut all: Vec<CoincidentEnd> = Vec::new();
    // An end enters the listing only when it can be placed: a number that is
    // not finite or is too large is a sentinel or an error, and is counted.
    let placed = |e: CoincidentEnd, all: &mut Vec<CoincidentEnd>, skipped: &mut u32| {
        if is_sane([e.at.x * k, e.at.y * k]) {
            all.push(e);
        } else {
            *skipped += 1;
        }
    };
    let mut unplaceable: u32 = 0;
    let mut not_read: BTreeMap<String, u32> = BTreeMap::new();
    let mut skip = |name: &str| *not_read.entry(name.to_string()).or_insert(0) += 1;

    for e in &db.entities {
        let id = e.common().id;
        if spaces
            .get(&id)
            .is_some_and(|s| !s.eq_ignore_ascii_case("*Model_Space"))
        {
            continue;
        }
        match e {
            Entity::Line(l) => {
                for (anchor, path, p) in [
                    (Anchor::Start, "start_point", l.start_point),
                    (Anchor::End, "end_point", l.end_point),
                ] {
                    let at = Point2D { x: p.x, y: p.y };
                    placed(
                        end(id, anchor, Some(path.to_string()), at, at, None),
                        &mut all,
                        &mut unplaceable,
                    );
                }
            }
            Entity::Arc(a) => match (Ocs::of(a.extrusion).and_then(|o| o.flat_map()), a.sweep()) {
                (Some(_), Some(_)) if !(a.radius.is_finite() && a.radius > 0.0) => {
                    // A radius that is not a positive number places no end.
                    unplaceable += 2;
                }
                (Some(plan), Some(sweep)) => {
                    let at = |t: f64| {
                        plan.apply(Point2D {
                            x: a.center.x + a.radius * t.cos(),
                            y: a.center.y + a.radius * t.sin(),
                        })
                    };
                    for (anchor, p) in [
                        (Anchor::Start, at(a.start_angle)),
                        (Anchor::End, at(a.start_angle + sweep)),
                    ] {
                        // Moving an arc's end is not a straight move.
                        placed(
                            end(id, anchor, None, p, p, Some(NoFollow::ArcEnd)),
                            &mut all,
                            &mut unplaceable,
                        );
                    }
                }
                (None, _) => skip("TILTED_PLANE"),
                (_, None) => skip("ARC_SWEEP_UNDEFINED"),
            },
            Entity::LwPolyline(p) | Entity::Polyline2D(p) => {
                match Ocs::of(p.extrusion).and_then(|o| o.flat_map()) {
                    Some(plan) if p.vertices.len() >= 2 => {
                        for (i, v) in p.vertices.iter().enumerate() {
                            let why = (!vertex_is_straight(&p.vertices, p.closed, i))
                                .then_some(NoFollow::ArcEnd);
                            placed(
                                end(
                                    id,
                                    Anchor::Vertex(i),
                                    Some(format!("vertices[{i}].point")),
                                    plan.apply(v.point),
                                    v.point,
                                    why,
                                ),
                                &mut all,
                                &mut unplaceable,
                            );
                        }
                    }
                    Some(_) => {}
                    None => skip("TILTED_PLANE"),
                }
            }
            Entity::Polyline3D(_) => skip("POLYLINE_3D"),
            Entity::Insert(_) => skip("INSERT"),
            Entity::Ellipse(_) => skip("ELLIPSE"),
            Entity::Spline(_) => skip("SPLINE"),
            _ => {}
        }
    }

    if unplaceable > 0 {
        not_read.insert("UNPLACEABLE".to_string(), unplaceable);
    }
    let asked = all
        .iter()
        .position(|e| e.entity == entity && e.anchor == anchor)
        .ok_or(Refusal::NoSuchEnd)?;
    let asked_end = all.swap_remove(asked);
    let near = |p: Point2D| {
        let (dx, dy) = ((p.x - asked_end.at.x) * k, (p.y - asked_end.at.y) * k);
        dx.hypot(dy) <= gap_mm + EPS_MM
    };
    let mut ends: Vec<CoincidentEnd> = all.into_iter().filter(|e| near(e.at)).collect();
    ends.sort_by_key(|e| (e.entity, e.anchor));
    Ok(CoincidentEnds {
        end: asked_end,
        ends,
        not_read,
    })
}

fn end(
    entity: EntityId,
    anchor: Anchor,
    path: Option<String>,
    at: Point2D,
    local: Point2D,
    why_not: Option<NoFollow>,
) -> CoincidentEnd {
    CoincidentEnd {
        entity,
        anchor,
        path,
        at,
        local,
        can_follow: why_not.is_none(),
        why_not,
    }
}

/// Whether the segments that leave and reach vertex `i` are both straight.
fn vertex_is_straight(vertices: &[PolylineVertex], closed: bool, i: usize) -> bool {
    let n = vertices.len();
    let curved = |from: usize, to: usize| {
        BulgeArc::between(
            vertices[from].point,
            vertices[to].point,
            vertices[from].bulge,
        )
        .is_some()
    };
    let leaves = i + 1 < n || closed;
    let reaches = i > 0 || closed;
    !(leaves && curved(i, (i + 1) % n)) && !(reaches && curved((i + n - 1) % n, i))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A drawing in the golden drawing's frame (its header, its units) with
    /// the entities given, each a JSON object that fills in a template's
    /// `common`.
    fn drawing(entities: Vec<serde_json::Value>) -> CadDatabase {
        let mut v: serde_json::Value =
            serde_json::from_str(include_str!("../tests/golden/g26.expected.json")).unwrap();
        let common = v["entities"][0]["common"].clone();
        let list: Vec<serde_json::Value> = entities
            .into_iter()
            .enumerate()
            .map(|(i, mut e)| {
                let mut c = common.clone();
                c["id"] = (2000 + i as u64).to_string().into();
                e["common"] = c;
                e
            })
            .collect();
        v["entities"] = list.into();
        serde_json::from_value(v).unwrap()
    }

    fn pt(x: f64, y: f64) -> serde_json::Value {
        serde_json::json!({"x": x, "y": y, "z": 0.0})
    }

    fn line(a: (f64, f64), b: (f64, f64)) -> serde_json::Value {
        serde_json::json!({"type": "LINE", "start_point": pt(a.0, a.1), "end_point": pt(b.0, b.1)})
    }

    fn id(i: u64) -> EntityId {
        EntityId::new(2000 + i)
    }

    #[test]
    fn lines_that_meet_within_the_gap_are_listed_by_entity_and_field() {
        let db = drawing(vec![
            line((0.0, 0.0), (10.0, 0.0)),
            line((10.0, 0.0), (10.0, 10.0)),
            line((10.0, 0.05), (20.0, 5.0)),
            line((50.0, 50.0), (60.0, 60.0)),
        ]);
        let got = coincident_ends(&db, id(0), Anchor::End, 100).unwrap();
        assert_eq!(got.end.path.as_deref(), Some("end_point"));
        let listed: Vec<_> = got
            .ends
            .iter()
            .map(|e| (e.entity, e.anchor, e.path.as_deref(), e.can_follow))
            .collect();
        assert_eq!(
            listed,
            vec![
                (id(1), Anchor::Start, Some("start_point"), true),
                (id(2), Anchor::Start, Some("start_point"), true),
            ]
        );
        // Tighter than the 0.05 mm offset, only the exact neighbour is left.
        let got = coincident_ends(&db, id(0), Anchor::End, 10).unwrap();
        assert_eq!(got.ends.len(), 1);
        assert_eq!(got.ends[0].entity, id(1));
    }

    #[test]
    fn the_end_of_an_arc_is_listed_and_says_it_cannot_follow() {
        let db = drawing(vec![
            line((0.0, 0.0), (15.0, 0.0)),
            serde_json::json!({"type": "ARC", "center": pt(10.0, 0.0), "radius": 5.0,
                               "start_angle": 0.0, "end_angle": std::f64::consts::FRAC_PI_2,
                               "extrusion": {"x": 0.0, "y": 0.0, "z": 1.0}}),
        ]);
        let got = coincident_ends(&db, id(0), Anchor::End, 100).unwrap();
        assert_eq!(got.ends.len(), 1);
        assert_eq!(got.ends[0].anchor, Anchor::Start);
        assert!(!got.ends[0].can_follow);
        assert_eq!(got.ends[0].why_not, Some(NoFollow::ArcEnd));
        assert_eq!(got.ends[0].path, None);
    }

    #[test]
    fn a_polyline_vertex_next_to_a_curved_segment_cannot_follow() {
        let vertex = |x: f64, y: f64, bulge: f64| serde_json::json!({"point": {"x": x, "y": y}, "bulge": bulge});
        let db = drawing(vec![
            line((0.0, 0.0), (10.0, 0.0)),
            serde_json::json!({"type": "LWPOLYLINE", "closed": false,
                "extrusion": {"x": 0.0, "y": 0.0, "z": 1.0},
                "vertices": [vertex(10.0, 0.0, 0.0), vertex(20.0, 0.0, 0.5), vertex(30.0, 10.0, 0.0)]}),
        ]);
        let got = coincident_ends(&db, id(0), Anchor::End, 100).unwrap();
        assert_eq!(got.ends.len(), 1);
        assert_eq!(got.ends[0].anchor, Anchor::Vertex(0));
        assert_eq!(got.ends[0].path.as_deref(), Some("vertices[0].point"));
        // Vertex 0 leaves a straight segment: it follows.
        assert!(got.ends[0].can_follow);
        // Vertex 1 reaches... and leaves a curved one: the polyline's own ask.
        let got = coincident_ends(&db, id(1), Anchor::Vertex(1), 100).unwrap();
        assert!(!got.end.can_follow);
        assert_eq!(got.end.why_not, Some(NoFollow::ArcEnd));
    }

    #[test]
    fn an_end_the_drawing_does_not_have_is_refused() {
        let db = drawing(vec![line((0.0, 0.0), (10.0, 0.0))]);
        assert_eq!(
            coincident_ends(&db, id(9), Anchor::End, 100).unwrap_err(),
            Refusal::NoSuchEnd
        );
        assert_eq!(
            coincident_ends(&db, id(0), Anchor::Vertex(0), 100).unwrap_err(),
            Refusal::NoSuchEnd
        );
    }

    #[test]
    fn every_listed_neighbour_is_in_the_asked_ends_group_of_contours() {
        // The two readings join at the same gap, so what this lists is a
        // subset of the group `contours` tells for the same entity.
        let db = drawing(vec![
            line((0.0, 0.0), (10.0, 0.0)),
            line((10.0, 0.0), (10.0, 10.0)),
            line((10.0, 0.05), (20.0, 5.0)),
            line((50.0, 50.0), (60.0, 60.0)),
        ]);
        let groups = crate::contours(&db, 100).unwrap();
        for asked in 0..3 {
            for anchor in [Anchor::Start, Anchor::End] {
                let got = coincident_ends(&db, id(asked), anchor, 100).unwrap();
                let group = groups
                    .components
                    .iter()
                    .find(|c| c.entities.contains(&id(asked)))
                    .unwrap();
                for e in &got.ends {
                    assert!(group.entities.contains(&e.entity), "{asked} {anchor:?}");
                }
            }
        }
    }

    #[test]
    fn an_arc_with_no_radius_places_no_end_and_is_counted() {
        let db = drawing(vec![
            line((0.0, 0.0), (10.0, 0.0)),
            serde_json::json!({"type": "ARC", "center": pt(10.0, 0.0), "radius": 0.0,
                               "start_angle": 0.0, "end_angle": 1.0,
                               "extrusion": {"x": 0.0, "y": 0.0, "z": 1.0}}),
        ]);
        let got = coincident_ends(&db, id(0), Anchor::End, 100).unwrap();
        assert!(got.ends.is_empty(), "{got:?}");
        assert_eq!(got.not_read.get("UNPLACEABLE"), Some(&2));
    }

    #[test]
    fn an_end_exactly_at_the_gap_is_listed_as_contours_joins_it() {
        // 0.05 mm is 10.05 - 10.0 only up to rounding; the gap is 50 um.
        let db = drawing(vec![
            line((0.0, 0.0), (10.0, 0.0)),
            line((10.05, 0.0), (20.0, 0.0)),
        ]);
        let got = coincident_ends(&db, id(0), Anchor::End, 50).unwrap();
        assert_eq!(got.ends.len(), 1, "{got:?}");
    }

    #[test]
    fn an_end_too_large_to_place_is_counted_and_an_asked_one_is_refused() {
        let db = drawing(vec![
            line((0.0, 0.0), (10.0, 0.0)),
            line((1.0e300, 0.0), (1.0e300, 5.0)),
        ]);
        let got = coincident_ends(&db, id(0), Anchor::End, 100).unwrap();
        assert!(got.ends.is_empty());
        assert_eq!(got.not_read.get("UNPLACEABLE"), Some(&2));
        assert_eq!(
            coincident_ends(&db, id(1), Anchor::Start, 100).unwrap_err(),
            Refusal::NoSuchEnd
        );
    }
}
