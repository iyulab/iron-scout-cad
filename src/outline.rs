//! The pieces an entity draws in the world's plan: the one place that turns a
//! line, a circle, an arc or a polyline into straight and curved pieces.
//!
//! The shape signature (through blocks and their placements) and the closure
//! of outlines (the model space as it stands) both start from these pieces.
//! Each used to read the entities itself, and the second copy of every guard
//! -- a plane that is tilted, an arc with no sweep, a radius that is not a
//! number -- was a place for the two to disagree.

use crate::extent::linear;
use crate::geometry::{in_plane, xy};
use uncad_model::bulge;
use uncad_model::model::{Entity, Point2D};
use uncad_model::Affine2;

/// Coordinates, in millimetres, past which a piece is not placed: a number
/// that large is a sentinel or an error, and a cell index made from it would
/// not fit.
pub(crate) const MAX_COORD_MM: f64 = 1e12;

/// Whether a point (millimetres) is one a piece may be placed at.
pub(crate) fn is_sane(p: [f64; 2]) -> bool {
    p.iter().all(|c| c.is_finite() && c.abs() < MAX_COORD_MM)
}

/// A piece's end: where it is and the direction leading from it into the
/// piece, as a unit vector.
#[derive(Debug, Clone, Copy)]
pub(crate) struct End {
    pub at: [f64; 2],
    pub inward: [f64; 2],
}

fn unit(v: [f64; 2]) -> Option<[f64; 2]> {
    let n = v[0].hypot(v[1]);
    (n.is_finite() && n > 0.0).then(|| [v[0] / n, v[1] / n])
}

/// One piece of an outline, in drawing units, in the world's plan.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Part {
    Line {
        from: Point2D,
        to: Point2D,
    },
    /// The arc `centre + u cos t + v sin t` for `t` from `start` through
    /// `sweep` (`u` and `v` are the images of the radius along x and y, so a
    /// mirror shows as a sweep read the other way). `radius` is the radius as
    /// the entity states it, before the placement.
    Arc {
        centre: Point2D,
        u: Point2D,
        v: Point2D,
        start: f64,
        sweep: f64,
        radius: f64,
    },
    /// A full circle: `centre + u cos t + v sin t` for all `t`.
    Circle {
        centre: Point2D,
        u: Point2D,
        v: Point2D,
        radius: f64,
    },
}

/// Why an entity gives no pieces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Why {
    /// Its plane is tilted out of the world's, or names none: no 2D map
    /// places a point of it exactly.
    Tilted,
    /// An arc whose angles are equal has no ends to join and no sweep.
    SweepUndefined,
    /// A radius that is not a positive number.
    BadRadius,
}

/// The pieces `e` draws when the drawing's plan is taken to the world's by `t`
/// (the identity for the model space itself, a block's placement inside an
/// insert). Entities that draw no piece of this kind give `Ok` of nothing;
/// what cannot be placed says why.
pub(crate) fn parts(e: &Entity, t: &Affine2) -> Result<Vec<Part>, Why> {
    match e {
        Entity::Line(l) => Ok(vec![Part::Line {
            from: t.apply(xy(l.start_point)),
            to: t.apply(xy(l.end_point)),
        }]),
        Entity::Circle(c) => {
            let m = in_plane(c.extrusion, t).ok_or(Why::Tilted)?;
            if !(c.radius.is_finite() && c.radius > 0.0) {
                return Err(Why::BadRadius);
            }
            let (u, v) = images(&m, c.radius);
            Ok(vec![Part::Circle {
                centre: m.apply(xy(c.center)),
                u,
                v,
                radius: c.radius,
            }])
        }
        Entity::Arc(a) => {
            let sweep = a.sweep().ok_or(Why::SweepUndefined)?;
            let m = in_plane(a.extrusion, t).ok_or(Why::Tilted)?;
            if !(a.radius.is_finite() && a.radius > 0.0) {
                return Err(Why::BadRadius);
            }
            let (u, v) = images(&m, a.radius);
            Ok(vec![Part::Arc {
                centre: m.apply(xy(a.center)),
                u,
                v,
                start: a.start_angle,
                sweep,
                radius: a.radius,
            }])
        }
        Entity::LwPolyline(p) | Entity::Polyline2D(p) => {
            let m = in_plane(p.extrusion, t).ok_or(Why::Tilted)?;
            Ok(bulge::segments(&p.vertices, p.closed)
                .map(|s| match s.arc {
                    None => Part::Line {
                        from: m.apply(s.from),
                        to: m.apply(s.to),
                    },
                    Some(arc) => {
                        let (u, v) = images(&m, arc.radius);
                        Part::Arc {
                            centre: m.apply(arc.center),
                            u,
                            v,
                            start: arc.start_angle,
                            sweep: arc.sweep,
                            radius: arc.radius,
                        }
                    }
                })
                .collect())
        }
        _ => Ok(Vec::new()),
    }
}

/// The images of the radius along x and along y under `m`.
fn images(m: &Affine2, radius: f64) -> (Point2D, Point2D) {
    (
        linear(m, Point2D { x: radius, y: 0.0 }),
        linear(m, Point2D { x: 0.0, y: radius }),
    )
}

impl Part {
    /// The two ends of a line or an arc and the direction leading from each
    /// into the piece; nothing for a circle, or for a piece with no length
    /// to lead along. The ends are not checked against [`MAX_COORD_MM`].
    pub(crate) fn ends(&self) -> Vec<End> {
        match *self {
            Part::Line { from, to } => match unit([to.x - from.x, to.y - from.y]) {
                Some(d) => vec![
                    End {
                        at: [from.x, from.y],
                        inward: d,
                    },
                    End {
                        at: [to.x, to.y],
                        inward: [-d[0], -d[1]],
                    },
                ],
                None => Vec::new(),
            },
            Part::Arc {
                centre,
                u,
                v,
                start,
                sweep,
                ..
            } => {
                // The direction the arc is travelled in at parameter `t`.
                let along = |t: f64| {
                    let s = sweep.signum();
                    [
                        s * (-u.x * t.sin() + v.x * t.cos()),
                        s * (-u.y * t.sin() + v.y * t.cos()),
                    ]
                };
                let end = start + sweep;
                let at = |t: f64| {
                    [
                        centre.x + u.x * t.cos() + v.x * t.sin(),
                        centre.y + u.y * t.cos() + v.y * t.sin(),
                    ]
                };
                match (unit(along(start)), unit(along(end))) {
                    (Some(a), Some(b)) => vec![
                        End {
                            at: at(start),
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
            Part::Circle { .. } => Vec::new(),
        }
    }

    /// The point of an arc or circle at angle `t`.
    pub(crate) fn at(&self, t: f64) -> Option<Point2D> {
        match *self {
            Part::Arc { centre, u, v, .. } | Part::Circle { centre, u, v, .. } => Some(Point2D {
                x: centre.x + u.x * t.cos() + v.x * t.sin(),
                y: centre.y + u.y * t.cos() + v.y * t.sin(),
            }),
            Part::Line { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An entity taken from the golden drawing's JSON with `patch` applied to
    /// its fields.
    fn entity(kind: &str, patch: serde_json::Value) -> Entity {
        let v: serde_json::Value =
            serde_json::from_str(include_str!("../tests/golden/g26.expected.json")).unwrap();
        let mut e = v["entities"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["type"] == kind || e.get(kind).is_some())
            .unwrap_or_else(|| panic!("no {kind} in the golden drawing"))
            .clone();
        for (k, val) in patch.as_object().unwrap() {
            e[k] = val.clone();
        }
        serde_json::from_value(e).unwrap()
    }

    #[test]
    fn a_line_is_one_part_taken_through_the_placement() {
        let e = entity(
            "LINE",
            serde_json::json!({"start_point": {"x": 1.0, "y": 2.0, "z": 0.0},
                               "end_point": {"x": 4.0, "y": 6.0, "z": 0.0}}),
        );
        let shift = Affine2 {
            e: 10.0,
            f: 20.0,
            ..Affine2::IDENTITY
        };
        let Ok(parts) = parts(&e, &shift) else {
            panic!("a line is placed")
        };
        assert_eq!(
            parts,
            vec![Part::Line {
                from: Point2D { x: 11.0, y: 22.0 },
                to: Point2D { x: 14.0, y: 26.0 },
            }]
        );
    }

    #[test]
    fn an_arc_with_equal_angles_says_so_before_its_plane() {
        let e = entity(
            "ARC",
            serde_json::json!({"start_angle": 1.0, "end_angle": 1.0,
                               "extrusion": {"x": 1.0, "y": 0.0, "z": 0.0}}),
        );
        assert_eq!(
            parts(&e, &Affine2::IDENTITY).unwrap_err(),
            Why::SweepUndefined
        );
    }

    #[test]
    fn a_circle_on_a_tilted_plane_is_not_placed() {
        let e = entity(
            "CIRCLE",
            serde_json::json!({"extrusion": {"x": 1.0, "y": 0.0, "z": 0.0}}),
        );
        assert_eq!(parts(&e, &Affine2::IDENTITY).unwrap_err(), Why::Tilted);
    }

    #[test]
    fn a_radius_that_is_not_a_positive_number_is_refused() {
        let e = entity("CIRCLE", serde_json::json!({"radius": -3.0}));
        assert_eq!(parts(&e, &Affine2::IDENTITY).unwrap_err(), Why::BadRadius);
    }

    #[test]
    fn a_mirror_plane_turns_the_arc_the_other_way() {
        let e = entity(
            "ARC",
            serde_json::json!({"center": {"x": 0.0, "y": 0.0, "z": 0.0}, "radius": 2.0,
                               "start_angle": 0.0, "end_angle": 1.0,
                               "extrusion": {"x": 0.0, "y": 0.0, "z": -1.0}}),
        );
        let Ok(parts) = parts(&e, &Affine2::IDENTITY) else {
            panic!("a mirror plane is flat")
        };
        let [part @ Part::Arc { u, .. }] = parts[..] else {
            panic!("one arc")
        };
        // The x axis of the arc's plane points the other way in the world.
        assert!(u.x < 0.0, "{u:?}");
        // Its start is where the drawing says, seen from above.
        let start = part.at(0.0).unwrap();
        assert!(
            (start.x + 2.0).abs() < 1e-12 && start.y.abs() < 1e-12,
            "{start:?}"
        );
    }
}
