//! Plain 2D geometry on the model's points: distances a hit-test needs.
//! Everything is on `f64` as computed, with no rounding.

use uncad_model::{Point2D, Point3D};

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

/// Distance from `p` to a polyline through `vertices`, closed or open.
/// `None` for fewer than two vertices.
pub(crate) fn distance_to_polyline(p: Point2D, vertices: &[Point2D], closed: bool) -> Option<f64> {
    if vertices.len() < 2 {
        return None;
    }
    let mut best = f64::INFINITY;
    for pair in vertices.windows(2) {
        best = best.min(distance_to_segment(p, pair[0], pair[1]));
    }
    if closed {
        best = best.min(distance_to_segment(
            p,
            vertices[vertices.len() - 1],
            vertices[0],
        ));
    }
    Some(best)
}

/// Whether `p` is inside the polygon through `vertices` (even-odd rule). A
/// point on the boundary may fall either way; a hit-test reports such a
/// point as a boundary hit anyway.
pub(crate) fn polygon_contains(p: Point2D, vertices: &[Point2D]) -> bool {
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

/// Distance from `p` to the arc of a circle at `center` with `radius` from
/// `start` to `end` (radians, counter-clockwise, as the model stores them).
pub(crate) fn distance_to_arc(
    p: Point2D,
    center: Point2D,
    radius: f64,
    start: f64,
    end: f64,
) -> f64 {
    let d = distance(p, center);
    let angle = (p.y - center.y).atan2(p.x - center.x);
    if angle_within(angle, start, end) {
        return (d - radius).abs();
    }
    let at = |a: f64| Point2D {
        x: center.x + radius * a.cos(),
        y: center.y + radius * a.sin(),
    };
    distance(p, at(start)).min(distance(p, at(end)))
}

/// Whether `angle` lies on the counter-clockwise sweep from `start` to `end`.
fn angle_within(angle: f64, start: f64, end: f64) -> bool {
    let tau = std::f64::consts::TAU;
    let norm = |a: f64| a.rem_euclid(tau);
    let sweep = norm(end - start);
    let offset = norm(angle - start);
    if sweep == 0.0 {
        // A full circle, as some writers store it.
        return true;
    }
    offset <= sweep
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn a_closed_polyline_has_a_closing_edge_and_an_inside() {
        let square = [p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0), p(0.0, 10.0)];
        assert_eq!(distance_to_polyline(p(-1.0, 5.0), &square, true), Some(1.0));
        assert_eq!(
            distance_to_polyline(p(-1.0, 5.0), &square, false),
            Some(1.0f64.hypot(5.0))
        );
        assert!(polygon_contains(p(5.0, 5.0), &square));
        assert!(!polygon_contains(p(15.0, 5.0), &square));
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
