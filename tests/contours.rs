//! Which outlines close, on golden case G26 (a flat pattern drawn as pieces).
//! The expected numbers were counted by hand from the drawing's spec before
//! the code was written; the gap sweep at the end counts them again from the
//! drawing's own free ends.

use iron_scout_cad::{contours, summarize, ContourKind, Contours};
use uncad_model::CadDatabase;

const JOIN_UM: u64 = 100;

fn g26() -> CadDatabase {
    serde_json::from_str(include_str!("golden/g26.expected.json"))
        .expect("the golden model deserializes")
}

fn at(gap_um: u64) -> Contours {
    contours(&g26(), gap_um).expect("G26 states millimetres")
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn at_the_signature_gap_four_outlines_close_one_closes_with_a_branch_and_four_stay_open() {
    let c = at(JOIN_UM);
    assert_eq!((c.loops, c.branched, c.open), (4, 1, 4));
    assert_eq!(c.components.len(), 9);
    assert_eq!(
        c.components
            .iter()
            .map(|c| c.free_ends.len())
            .sum::<usize>(),
        8
    );
    assert!(c.not_read.is_empty(), "{:?}", c.not_read);
}

#[test]
fn each_group_has_the_pieces_length_and_layer_its_drawing_spec_gives() {
    let c = at(JOIN_UM);
    let by_len = |len: f64| {
        c.components
            .iter()
            .find(|g| close(g.length_mm, len))
            .unwrap_or_else(|| panic!("no group of length {len}: {:?}", c.components))
    };
    // (length, kind, pieces, layers)
    let tri = 14.0 + 193f64.sqrt() + (49.0 + 11.95f64 * 11.95).sqrt();
    for (len, kind, pieces, layer) in [
        (
            240.0 + 20.0 * std::f64::consts::PI,
            ContourKind::Loop,
            8,
            "CUT",
        ),
        (10.0 * std::f64::consts::PI, ContourKind::Loop, 1, "CUT"),
        (
            40.0 + 10.0 * std::f64::consts::PI,
            ContourKind::Loop,
            4,
            "CUT",
        ),
        (tri, ContourKind::Loop, 3, "CUT"),
        (53.5, ContourKind::Open, 4, "CUT"),
        (52.0, ContourKind::Open, 4, "CUT"),
        (36.0, ContourKind::Branched, 5, "CUT"),
        (54.0, ContourKind::Open, 1, "BEND"),
        (32.0, ContourKind::Open, 1, "BEND"),
    ] {
        let g = by_len(len);
        assert_eq!(
            (g.kind, g.pieces, g.layers.as_slice()),
            (kind, pieces, [layer.to_string()].as_slice()),
            "{len}"
        );
    }
}

#[test]
fn a_free_end_names_how_far_the_nearest_end_of_another_piece_is() {
    let c = at(JOIN_UM);
    let gaps = |len: f64| -> Vec<f64> {
        let g = c
            .components
            .iter()
            .find(|g| close(g.length_mm, len))
            .unwrap();
        g.free_ends
            .iter()
            .map(|e| e.nearest_gap_mm.expect("a nearest end exists"))
            .collect()
    };
    assert!(
        gaps(53.5).iter().all(|d| close(*d, 0.5)),
        "{:?}",
        gaps(53.5)
    );
    assert!(
        gaps(52.0).iter().all(|d| close(*d, 2.0)),
        "{:?}",
        gaps(52.0)
    );
    // A bend line stops well away from everything else.
    assert!(gaps(54.0).iter().all(|d| *d > 2.5));
    assert!(gaps(32.0).iter().all(|d| *d > 2.5));
    // Where the free end lies is in the drawing's units (mm here).
    let rect = c
        .components
        .iter()
        .find(|g| close(g.length_mm, 53.5))
        .unwrap();
    let mut ys: Vec<f64> = rect.free_ends.iter().map(|e| e.at.y).collect();
    ys.sort_by(f64::total_cmp);
    assert!(close(ys[0], 40.0) && close(ys[1], 40.5), "{ys:?}");
}

/// Closure is a property of the gap: counted again here from the drawing's
/// free ends, the hand-counted numbers for a narrower and two wider gaps.
#[test]
fn the_gap_decides_which_near_misses_close() {
    // (gap in micrometres, outlines that close incl. the circle, open groups)
    for (gap, closed, open) in [(10, 3, 5), (100, 4, 4), (1_000, 5, 3), (2_500, 6, 2)] {
        let c = at(gap);
        assert_eq!((c.loops, c.open), (closed, open), "gap {gap} um: {:?}", c);
        assert_eq!(c.branched, 1, "gap {gap} um");
    }
}

#[test]
fn the_summary_carries_the_totals_and_a_drawing_without_a_unit_has_none() {
    let totals = summarize(&g26()).contour_totals.expect("millimetres");
    assert_eq!(
        (totals.loops, totals.branched, totals.open, totals.free_ends),
        (4, 1, 4, 8)
    );
    assert_eq!(totals.join_gap_um, JOIN_UM);
    assert!(close(totals.length_mm_by_layer["BEND"], 86.0));
    assert!(close(totals.length_mm_by_layer["CUT"], 588.905_428_1));
    assert!(close(
        totals.closed_length_mm,
        302.831_853 + 31.415_927 + 71.415_927 + 41.741_722 + 36.0
    ));

    let mut v: serde_json::Value =
        serde_json::from_str(include_str!("golden/g26.expected.json")).unwrap();
    v["header"]["insunits"] = serde_json::Value::Null;
    let unitless: CadDatabase = serde_json::from_value(v).unwrap();
    assert_eq!(summarize(&unitless).contour_totals, None);
    assert_eq!(contours(&unitless, JOIN_UM), None);
}

#[test]
fn what_is_not_read_is_counted_by_name() {
    // G1 has a title block reference: its block is not entered.
    let g1: CadDatabase = serde_json::from_str(include_str!("golden/g1.expected.json")).unwrap();
    let c = contours(&g1, JOIN_UM).unwrap();
    assert_eq!(c.not_read.get("INSERT"), Some(&1));
}

// ---- what an independent read of the grouping drew from degenerate drawings ----

use serde_json::{json, Value};

const TAU: f64 = std::f64::consts::TAU;

/// G26's header with `entities` in place of its drawing.
fn drawing(entities: Vec<Value>) -> CadDatabase {
    let mut v: Value = serde_json::from_str(include_str!("golden/g26.expected.json")).unwrap();
    v["entities"] = entities.into();
    serde_json::from_value(v).unwrap()
}

fn common(id: u64, layer: &str) -> Value {
    let v: Value = serde_json::from_str(include_str!("golden/g26.expected.json")).unwrap();
    let mut c = v["entities"][0]["common"].clone();
    c["id"] = id.to_string().into();
    c["layer"] = json!({"type": "RESOLVED", "data": layer});
    c
}

fn line(id: u64, layer: &str, from: [f64; 2], to: [f64; 2]) -> Value {
    json!({
        "type": "LINE", "common": common(id, layer),
        "start_point": {"x": from[0], "y": from[1], "z": 0.0},
        "end_point": {"x": to[0], "y": to[1], "z": 0.0},
    })
}

fn arc(id: u64, centre: [f64; 2], r: f64, from: f64, to: f64) -> Value {
    json!({
        "type": "ARC", "common": common(id, "CUT"),
        "center": {"x": centre[0], "y": centre[1], "z": 0.0}, "radius": r,
        "start_angle": from, "end_angle": to,
        "extrusion": {"x": 0.0, "y": 0.0, "z": 1.0},
    })
}

fn circle(id: u64, centre: [f64; 2], r: f64) -> Value {
    json!({
        "type": "CIRCLE", "common": common(id, "CUT"),
        "center": {"x": centre[0], "y": centre[1], "z": 0.0}, "radius": r,
        "extrusion": {"x": 0.0, "y": 0.0, "z": 1.0},
    })
}

fn polyline(id: u64, vertices: &[[f64; 2]], closed: bool) -> Value {
    json!({
        "type": "LWPOLYLINE", "common": common(id, "CUT"),
        "vertices": vertices.iter().map(|p| json!({
            "point": {"x": p[0], "y": p[1]}, "bulge": 0.0, "start_width": 0.0, "end_width": 0.0,
        })).collect::<Vec<_>>(),
        "closed": closed, "const_width": 0.0, "elevation": 0.0,
        "extrusion": {"x": 0.0, "y": 0.0, "z": 1.0},
    })
}

fn counts(c: &Contours) -> (u32, u32, u32) {
    (c.loops, c.branched, c.open)
}

/// An arc drawn from 0 to 360 degrees, or from 0 to just under it, has its ends
/// where they started: it closes on itself the way a circle does.
#[test]
fn an_arc_whose_ends_meet_closes_on_itself() {
    for end in [TAU, TAU - 0.0005] {
        let c = contours(&drawing(vec![arc(1, [0.0, 0.0], 10.0, 0.0, end)]), JOIN_UM).unwrap();
        assert_eq!(counts(&c), (1, 0, 0), "to {end}");
        assert_eq!(c.components[0].pieces, 1);
        assert!(c.components[0].free_ends.is_empty());
        assert!(c.not_read.is_empty(), "{:?}", c.not_read);
    }
}

/// A repeated vertex is a piece with no length: it neither opens nor branches
/// the outline it sits in, and is counted.
#[test]
fn a_repeated_vertex_leaves_the_square_a_loop() {
    let square = [
        [0.0, 0.0],
        [10.0, 0.0],
        [10.0, 0.0],
        [10.0, 10.0],
        [0.0, 10.0],
    ];
    let c = contours(&drawing(vec![polyline(1, &square, true)]), JOIN_UM).unwrap();
    assert_eq!(counts(&c), (1, 0, 0), "{:?}", c.components);
    assert_eq!(c.components[0].pieces, 4);
    assert!(close(c.components[0].length_mm, 40.0));
    assert_eq!(c.not_read.get("DEGENERATE"), Some(&1));
    // A line of no length drawn on its own is only counted.
    let c = contours(
        &drawing(vec![line(2, "CUT", [5.0, 5.0], [5.0, 5.0])]),
        JOIN_UM,
    )
    .unwrap();
    assert_eq!((counts(&c), c.components.len()), ((0, 0, 0), 0));
    assert_eq!(c.not_read.get("DEGENERATE"), Some(&1));
}

/// Grouping follows ends, not crossings: a closed polyline whose two diagonals
/// cross at a point that is no vertex is one loop of four pieces (the crossing
/// joins nothing), and the same vertices left open are one open group with two
/// free ends. Nothing is said about an area, which a crossing outline has no
/// single value for.
#[test]
fn a_polyline_that_crosses_itself_is_grouped_by_its_ends_not_its_crossing() {
    let bow_tie = [[0.0, 0.0], [10.0, 10.0], [10.0, 0.0], [0.0, 10.0]];
    let diagonal = 10.0 * std::f64::consts::SQRT_2;
    let closed = contours(&drawing(vec![polyline(1, &bow_tie, true)]), JOIN_UM).unwrap();
    assert_eq!(counts(&closed), (1, 0, 0), "{:?}", closed.components);
    assert_eq!(closed.components[0].pieces, 4);
    assert!(close(closed.components[0].length_mm, 2.0 * diagonal + 20.0));
    assert!(closed.components[0].free_ends.is_empty());

    let open = contours(&drawing(vec![polyline(2, &bow_tie, false)]), JOIN_UM).unwrap();
    assert_eq!(counts(&open), (0, 0, 1), "{:?}", open.components);
    assert_eq!(open.components[0].pieces, 3);
    assert!(close(open.components[0].length_mm, 2.0 * diagonal + 10.0));
    assert_eq!(open.components[0].free_ends.len(), 2);
}

/// Two pieces drawn on top of each other enclose nothing; two that meet at both
/// ends and are not the same curve do.
#[test]
fn two_pieces_on_top_of_each_other_are_a_side_drawn_twice_and_two_halves_a_circle() {
    let twice = [
        line(1, "CUT", [0.0, 0.0], [10.0, 0.0]),
        line(2, "CUT", [10.0, 0.0], [0.0, 0.0]),
    ];
    let c = contours(&drawing(twice.to_vec()), JOIN_UM).unwrap();
    assert_eq!(counts(&c), (0, 1, 0));
    let same_arc = [
        arc(1, [0.0, 0.0], 10.0, 0.0, std::f64::consts::PI),
        arc(2, [0.0, 0.0], 10.0, 0.0, std::f64::consts::PI),
    ];
    let c = contours(&drawing(same_arc.to_vec()), JOIN_UM).unwrap();
    assert_eq!(counts(&c), (0, 1, 0));
    let halves = [
        arc(1, [0.0, 0.0], 10.0, 0.0, std::f64::consts::PI),
        arc(2, [0.0, 0.0], 10.0, std::f64::consts::PI, TAU),
    ];
    let c = contours(&drawing(halves.to_vec()), JOIN_UM).unwrap();
    assert_eq!(counts(&c), (1, 0, 0));
    assert_eq!(c.components[0].pieces, 2);
}

/// A number that is not a coordinate is not placed -- not a panic, not a length
/// that is not a number, and not silently gone.
#[test]
fn a_piece_that_cannot_be_placed_is_counted_and_adds_no_length() {
    let mut db = drawing(vec![
        line(1, "CUT", [0.0, 0.0], [10.0, 0.0]),
        line(2, "CUT", [0.0, 0.0], [1e300, 0.0]),
        line(3, "CUT", [0.0, 5.0], [10.0, 5.0]),
        circle(4, [0.0, 0.0], -5.0),
        circle(5, [0.0, 0.0], 0.0),
    ]);
    if let uncad_model::model::Entity::Line(l) = &mut db.entities[2] {
        l.start_point.x = f64::NAN;
    }
    for gap in [0, JOIN_UM, 1_000] {
        let c = contours(&db, gap).unwrap();
        assert_eq!(c.not_read.get("UNPLACEABLE"), Some(&4), "gap {gap}");
        assert!(
            c.components.iter().all(|g| g.length_mm.is_finite()),
            "{:?}",
            c.components
        );
        let total: f64 = c.components.iter().map(|g| g.length_mm).sum();
        assert!(close(total, 10.0), "{total}");
    }
}

/// A gap of nothing and a coordinate at the edge of what is placed do not
/// overflow the grid.
#[test]
fn a_gap_of_nothing_and_far_coordinates_do_not_overflow() {
    let db = drawing(vec![
        line(1, "CUT", [9e11, 9e11], [9e11 + 1.0, 9e11]),
        line(2, "CUT", [9e11 + 1.0, 9e11], [9e11 + 1.0, 9e11 + 1.0]),
        line(3, "CUT", [-9e11, -9e11], [0.0, 0.0]),
    ]);
    for gap in [0, 1] {
        let c = contours(&db, gap).unwrap();
        assert_eq!(counts(&c), (0, 0, 2), "gap {gap}");
    }
}

/// A drawing of one open line has ends and no other end: it names no budget.
#[test]
fn a_line_with_nothing_near_names_no_nearest_distance_and_no_budget() {
    let c = contours(
        &drawing(vec![line(1, "CUT", [0.0, 0.0], [10.0, 0.0])]),
        JOIN_UM,
    )
    .unwrap();
    assert_eq!(counts(&c), (0, 0, 1));
    assert_eq!(c.components[0].free_ends.len(), 2);
    assert!(c.components[0]
        .free_ends
        .iter()
        .all(|e| e.nearest_gap_mm.is_none()));
    assert!(c.not_read.is_empty(), "{:?}", c.not_read);
}

/// An arc with equal angles has no ends to join: it is counted, not dropped.
#[test]
fn an_arc_with_equal_angles_is_counted() {
    let c = contours(&drawing(vec![arc(1, [0.0, 0.0], 10.0, 1.0, 1.0)]), JOIN_UM).unwrap();
    assert!(c.components.is_empty());
    assert_eq!(c.not_read.get("ARC_SWEEP_UNDEFINED"), Some(&1));
}

/// The answer is a property of the drawing, not of the order the file lists it
/// in: G26 listed reversed and shuffled reads to the same groups, lengths and
/// free ends.
#[test]
fn the_order_the_file_lists_the_entities_in_does_not_change_the_answer() {
    let original = g26();
    let expected = contours(&original, JOIN_UM).unwrap();
    let mut reversed = original.clone();
    reversed.entities.reverse();
    assert_eq!(contours(&reversed, JOIN_UM).unwrap(), expected);
    // A fixed shuffle: every third entity first, then the rest.
    let mut shuffled = original.clone();
    let n = shuffled.entities.len();
    shuffled.entities = (0..3)
        .flat_map(|r| (0..n).filter(move |i| i % 3 == r))
        .map(|i| original.entities[i].clone())
        .collect();
    assert_eq!(contours(&shuffled, JOIN_UM).unwrap(), expected);
    for gap in [10, 1_000, 2_500] {
        assert_eq!(
            contours(&reversed, gap).unwrap(),
            contours(&original, gap).unwrap(),
            "gap {gap}"
        );
    }
}

/// A group that spans layers says how much of its length each one holds, and the
/// layers' totals add up to the pieces.
#[test]
fn a_group_that_spans_layers_splits_its_length_by_layer() {
    let db = drawing(vec![
        line(1, "CUT", [0.0, 0.0], [10.0, 0.0]),
        line(2, "BEND", [10.0, 0.0], [10.0, 4.0]),
    ]);
    let c = contours(&db, JOIN_UM).unwrap();
    assert_eq!(c.components.len(), 1);
    let g = &c.components[0];
    assert!(close(g.length_mm, 14.0));
    assert!(close(g.length_mm_by_layer["CUT"], 10.0));
    assert!(close(g.length_mm_by_layer["BEND"], 4.0));
    let t = c.totals();
    assert!(close(t.length_mm_by_layer["CUT"], 10.0));
    assert!(close(t.length_mm_by_layer["BEND"], 4.0));
}
