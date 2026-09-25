//! Where an ELLIPSE or a SPLINE is, for pointing at it: the curve, seen from
//! above and placed where it is drawn, as chords -- and how far the curve can
//! stray from those chords.
//!
//! The points on the curve are the model's ([`EllipseEntity::point_at`],
//! [`Nurbs`](uncad_model::Nurbs)); what is chosen here is how densely to
//! take them. The bound comes from the curve's second derivative: a chord
//! over a parameter step
//! `h` of a curve whose second derivative never exceeds `M` in length stays
//! within `M h^2 / 8` of the curve, and the curve within the same distance of
//! the chord. For an ellipse `M` is its longer semi-axis. For a polynomial
//! spline it is the longest control point of the curve's second derivative
//! over each piece (a spline lies in the hull of its control points, and so
//! does its derivative). Looking from above and placing the curve through a
//! block reference can stretch that by at most the placement's norm.
//!
//! A rational spline -- its weights differ -- has no such simple bound, and
//! a spline stored only by the points it passes through has no curve to
//! sample: the program that draws it fits one. Neither is guessed.

use uncad_model::model::{EllipseEntity, SplineEntity};
use uncad_model::{Affine2, Point2D, Point3D};

/// The distance the chords may stray from the curve, as a fraction of the
/// search tolerance: small enough that a distance is good to three digits of
/// the tolerance.
const BOUND_PER_TOLERANCE: f64 = 1e-3;

/// The most chords one ellipse, or one piece of a spline, is cut into.
const MAX_CHORDS: usize = 4096;

/// A curve as chords.
pub(crate) struct Chords {
    /// The chords' ends, in order, placed where the curve is drawn.
    pub points: Vec<Point2D>,
    /// How far the curve can be from the chords, and the chords from the
    /// curve, in the drawing's units.
    pub within: f64,
}

/// What this crate can say about where a curve is.
pub(crate) enum Curve {
    Chords(Chords),
    /// The file does not define the curve.
    Undefined,
    /// The curve is defined, but its distance from chords is not bounded.
    BoundUnknown,
}

/// The largest factor by which the linear part of `t` stretches a vector.
fn stretch(t: &Affine2) -> f64 {
    let half = (t.a * t.a + t.b * t.b + t.c * t.c + t.d * t.d) / 2.0;
    let det = t.determinant();
    (half + (half * half - det * det).max(0.0).sqrt()).sqrt()
}

fn length(v: Point3D) -> f64 {
    (v.x * v.x + v.y * v.y + v.z * v.z).sqrt()
}

/// How many chords of parameter length `h` / count keep a curve whose
/// second derivative is at most `m` within `target`, and the bound they
/// actually give.
fn chords_for(h: f64, m: f64, target: f64) -> (usize, f64) {
    let wanted = if m <= 0.0 {
        1.0
    } else if target > 0.0 {
        (h * (m / (8.0 * target)).sqrt()).ceil()
    } else {
        MAX_CHORDS as f64
    };
    let count = if wanted.is_finite() {
        (wanted as usize).clamp(1, MAX_CHORDS)
    } else {
        MAX_CHORDS
    };
    let step = h / count as f64;
    (count, m * step * step / 8.0)
}

/// An ELLIPSE placed through `t`, as chords good to `tolerance`.
pub(crate) fn ellipse(el: &EllipseEntity, t: &Affine2, tolerance: f64) -> Curve {
    let Some(minor) = el.minor_axis() else {
        return Curve::Undefined;
    };
    let stretch = stretch(t);
    let m = length(el.major_axis_endpoint).max(length(minor));
    let sweep = el.sweep();
    let (count, bound) = chords_for(sweep, m, tolerance * BOUND_PER_TOLERANCE / stretch);
    let points = (0..=count)
        .filter_map(|i| el.point_at(el.start_angle + sweep * (i as f64 / count as f64)))
        .map(|q| t.apply(Point2D { x: q.x, y: q.y }))
        .collect();
    Curve::Chords(Chords {
        points,
        within: bound * stretch,
    })
}

/// A SPLINE placed through `t`, as chords good to `tolerance`.
pub(crate) fn spline(s: &SplineEntity, t: &Affine2, tolerance: f64) -> Curve {
    let Some(curve) = s.nurbs() else {
        return Curve::Undefined;
    };
    if s.weights.windows(2).any(|w| w[0] != w[1]) {
        return Curve::BoundUnknown;
    }
    let p = s.degree as usize;
    let second = second_derivative(p, &s.knots, &s.control_points);
    let stretch = stretch(t);
    let target = tolerance * BOUND_PER_TOLERANCE / stretch;
    let mut points = Vec::new();
    let mut within: f64 = 0.0;
    let mut end = None;
    for k in p..s.control_points.len() {
        let (a, b) = (s.knots[k], s.knots[k + 1]);
        if a >= b {
            continue;
        }
        // The second derivative over this piece lies in the hull of its
        // control points k - p ..= k - 2 (none for a degree-one spline,
        // whose pieces are straight).
        let m = if p >= 2 {
            second[k - p..=k - 2]
                .iter()
                .map(|&v| length(v))
                .fold(0.0, f64::max)
        } else {
            0.0
        };
        let (count, bound) = chords_for(b - a, m, target);
        within = within.max(bound);
        for i in 0..count {
            let u = a + (b - a) * (i as f64 / count as f64);
            points.extend(curve.point_at(u));
        }
        end = Some(b);
    }
    points.extend(end.and_then(|b| curve.point_at(b)));
    Curve::Chords(Chords {
        points: points
            .into_iter()
            .map(|q| t.apply(Point2D { x: q.x, y: q.y }))
            .collect(),
        within: within * stretch,
    })
}

/// The control points of a polynomial spline's second derivative: those of
/// its first derivative differenced once more. A control point over a span
/// of zero length has a basis function that is zero everywhere, so it is
/// left at zero -- it shapes nothing.
fn second_derivative(p: usize, knots: &[f64], control: &[Point3D]) -> Vec<Point3D> {
    let diff = |from: &[Point3D], degree: usize, offset: usize| -> Vec<Point3D> {
        from.windows(2)
            .enumerate()
            .map(|(i, w)| {
                let span = knots[i + p + 1] - knots[i + offset];
                if span > 0.0 {
                    let f = degree as f64 / span;
                    Point3D {
                        x: (w[1].x - w[0].x) * f,
                        y: (w[1].y - w[0].y) * f,
                        z: (w[1].z - w[0].z) * f,
                    }
                } else {
                    Point3D {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                    }
                }
            })
            .collect()
    };
    if p < 2 {
        return Vec::new();
    }
    let first = diff(control, p, 1);
    diff(&first, p - 1, 2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uncad_model::model::{Confidence, EntityCommon, EntityId, EntityLinetype, Origin, Ref};

    fn common() -> EntityCommon {
        EntityCommon {
            id: EntityId::new(1),
            origin: Origin::Vector,
            confidence: Confidence::High,
            source_handle: Ref::Absent,
            layer: Ref::Absent,
            color_index: 7,
            true_color: None,
            invisible: false,
            linetype: EntityLinetype::ByLayer,
            linetype_scale: 1.0,
            lineweight: Some(-1),
            transparency: Some(0),
        }
    }

    fn p(x: f64, y: f64) -> Point3D {
        Point3D { x, y, z: 0.0 }
    }

    fn spline(degree: u32, control: &[Point3D], knots: &[f64], weights: &[f64]) -> SplineEntity {
        SplineEntity {
            common: common(),
            degree,
            closed: Some(false),
            periodic: Some(false),
            knots: knots.to_vec(),
            weights: weights.to_vec(),
            fit_points: Vec::new(),
            control_points: control.to_vec(),
            start_tangent: None,
            end_tangent: None,
        }
    }

    #[test]
    fn a_placement_stretches_by_its_longest_axis() {
        let t = Affine2 {
            a: 3.0,
            b: 0.0,
            c: 0.0,
            d: 2.0,
            e: 5.0,
            f: 5.0,
        };
        assert!((stretch(&t) - 3.0).abs() < 1e-12);
        assert!((stretch(&Affine2::IDENTITY) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn a_parabola_has_its_second_derivative_everywhere() {
        // A clamped quadratic Bezier over [0, 1]: its second derivative is
        // the constant 2 (P0 - 2 P1 + P2).
        let control = [p(0.0, 0.0), p(1.0, 2.0), p(2.0, 0.0)];
        let second = second_derivative(2, &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0], &control);
        assert_eq!(second.len(), 1);
        assert!((length(second[0]) - 8.0).abs() < 1e-12, "{:?}", second[0]);
    }

    #[test]
    fn the_bound_holds_on_a_parabola() {
        // y = 4 x (1 - x) for x in [0, 1], a quadratic Bezier; its chords'
        // true deviation is measurable in closed form.
        let s = spline(
            2,
            &[p(0.0, 0.0), p(0.5, 2.0), p(1.0, 0.0)],
            &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            &[],
        );
        let Curve::Chords(c) = super::spline(&s, &Affine2::IDENTITY, 0.1) else {
            panic!("a polynomial spline is chords");
        };
        assert!(c.within <= 0.1 * BOUND_PER_TOLERANCE);
        // Every chord's midpoint is within the bound of the curve directly
        // above or below it (the curve is a function of x).
        for w in c.points.windows(2) {
            let mx = (w[0].x + w[1].x) / 2.0;
            let my = (w[0].y + w[1].y) / 2.0;
            let on = 4.0 * mx * (1.0 - mx);
            assert!((on - my).abs() <= c.within + 1e-15, "{mx} {my} {on}");
        }
    }

    #[test]
    fn a_rational_spline_is_not_bounded() {
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let s = spline(
            2,
            &[p(1.0, 0.0), p(1.0, 1.0), p(0.0, 1.0)],
            &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            &[1.0, h, 1.0],
        );
        assert!(matches!(
            super::spline(&s, &Affine2::IDENTITY, 0.1),
            Curve::BoundUnknown
        ));
        // Equal weights are no weights.
        let s = spline(
            2,
            &[p(1.0, 0.0), p(1.0, 1.0), p(0.0, 1.0)],
            &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            &[2.0, 2.0, 2.0],
        );
        assert!(matches!(
            super::spline(&s, &Affine2::IDENTITY, 0.1),
            Curve::Chords(_)
        ));
    }

    #[test]
    fn a_spline_stored_by_its_fit_points_is_not_defined() {
        let mut s = spline(3, &[], &[], &[]);
        s.fit_points = vec![p(0.0, 0.0), p(1.0, 1.0), p(2.0, 0.0)];
        assert!(matches!(
            super::spline(&s, &Affine2::IDENTITY, 0.1),
            Curve::Undefined
        ));
    }

    #[test]
    fn an_ellipse_is_cut_finely_enough_for_the_tolerance() {
        let el = EllipseEntity {
            common: common(),
            center: p(0.0, 0.0),
            major_axis_endpoint: p(10.0, 0.0),
            axis_ratio: 0.5,
            start_angle: 0.0,
            end_angle: 0.0,
            extrusion: Point3D {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
        };
        let Curve::Chords(c) = ellipse(&el, &Affine2::IDENTITY, 0.01) else {
            panic!("an ellipse with a plane is chords");
        };
        assert!(c.within <= 0.01 * BOUND_PER_TOLERANCE);
        // The whole ellipse: it closes on itself.
        let (first, last) = (c.points[0], *c.points.last().unwrap());
        assert!((first.x - last.x).abs() < 1e-9 && (first.y - last.y).abs() < 1e-9);
        // Every chord end is on the ellipse (x/10)^2 + (y/5)^2 = 1.
        for q in &c.points {
            let r = (q.x / 10.0).powi(2) + (q.y / 5.0).powi(2);
            assert!((r - 1.0).abs() < 1e-12, "{q:?}");
        }
    }
}
