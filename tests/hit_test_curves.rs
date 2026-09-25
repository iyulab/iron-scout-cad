//! Pointing at curves the file defines by parameters: an ELLIPSE, and a
//! SPLINE by its control points. They are measured through chords, and each
//! hit says how far the curve can be from them.

use iron_scout_cad::{hit_test, NotSearchedReason};
use serde_json::{json, Value};
use uncad_model::{CadDatabase, Point2D};

fn p(x: f64, y: f64) -> Point2D {
    Point2D { x, y }
}

fn common(id: u64) -> Value {
    json!({
        "id": id, "origin": "VECTOR", "confidence": "HIGH",
        "source_handle": {"type": "ABSENT"}, "layer": {"type": "RESOLVED", "data": "0"},
        "color_index": 256, "true_color": null, "invisible": false,
        "linetype": {"type": "BY_LAYER"}, "linetype_scale": 1.0,
        "lineweight": -1, "transparency": 0
    })
}

fn xyz(x: f64, y: f64) -> Value {
    json!({"x": x, "y": y, "z": 0.0})
}

fn drawing(entities: Vec<Value>) -> CadDatabase {
    CadDatabase {
        entities: serde_json::from_value(Value::Array(entities)).expect("the entities deserialize"),
        tables: Default::default(),
        read_diagnostics: Default::default(),
    }
}

fn entity(kind: &str, id: u64, fields: Value) -> Value {
    let mut e = fields;
    e["type"] = json!(kind);
    e["common"] = common(id);
    e
}

/// An ellipse centred on the origin with semi-axes 10 (along x) and 5,
/// running from parameter `start` to `end`.
fn ellipse(start: f64, end: f64) -> CadDatabase {
    drawing(vec![entity(
        "ELLIPSE",
        1,
        json!({
            "center": xyz(0.0, 0.0), "major_axis_endpoint": xyz(10.0, 0.0),
            "axis_ratio": 0.5, "start_angle": start, "end_angle": end,
            "extrusion": {"x": 0.0, "y": 0.0, "z": 1.0}
        }),
    )])
}

fn spline(
    control: &[(f64, f64)],
    knots: &[f64],
    weights: &[f64],
    fit: &[(f64, f64)],
) -> CadDatabase {
    drawing(vec![entity(
        "SPLINE",
        1,
        json!({
            "degree": 3, "closed": false, "periodic": false,
            "knots": knots, "weights": weights,
            "fit_points": fit.iter().map(|&(x, y)| xyz(x, y)).collect::<Vec<_>>(),
            "control_points": control.iter().map(|&(x, y)| xyz(x, y)).collect::<Vec<_>>()
        }),
    )])
}

const TOLERANCE: f64 = 0.01;

#[test]
fn an_ellipse_is_hit_on_its_curve_and_says_how_closely() {
    let db = ellipse(0.0, 0.0);
    // (6, 4) is on the ellipse: (6/10)^2 + (4/5)^2 = 1.
    let r = hit_test(&db, p(6.0, 4.0), TOLERANCE);
    assert_eq!(r.hits.len(), 1, "{r:?}");
    let hit = &r.hits[0];
    assert_eq!(hit.entity_type, "ELLIPSE");
    assert!(!hit.anchored);
    let within = hit
        .within
        .expect("a curve measured through chords says how closely");
    assert!(within <= TOLERANCE / 1000.0, "{within}");
    assert!(hit.distance <= within, "{hit:?}");
    assert!(r.unsupported.is_empty(), "{r:?}");
}

#[test]
fn a_whole_ellipse_encloses_its_middle() {
    let r = hit_test(&ellipse(0.0, 0.0), p(1.0, 1.0), TOLERANCE);
    assert!(r.hits.is_empty(), "{r:?}");
    assert_eq!(r.enclosing.len(), 1, "{r:?}");
}

#[test]
fn an_elliptical_arc_ends_where_its_parameters_do() {
    // A quarter: from (10, 0) to (0, 5).
    let db = ellipse(0.0, std::f64::consts::FRAC_PI_2);
    assert_eq!(hit_test(&db, p(0.0, 5.0), TOLERANCE).hits.len(), 1);
    let r = hit_test(&db, p(-10.0, 0.0), TOLERANCE);
    assert!(r.hits.is_empty() && r.enclosing.is_empty(), "{r:?}");
}

#[test]
fn a_spline_is_hit_on_its_curve_not_on_its_control_polygon() {
    // A clamped cubic Bezier: its midpoint is (P0 + 3 P1 + 3 P2 + P3) / 8.
    let db = spline(
        &[(0.0, 0.0), (1.0, 2.0), (3.0, 2.0), (4.0, 0.0)],
        &[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        &[],
        &[],
    );
    let r = hit_test(&db, p(2.0, 1.5), TOLERANCE);
    assert_eq!(r.hits.len(), 1, "{r:?}");
    let within = r.hits[0].within.expect("a bound");
    assert!(r.hits[0].distance <= within, "{r:?}");
    // The control point (1, 2) is off the curve.
    assert!(hit_test(&db, p(1.0, 2.0), TOLERANCE).hits.is_empty());
}

#[test]
fn a_spline_stored_by_the_points_it_passes_through_is_not_measured() {
    let db = spline(&[], &[], &[], &[(0.0, 0.0), (2.0, 1.0), (4.0, 0.0)]);
    // Even on a fit point: the curve between them is the drawing program's.
    let r = hit_test(&db, p(2.0, 1.0), TOLERANCE);
    assert!(r.hits.is_empty(), "{r:?}");
    assert!(r.unsupported.is_empty(), "{r:?}");
    assert_eq!(r.not_searched.len(), 1, "{r:?}");
    assert_eq!(r.not_searched[0].reason, NotSearchedReason::CurveUndefined);
}

#[test]
fn a_rational_spline_is_listed_rather_than_given_an_unbounded_distance() {
    let db = spline(
        &[(0.0, 0.0), (1.0, 2.0), (3.0, 2.0), (4.0, 0.0)],
        &[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        &[1.0, 3.0, 3.0, 1.0],
        &[],
    );
    let r = hit_test(&db, p(2.0, 1.5), TOLERANCE);
    assert!(r.hits.is_empty(), "{r:?}");
    assert_eq!(r.not_searched.len(), 1, "{r:?}");
    assert_eq!(
        r.not_searched[0].reason,
        NotSearchedReason::CurveBoundUnknown
    );
}

#[test]
fn an_exactly_measured_hit_carries_no_bound() {
    let db = drawing(vec![entity(
        "LINE",
        1,
        json!({"start_point": xyz(0.0, 0.0), "end_point": xyz(10.0, 0.0)}),
    )]);
    let r = hit_test(&db, p(5.0, 0.0), TOLERANCE);
    assert_eq!(r.hits.len(), 1);
    assert_eq!(r.hits[0].within, None);
    let json = serde_json::to_string(&r).unwrap();
    assert!(!json.contains("within"), "{json}");
}
