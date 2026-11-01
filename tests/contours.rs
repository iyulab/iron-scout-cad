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
