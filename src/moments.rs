//! Length-weighted moments of the outline a view draws: the straight sides
//! and arcs, summed in closed form so that the integers they are rounded to
//! are the same for the same drawing.
//!
//! A segment or an arc is weighted by its length: a point of it counts as much
//! as any other point of the same length of line. Only lines and arcs weigh;
//! a circle, an ellipse or a spline has no weight here, and the view's
//! `entities` and `circle_diameters_um` say they are there.

/// Sums over a set of segments and arcs, in millimetres, taken about a point
/// of their own (the `origin` they were added with) -- a set is moved to
/// another point with [`Moments::about`].
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Moments {
    /// The total length.
    pub w: f64,
    /// Integrals of x, y, x², y² and xy over the length.
    pub sx: f64,
    pub sy: f64,
    pub sxx: f64,
    pub syy: f64,
    pub sxy: f64,
}

impl Moments {
    /// Adds the straight segment from `p0` to `p1`, both relative to the
    /// point the moments are taken about.
    pub(crate) fn segment(&mut self, p0: [f64; 2], p1: [f64; 2]) {
        let length = (p1[0] - p0[0]).hypot(p1[1] - p0[1]);
        if !(length.is_finite() && length > 0.0) {
            return;
        }
        let ([x0, y0], [x1, y1]) = (p0, p1);
        self.w += length;
        self.sx += length * (x0 + x1) / 2.0;
        self.sy += length * (y0 + y1) / 2.0;
        self.sxx += length * (x0 * x0 + x0 * x1 + x1 * x1) / 3.0;
        self.syy += length * (y0 * y0 + y0 * y1 + y1 * y1) / 3.0;
        self.sxy += length * (2.0 * x0 * y0 + x0 * y1 + x1 * y0 + 2.0 * x1 * y1) / 6.0;
    }

    /// Adds the arc `c + u cos t + v sin t` for `t` from `start` through
    /// `sweep` (radians, signed), with `|u| = |v|` and `u` perpendicular to
    /// `v`: a circular arc in any placement, a mirrored one included. The
    /// points are relative to the point the moments are taken about.
    pub(crate) fn arc(&mut self, c: [f64; 2], u: [f64; 2], v: [f64; 2], start: f64, sweep: f64) {
        let radius = u[0].hypot(u[1]);
        if !(radius.is_finite() && radius > 0.0 && sweep.is_finite() && sweep != 0.0) {
            return;
        }
        let (t0, t1) = if sweep > 0.0 {
            (start, start + sweep)
        } else {
            (start + sweep, start)
        };
        let d = t1 - t0;
        // Integrals of cos, sin, cos², sin² and sin·cos over [t0, t1].
        let ic = t1.sin() - t0.sin();
        let is = t0.cos() - t1.cos();
        let double = (2.0 * t1).sin() - (2.0 * t0).sin();
        let icc = d / 2.0 + double / 4.0;
        let iss = d / 2.0 - double / 4.0;
        let isc = (t1.sin().powi(2) - t0.sin().powi(2)) / 2.0;
        // The integral over t of a·b for a = a0 + au cos + av sin, same for b.
        let product = |a: [f64; 3], b: [f64; 3]| {
            a[0] * b[0] * d
                + (a[0] * b[1] + a[1] * b[0]) * ic
                + (a[0] * b[2] + a[2] * b[0]) * is
                + a[1] * b[1] * icc
                + a[2] * b[2] * iss
                + (a[1] * b[2] + a[2] * b[1]) * isc
        };
        let x = [c[0], u[0], v[0]];
        let y = [c[1], u[1], v[1]];
        // The length element is radius dt.
        self.w += radius * d;
        self.sx += radius * (c[0] * d + u[0] * ic + v[0] * is);
        self.sy += radius * (c[1] * d + u[1] * ic + v[1] * is);
        self.sxx += radius * product(x, x);
        self.syy += radius * product(y, y);
        self.sxy += radius * product(x, y);
    }

    /// These sums taken about the point `shift` away from the one they were
    /// taken about: a point `p` of the outline is `p + shift` there.
    pub(crate) fn about(&self, shift: [f64; 2]) -> Moments {
        let [dx, dy] = shift;
        Moments {
            w: self.w,
            sx: self.sx + dx * self.w,
            sy: self.sy + dy * self.w,
            sxx: self.sxx + 2.0 * dx * self.sx + dx * dx * self.w,
            syy: self.syy + 2.0 * dy * self.sy + dy * dy * self.w,
            sxy: self.sxy + dx * self.sy + dy * self.sx + dx * dy * self.w,
        }
    }

    /// Adds `other`, taken about the same point.
    pub(crate) fn add(&mut self, other: &Moments) {
        self.w += other.w;
        self.sx += other.sx;
        self.sy += other.sy;
        self.sxx += other.sxx;
        self.syy += other.syy;
        self.sxy += other.sxy;
    }

    /// The second moments about the point these are taken about, per unit of
    /// length, over the size of a `width` by `height` box (millimetres):
    /// `[E[x²]/w², E[y²]/h², E[xy]/(w·h)]`, each times 10⁶ and rounded.
    /// `None` without length, or when the box has no width or no height.
    pub(crate) fn second(&self, width: f64, height: f64) -> Option<[i64; 3]> {
        if !(self.w > 0.0 && width > 0.0 && height > 0.0) {
            return None;
        }
        let scale = |value: f64, over: f64| (value / self.w / over * 1e6).round() as i64;
        Some([
            scale(self.sxx, width * width),
            scale(self.syy, height * height),
            scale(self.sxy, width * height),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same sums by summing `n` short chords of the curve.
    fn sampled(point: impl Fn(f64) -> [f64; 2], from: f64, to: f64, n: usize) -> Moments {
        let mut m = Moments::default();
        for i in 0..n {
            let (a, b) = (
                from + (to - from) * i as f64 / n as f64,
                from + (to - from) * (i + 1) as f64 / n as f64,
            );
            m.segment(point(a), point(b));
        }
        m
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-4 * (1.0 + a.abs().max(b.abs()))
    }

    fn same(a: &Moments, b: &Moments) -> bool {
        close(a.w, b.w)
            && close(a.sx, b.sx)
            && close(a.sy, b.sy)
            && close(a.sxx, b.sxx)
            && close(a.syy, b.syy)
            && close(a.sxy, b.sxy)
    }

    #[test]
    fn a_segment_is_a_line_of_points_weighted_by_length() {
        // The unit square's diagonal, from (0, 0) to (3, 4): length 5.
        let mut m = Moments::default();
        m.segment([0.0, 0.0], [3.0, 4.0]);
        let by_chords = sampled(|t| [3.0 * t, 4.0 * t], 0.0, 1.0, 4000);
        assert!(same(&m, &by_chords), "{m:?} {by_chords:?}");
        assert!(close(m.w, 5.0));
    }

    #[test]
    fn an_arc_in_closed_form_matches_its_chords() {
        let (c, u, v) = ([2.0, -1.0], [3.0, 0.0], [0.0, 3.0]);
        let point = |t: f64| {
            [
                c[0] + u[0] * t.cos() + v[0] * t.sin(),
                c[1] + u[1] * t.cos() + v[1] * t.sin(),
            ]
        };
        for (start, sweep) in [(0.3, 1.9), (2.0, -4.0), (-1.0, 6.0)] {
            let mut m = Moments::default();
            m.arc(c, u, v, start, sweep);
            let (lo, hi) = if sweep > 0.0 {
                (start, start + sweep)
            } else {
                (start + sweep, start)
            };
            let by_chords = sampled(point, lo, hi, 20_000);
            assert!(same(&m, &by_chords), "{start} {sweep}: {m:?} {by_chords:?}");
        }
    }

    #[test]
    fn a_mirrored_arc_weighs_like_the_arc_it_mirrors() {
        // u and v the other way round: the same circle run clockwise.
        let (c, u, v) = ([0.5, 0.5], [2.0, 0.0], [0.0, -2.0]);
        let point = |t: f64| {
            [
                c[0] + u[0] * t.cos() + v[0] * t.sin(),
                c[1] + u[1] * t.cos() + v[1] * t.sin(),
            ]
        };
        let mut m = Moments::default();
        m.arc(c, u, v, 0.4, 2.5);
        assert!(same(&m, &sampled(point, 0.4, 2.9, 20_000)));
    }

    #[test]
    fn moving_the_point_the_sums_are_taken_about_moves_them_exactly() {
        let mut at_origin = Moments::default();
        at_origin.segment([1.0, 2.0], [4.0, 6.0]);
        at_origin.arc([0.0, 1.0], [2.0, 0.0], [0.0, 2.0], 0.2, 1.1);
        let shift = [3.0, -5.0];
        let mut moved = Moments::default();
        moved.segment([4.0, -3.0], [7.0, 1.0]);
        moved.arc([3.0, -4.0], [2.0, 0.0], [0.0, 2.0], 0.2, 1.1);
        assert!(same(&at_origin.about(shift), &moved));
    }

    #[test]
    fn a_box_with_no_size_or_no_length_gives_nothing() {
        let mut m = Moments::default();
        assert_eq!(m.second(10.0, 10.0), None);
        m.segment([0.0, 0.0], [0.0, 10.0]);
        assert_eq!(m.second(0.0, 10.0), None);
        assert!(m.second(10.0, 10.0).is_some());
    }
}
