//! Plain 2D geometry on the model's points: distances a hit-test needs.
//! Everything is on `f64` as computed, with no rounding.

use uncad_model::bulge::Segment;
use uncad_model::{Affine2, BulgeArc, Ocs, Point2D, Point3D};

/// The coordinate system of an entity written in its own plane, when that
/// plane is parallel to the world's -- facing up, or down as a mirror copy's
/// does -- so that what is drawn there is seen from above undistorted.
/// `None` for a tilted plane, or an extrusion that names none.
pub(crate) fn flat_plane(extrusion: Point3D) -> Option<Ocs> {
    Ocs::of(extrusion).filter(|o| o.is_flat())
}

/// A point of `plane` taken to the world, seen from above.
pub(crate) fn world_xy(plane: Ocs, p: Point3D) -> Point2D {
    xy(plane.to_world(p))
}

/// The world direction (radians) of the direction `angle` in `plane`.
pub(crate) fn world_angle(plane: Ocs, angle: f64) -> f64 {
    let d = world_xy(
        plane,
        Point3D {
            x: angle.cos(),
            y: angle.sin(),
            z: 0.0,
        },
    );
    d.y.atan2(d.x)
}

/// `t` preceded by the map from the plane `extrusion` names to the world's
/// XY -- `None` when that plane is tilted out of the world's (or names no
/// plane), where no 2D map places a point of it exactly.
pub(crate) fn in_plane(extrusion: Point3D, t: &Affine2) -> Option<Affine2> {
    Some(Ocs::of(extrusion)?.flat_map()?.then(t))
}

pub(crate) fn xy(p: Point3D) -> Point2D {
    Point2D { x: p.x, y: p.y }
}

pub(crate) fn distance(a: Point2D, b: Point2D) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

/// Distance from `p` to the segment `a`-`b`.
pub(crate) fn distance_to_segment(p: Point2D, a: Point2D, b: Point2D) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    if len2 == 0.0 {
        return distance(p, a);
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0);
    distance(
        p,
        Point2D {
            x: a.x + t * dx,
            y: a.y + t * dy,
        },
    )
}

/// Distance from `p` to a polyline's segments, each straight or the arc
/// its bulge describes. `None` when there are no segments (fewer than two
/// vertices).
pub(crate) fn distance_to_segments(p: Point2D, segments: &[Segment]) -> Option<f64> {
    segments
        .iter()
        .map(|s| match &s.arc {
            Some(arc) => distance_to_bulge_arc(p, s.from, s.to, arc),
            None => distance_to_segment(p, s.from, s.to),
        })
        .reduce(f64::min)
}

/// Distance from `p` to the arc of a bulged segment from `from` to `to`.
fn distance_to_bulge_arc(p: Point2D, from: Point2D, to: Point2D, arc: &BulgeArc) -> f64 {
    let angle = (p.y - arc.center.y).atan2(p.x - arc.center.x);
    if arc.contains_angle(angle) {
        return (distance(p, arc.center) - arc.radius).abs();
    }
    distance(p, from).min(distance(p, to))
}

/// Whether `p` is inside the closed shape a polyline's segments bound
/// (even-odd rule), arcs included: the polygon through the segments' ends,
/// with each arc's cap -- the region between its chord and the arc -- added
/// or cut away. A point on the boundary may fall either way; a hit-test
/// reports such a point as a boundary hit anyway.
pub(crate) fn shape_contains(p: Point2D, segments: &[Segment]) -> bool {
    let corners: Vec<Point2D> = segments.iter().map(|s| s.from).collect();
    let mut inside = polygon_contains(p, &corners);
    for s in segments {
        if let Some(arc) = &s.arc {
            if in_cap(p, s.from, s.to, arc) {
                inside = !inside;
            }
        }
    }
    inside
}

/// Whether `p` is strictly inside the cap of an arc: within its circle and
/// on the arc's side of its chord. A cap is the minor or the major part of
/// the disk as the arc is shorter or longer than a half turn.
fn in_cap(p: Point2D, from: Point2D, to: Point2D, arc: &BulgeArc) -> bool {
    let side = |q: Point2D| (to.x - from.x) * (q.y - from.y) - (to.y - from.y) * (q.x - from.x);
    let middle = arc.at(arc.start_angle + arc.sweep / 2.0);
    distance(p, arc.center) < arc.radius && side(p) * side(middle) > 0.0
}

/// Whether `p` is inside the polygon through `vertices` (even-odd rule). A
/// point on the boundary may fall either way; a hit-test reports such a
/// point as a boundary hit anyway.
fn polygon_contains(p: Point2D, vertices: &[Point2D]) -> bool {
    if vertices.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = vertices.len() - 1;
    for i in 0..vertices.len() {
        let (a, b) = (vertices[i], vertices[j]);
        if (a.y > p.y) != (b.y > p.y) {
            let x = a.x + (p.y - a.y) * (b.x - a.x) / (b.y - a.y);
            if p.x < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// Distance from `p` to the arc of a circle at `center` with `radius` that
/// runs counter-clockwise from `start` by `sweep` (radians, within one turn
/// -- see [`ArcEntity::sweep`](uncad_model::model::ArcEntity::sweep)).
pub(crate) fn distance_to_arc(
    p: Point2D,
    center: Point2D,
    radius: f64,
    start: f64,
    sweep: f64,
) -> f64 {
    let d = distance(p, center);
    let angle = (p.y - center.y).atan2(p.x - center.x);
    if (angle - start).rem_euclid(std::f64::consts::TAU) <= sweep {
        return (d - radius).abs();
    }
    let end = start + sweep;
    let at = |a: f64| Point2D {
        x: center.x + radius * a.cos(),
        y: center.y + radius * a.sin(),
    };
    distance(p, at(start)).min(distance(p, at(end)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use uncad_model::PolylineVertex;

    fn p(x: f64, y: f64) -> Point2D {
        Point2D { x, y }
    }

    #[test]
    fn segment_distance_clamps_to_the_endpoints() {
        assert_eq!(
            distance_to_segment(p(5.0, 3.0), p(0.0, 0.0), p(10.0, 0.0)),
            3.0
        );
        assert_eq!(
            distance_to_segment(p(-4.0, 0.0), p(0.0, 0.0), p(10.0, 0.0)),
            4.0
        );
        assert_eq!(
            distance_to_segment(p(1.0, 1.0), p(2.0, 2.0), p(2.0, 2.0)),
            2f64.sqrt()
        );
    }

    fn segs(vertices: &[(f64, f64, f64)], closed: bool) -> Vec<Segment> {
        let v: Vec<PolylineVertex> = vertices
            .iter()
            .map(|&(x, y, bulge)| PolylineVertex {
                point: p(x, y),
                bulge,
                ..PolylineVertex::default()
            })
            .collect();
        uncad_model::bulge::segments(&v, closed).collect()
    }

    #[test]
    fn a_closed_polyline_has_a_closing_edge_and_an_inside() {
        let square = [
            (0.0, 0.0, 0.0),
            (10.0, 0.0, 0.0),
            (10.0, 10.0, 0.0),
            (0.0, 10.0, 0.0),
        ];
        let closed = segs(&square, true);
        assert_eq!(distance_to_segments(p(-1.0, 5.0), &closed), Some(1.0));
        assert_eq!(
            distance_to_segments(p(-1.0, 5.0), &segs(&square, false)),
            Some(1.0f64.hypot(5.0))
        );
        assert!(shape_contains(p(5.0, 5.0), &closed));
        assert!(!shape_contains(p(15.0, 5.0), &closed));
        assert_eq!(
            distance_to_segments(p(0.0, 0.0), &segs(&square[..1], true)),
            None
        );
    }

    #[test]
    fn a_bulged_segment_is_measured_along_its_arc_not_its_chord() {
        // (0,0) -> (2,0) with bulge 1: the half circle below, through (1,-1).
        let half = segs(&[(0.0, 0.0, 1.0), (2.0, 0.0, 0.0)], false);
        let d = distance_to_segments(p(1.0, -1.0), &half).unwrap();
        assert!(d.abs() < 1e-12, "{d}");
        // Straight above the chord the nearest arc point is an end.
        let d = distance_to_segments(p(1.0, 1.0), &half).unwrap();
        assert!((d - 2f64.sqrt()).abs() < 1e-12, "{d}");
    }

    #[test]
    fn a_circle_drawn_as_two_bulged_segments_encloses_its_middle() {
        // Two half circles, (0,0) -> (2,0) -> back: a full circle of radius 1
        // about (1,0), whose corner polygon has no area at all.
        let circle = segs(&[(0.0, 0.0, 1.0), (2.0, 0.0, 1.0)], true);
        assert!(shape_contains(p(1.0, 0.5), &circle));
        assert!(shape_contains(p(1.0, -0.5), &circle));
        assert!(!shape_contains(p(1.0, 1.5), &circle));
    }

    #[test]
    fn a_bulge_that_bows_inward_cuts_its_cap_out_of_the_polygon() {
        // A 10 x 10 square whose bottom edge bows up into it (bulge -1 from
        // (0,0) to (10,0) runs clockwise, above the chord).
        let bitten = segs(
            &[
                (0.0, 0.0, -1.0),
                (10.0, 0.0, 0.0),
                (10.0, 10.0, 0.0),
                (0.0, 10.0, 0.0),
            ],
            true,
        );
        assert!(!shape_contains(p(5.0, 2.0), &bitten), "inside the bite");
        assert!(shape_contains(p(5.0, 8.0), &bitten));
    }

    #[test]
    fn an_arc_is_hit_on_its_sweep_and_at_its_ends_otherwise() {
        // A quarter arc from 0 to 90 degrees, radius 10 at the origin.
        let q = std::f64::consts::FRAC_PI_2;
        assert!((distance_to_arc(p(0.0, 12.0), p(0.0, 0.0), 10.0, 0.0, q) - 2.0).abs() < 1e-12);
        // Opposite side: the nearest arc point is an end point.
        let d = distance_to_arc(p(-10.0, 0.0), p(0.0, 0.0), 10.0, 0.0, q);
        assert!((d - 10.0 * 2f64.sqrt()).abs() < 1e-12, "{d}");
    }
}
