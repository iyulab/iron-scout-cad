//! The same drawing must give the same summary and the same hits, byte for
//! byte, every time.

use iron_scout_cad::{hit_test, summarize};
use uncad_model::{CadDatabase, Point2D};

/// With a handful of entries in a leaked hash set, two consecutive
/// identical orders are already unlikely; this many leave no realistic
/// chance of a false pass.
const RUNS: usize = 24;

fn g1() -> CadDatabase {
    serde_json::from_str(include_str!("golden/g1.expected.json"))
        .expect("the golden model deserializes")
}

#[test]
fn repeated_summaries_are_byte_identical() {
    let db = g1();
    let first = serde_json::to_string(&summarize(&db)).unwrap();
    for run in 1..RUNS {
        let again = serde_json::to_string(&summarize(&db)).unwrap();
        assert_eq!(first, again, "run {run}: the summary changed");
    }
}

#[test]
fn repeated_hit_tests_are_byte_identical() {
    let db = g1();
    let point = Point2D { x: 25.0, y: 20.0 };
    let first = serde_json::to_string(&hit_test(&db, point, 0.5)).unwrap();
    for run in 1..RUNS {
        let again = serde_json::to_string(&hit_test(&db, point, 0.5)).unwrap();
        assert_eq!(first, again, "run {run}: the hit test changed");
    }
}

#[test]
fn the_input_is_never_modified() {
    let db = g1();
    let before = serde_json::to_string(&db).unwrap();
    let _ = summarize(&db);
    let _ = hit_test(&db, Point2D { x: 0.0, y: 0.0 }, 1.0);
    assert_eq!(serde_json::to_string(&db).unwrap(), before);
}
