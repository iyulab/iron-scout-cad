//! Pointing at a HATCH: its boundary paths, in the hatch's own plane, are
//! where it is; the area they bound -- islands alternating -- encloses the
//! points inside it.

use iron_scout_cad::{hit_test, HitTest, NotSearchedReason};
use serde_json::{json, Value};
use std::f64::consts::{FRAC_PI_2, PI};
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

fn pt(x: f64, y: f64) -> Value {
    json!({"x": x, "y": y})
}

fn vertex(x: f64, y: f64, bulge: f64) -> Value {
    json!({"point": pt(x, y), "bulge": bulge})
}

fn polyline_path(points: &[(f64, f64, f64)]) -> Value {
    let vertices: Vec<Value> = points.iter().map(|&(x, y, b)| vertex(x, y, b)).collect();
    json!({"type": "POLYLINE", "data": vertices})
}

fn square(lo: f64, hi: f64) -> Value {
    polyline_path(&[(lo, lo, 0.0), (hi, lo, 0.0), (hi, hi, 0.0), (lo, hi, 0.0)])
}

fn hatch_in(paths: Vec<Value>, extrusion: (f64, f64, f64)) -> CadDatabase {
    let mut e = json!({
        "boundary_paths": paths, "solid_fill": true, "gradient": null,
        "pattern_lines": [], "elevation": 0.0,
        "extrusion": {"x": extrusion.0, "y": extrusion.1, "z": extrusion.2},
        "style": null
    });
    e["type"] = json!("HATCH");
    e["common"] = common(1);
    CadDatabase {
        entities: serde_json::from_value(json!([e])).expect("the hatch deserializes"),
        tables: Default::default(),
        header: Default::default(),
        read_diagnostics: Default::default(),
    }
}

fn hatch(paths: Vec<Value>) -> CadDatabase {
    hatch_in(paths, (0.0, 0.0, 1.0))
}

fn only_hit(r: &HitTest) -> f64 {
    assert!(
        r.unsupported.is_empty() && r.not_searched.is_empty(),
        "{r:?}"
    );
    assert_eq!(r.hits.len(), 1, "{r:?}");
    assert_eq!(r.hits[0].entity_type, "HATCH");
    r.hits[0].distance
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn a_hatch_is_hit_on_its_boundary_and_encloses_its_inside() {
    let db = hatch(vec![square(0.0, 10.0)]);
    assert_eq!(only_hit(&hit_test(&db, p(5.0, 0.0), 1e-9)), 0.0);
    assert!(close(only_hit(&hit_test(&db, p(5.0, 11.0), 1.5)), 1.0));
    let inside = hit_test(&db, p(5.0, 5.0), 1.0);
    assert!(inside.hits.is_empty());
    assert_eq!(inside.enclosing.len(), 1);
    assert!(close(inside.enclosing[0].distance, 5.0));
    assert!(inside.enclosing[0].within.is_none());
}

#[test]
fn a_bulged_polyline_boundary_is_measured_on_its_arc() {
    // The bottom side bulges down into a half circle of radius 5 about
    // (5, 0): bulge 1 is half a turn counter-clockwise, so it runs below.
    let db = hatch(vec![polyline_path(&[
        (0.0, 0.0, 1.0),
        (10.0, 0.0, 0.0),
        (10.0, 10.0, 0.0),
        (0.0, 10.0, 0.0),
    ])]);
    assert!(close(only_hit(&hit_test(&db, p(5.0, -5.0), 1e-9)), 0.0));
    // The straight chord is not drawn.
    let chord = hit_test(&db, p(5.0, 0.0), 1.0);
    assert!(chord.hits.is_empty());
    assert_eq!(chord.enclosing.len(), 1);
}

fn line(sx: f64, sy: f64, ex: f64, ey: f64) -> Value {
    json!({"type": "LINE", "start": pt(sx, sy), "end": pt(ex, ey)})
}

fn arc(cx: f64, cy: f64, r: f64, start: f64, end: f64, ccw: bool) -> Value {
    json!({
        "type": "ARC", "center": pt(cx, cy), "radius": r,
        "start_angle": start, "end_angle": end, "is_ccw": ccw
    })
}

fn edges(list: Vec<Value>) -> Value {
    json!({"type": "EDGES", "data": list})
}

#[test]
fn an_arc_edge_is_measured_exactly() {
    // A D shape: the line from (0, -5) up to (0, 5), and the right half of
    // the circle of radius 5 back down, counter-clockwise from -90 degrees.
    let db = hatch(vec![edges(vec![
        arc(0.0, 0.0, 5.0, -FRAC_PI_2, FRAC_PI_2, true),
        line(0.0, 5.0, 0.0, -5.0),
    ])]);
    let at = |a: f64| p(5.0 * a.cos(), 5.0 * a.sin());
    for a in [-1.2, -0.3, 0.0, 0.7, 1.5] {
        assert!(close(only_hit(&hit_test(&db, at(a), 1e-9)), 0.0), "{a}");
    }
    // Beside the arc by 0.25, exactly.
    assert!(close(only_hit(&hit_test(&db, p(5.25, 0.0), 0.5)), 0.25));
    // The left half is not drawn, and is outside.
    let left = hit_test(&db, p(-5.0, 0.0), 1.0);
    assert!(left.hits.is_empty() && left.enclosing.is_empty());
    assert_eq!(hit_test(&db, p(2.0, 0.0), 1.0).enclosing.len(), 1);
}

#[test]
fn a_clockwise_arc_edge_states_its_angles_measured_clockwise() {
    // The same right half circle, written clockwise from +90 down to -90:
    // clockwise angles are the complements, so it states -90 and +90.
    let db = hatch(vec![edges(vec![
        line(0.0, -5.0, 0.0, 5.0),
        arc(0.0, 0.0, 5.0, -FRAC_PI_2, FRAC_PI_2, false),
    ])]);
    assert!(close(only_hit(&hit_test(&db, p(5.0, 0.0), 1e-9)), 0.0));
    let left = hit_test(&db, p(-5.0, 0.0), 1.0);
    assert!(
        left.hits.is_empty() && left.enclosing.is_empty(),
        "{left:?}"
    );
    assert_eq!(hit_test(&db, p(2.0, 0.0), 1.0).enclosing.len(), 1);
}

#[test]
fn a_whole_circle_edge_is_a_whole_circle() {
    let db = hatch(vec![edges(vec![arc(0.0, 0.0, 3.0, 0.0, 0.0, true)])]);
    for a in [0.0, 0.5 * PI, PI, 1.5 * PI] {
        assert!(close(
            only_hit(&hit_test(&db, p(3.0 * a.cos(), 3.0 * a.sin()), 1e-9)),
            0.0
        ));
    }
    assert_eq!(hit_test(&db, p(0.0, 0.0), 1.0).enclosing.len(), 1);
}

#[test]
fn an_elliptical_edge_is_cut_into_chords_and_says_how_far_they_may_be() {
    // A whole ellipse, semi-axes 10 and 5.
    let db = hatch(vec![edges(vec![json!({
        "type": "ELLIPSE", "center": pt(0.0, 0.0), "end": pt(10.0, 0.0),
        "minor_major_ratio": 0.5, "start_angle": 0.0, "end_angle": 2.0 * PI,
        "is_ccw": true
    })])]);
    let r = hit_test(&db, p(0.0, 5.0), 1e-3);
    let d = only_hit(&r);
    let within = r.hits[0]
        .within
        .expect("an elliptical edge is measured through chords");
    assert!(within > 0.0 && d <= within, "{r:?}");
    assert_eq!(hit_test(&db, p(8.0, 0.0), 1.0).enclosing.len(), 1);
}

#[test]
fn an_island_is_not_enclosed() {
    let db = hatch(vec![square(0.0, 10.0), square(3.0, 7.0)]);
    let island = hit_test(&db, p(5.0, 5.0), 1.0);
    assert!(island.hits.is_empty() && island.enclosing.is_empty());
    assert_eq!(hit_test(&db, p(1.5, 5.0), 1.0).enclosing.len(), 1);
    // The island's own edge is the hatch's boundary too.
    assert_eq!(only_hit(&hit_test(&db, p(3.0, 5.0), 1e-9)), 0.0);
}

#[test]
fn a_mirror_copy_is_measured_where_it_is_drawn() {
    // Seen from below, the hatch's x runs the other way: the D shape of the
    // arc test lands left of the y axis.
    let db = hatch_in(
        vec![edges(vec![
            arc(0.0, 0.0, 5.0, -FRAC_PI_2, FRAC_PI_2, true),
            line(0.0, 5.0, 0.0, -5.0),
        ])],
        (0.0, 0.0, -1.0),
    );
    assert!(close(only_hit(&hit_test(&db, p(-5.0, 0.0), 1e-9)), 0.0));
    assert!(close(only_hit(&hit_test(&db, p(-3.0, 4.0), 1e-9)), 0.0));
    assert!(hit_test(&db, p(5.0, 0.0), 1.0).hits.is_empty());
    assert_eq!(hit_test(&db, p(-2.0, 0.0), 1.0).enclosing.len(), 1);
}

#[test]
fn a_hatch_on_a_tilted_plane_is_not_searched() {
    let db = hatch_in(vec![square(0.0, 10.0)], (0.0, 0.6, 0.8));
    let r = hit_test(&db, p(5.0, 0.0), 1.0);
    assert!(r.hits.is_empty());
    assert_eq!(r.not_searched.len(), 1);
    assert_eq!(
        r.not_searched[0].reason,
        NotSearchedReason::NonSimilarPlacement
    );
}

#[test]
fn a_spline_edge_the_file_does_not_define_is_not_guessed() {
    // Only fit points: the program that draws it fits a curve.
    let db = hatch(vec![edges(vec![
        json!({
            "type": "SPLINE", "degree": 3, "rational": false, "periodic": false,
            "knots": [], "control_points": [], "weights": [],
            "fit_points": [pt(0.0, 0.0), pt(5.0, 3.0), pt(10.0, 0.0)],
            "start_tangent": null, "end_tangent": null
        }),
        line(10.0, 0.0, 0.0, 0.0),
    ])]);
    let r = hit_test(&db, p(5.0, 0.0), 1.0);
    assert!(r.hits.is_empty());
    assert_eq!(r.not_searched.len(), 1);
    assert_eq!(r.not_searched[0].reason, NotSearchedReason::CurveUndefined);
}

#[test]
fn a_spline_edge_the_file_defines_is_measured_through_chords() {
    // A degree-one spline edge is its control polygon, exactly.
    let db = hatch(vec![edges(vec![
        json!({
            "type": "SPLINE", "degree": 1, "rational": false, "periodic": false,
            "knots": [0.0, 0.0, 1.0, 2.0, 2.0],
            "control_points": [pt(0.0, 0.0), pt(5.0, 5.0), pt(10.0, 0.0)],
            "weights": [], "fit_points": [],
            "start_tangent": null, "end_tangent": null
        }),
        line(10.0, 0.0, 0.0, 0.0),
    ])]);
    let r = hit_test(&db, p(2.5, 2.5), 1e-9);
    assert!(close(only_hit(&r), 0.0));
    assert_eq!(hit_test(&db, p(5.0, 1.0), 0.5).enclosing.len(), 1);
}
