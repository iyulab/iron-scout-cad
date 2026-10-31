//! Layer 3 of the shape signature -- how a view's pieces are joined, counted
//! as local patterns -- checked on G24, a plate whose outline is separate
//! lines and an arc meeting at their ends.

use iron_scout_cad::{signature, PatternCounts, Signature, MAX_NODES};
use std::collections::BTreeMap;
use uncad_model::model::{Entity, Ref};
use uncad_model::{CadDatabase, Point3D};

fn g1() -> CadDatabase {
    serde_json::from_str(include_str!("golden/g1.expected.json")).expect("G1 deserializes")
}

fn g24() -> CadDatabase {
    serde_json::from_str(include_str!("golden/g24.expected.json")).expect("G24 deserializes")
}

fn model_space(db: &mut CadDatabase) -> &mut Vec<Entity> {
    &mut db
        .tables
        .block_records
        .iter_mut()
        .find(|(name, _)| name.eq_ignore_ascii_case("*Model_Space"))
        .expect("a model space")
        .1
        .entities
}

/// The one view of `db` and the patterns it makes.
fn patterns(db: &CadDatabase) -> PatternCounts {
    let s = signature(db);
    let views = s.views.expect("views");
    assert_eq!(views.len(), 1, "one view");
    views[0].wl.clone().expect("its patterns are counted")
}

fn round<'a>(p: &'a PatternCounts, r: &str) -> &'a BTreeMap<String, u64> {
    &p.rounds[r]
}

/// `db` with each point sent through `point` and each arc's angles through
/// `angles` (which takes the start and end angles in radians, the arc being
/// read counter-clockwise from start to end).
fn moved(
    mut db: CadDatabase,
    point: impl Fn(f64, f64) -> (f64, f64),
    angles: impl Fn(f64, f64) -> (f64, f64),
) -> CadDatabase {
    let map = |p: &mut Point3D| {
        let (x, y) = point(p.x, p.y);
        p.x = x;
        p.y = y;
    };
    for e in model_space(&mut db) {
        match e {
            Entity::Line(l) => {
                map(&mut l.start_point);
                map(&mut l.end_point);
            }
            Entity::Circle(c) => map(&mut c.center),
            Entity::Arc(a) => {
                map(&mut a.center);
                let (s, e) = angles(a.start_angle, a.end_angle);
                a.start_angle = s;
                a.end_angle = e;
            }
            _ => {}
        }
    }
    db
}

fn count(m: &BTreeMap<String, u64>, label: &str) -> u64 {
    m.get(label).copied().unwrap_or(0)
}

#[test]
fn the_pieces_of_the_plate_and_how_they_meet() {
    let p = patterns(&g24());
    // Lengths 100 and 90 mm are bin 7, 60 and 50 are bin 6, the loose line
    // (30 mm) is bin 5; the corner is a quarter circle (two 45-degree steps)
    // of radius 10 mm (bin 4); the hole is 10 mm across (bin 4).
    let r0: Vec<(&str, u64)> = round(&p, "r0")
        .iter()
        .map(|(k, n)| (k.as_str(), *n))
        .collect();
    assert_eq!(
        r0,
        [
            ("ARC:r4:a2", 1),
            ("CIRCLE:b4", 1),
            ("LINE:b5", 1),
            ("LINE:b6", 2),
            ("LINE:b7", 2),
        ]
    );
    // Round 1: each piece with the way it meets the pieces at its ends. The
    // arc joins its two lines smoothly, the square corners are right angles,
    // and what touches nothing has no entry.
    let r1: Vec<(&str, u64)> = round(&p, "r1")
        .iter()
        .map(|(k, n)| (k.as_str(), *n))
        .collect();
    assert_eq!(
        r1,
        [
            ("ARC:r4:a2[smooth LINE:b6, smooth LINE:b7]", 1),
            ("CIRCLE:b4[]", 1),
            ("LINE:b5[]", 1),
            ("LINE:b6[orthogonal LINE:b7, orthogonal LINE:b7]", 1),
            ("LINE:b6[orthogonal LINE:b7, smooth ARC:r4:a2]", 1),
            ("LINE:b7[orthogonal LINE:b6, orthogonal LINE:b6]", 1),
            ("LINE:b7[orthogonal LINE:b6, smooth ARC:r4:a2]", 1),
        ]
    );
    assert!(!p.truncated);
    // Three rounds are kept: r0, r1, r2.
    assert_eq!(p.rounds.keys().collect::<Vec<_>>(), ["r0", "r1", "r2"]);
    assert_eq!(round(&p, "r2").values().sum::<u64>(), 7);
}

#[test]
fn the_parameters_are_stated_with_the_version() {
    let s: Signature = signature(&g24());
    assert_eq!(s.signature_version, 3);
    assert_eq!(s.wl_params.endpoint_gap_um, 100);
    assert_eq!(s.wl_params.angle_tolerance_deg, 1);
    assert_eq!(s.wl_params.rounds, 2);
    assert_eq!(s.wl_params.label_hash, "fnv1a-64");
}

#[test]
fn the_order_the_entities_are_stored_in_changes_nothing() {
    let mut db = g24();
    let before = serde_json::to_string(&signature(&db)).unwrap();
    model_space(&mut db).reverse();
    assert_eq!(before, serde_json::to_string(&signature(&db)).unwrap());
}

#[test]
fn turning_moving_or_mirroring_the_view_changes_nothing() {
    let base = patterns(&g24());
    let quarter = std::f64::consts::FRAC_PI_2;
    // A quarter turn about the origin, then a move.
    let turned = moved(
        g24(),
        |x, y| (-y + 300.0, x - 50.0),
        |s, e| (s + quarter, e + quarter),
    );
    assert_eq!(patterns(&turned), base);
    // A mirror across the y axis: an arc read counter-clockwise runs the
    // other way, so its start and end swap places.
    let pi = std::f64::consts::PI;
    let mirrored = moved(g24(), |x, y| (-x, y), |s, e| (pi - e, pi - s));
    assert_eq!(patterns(&mirrored), base);
    // A turn by an angle that is no multiple of a right angle.
    let t = 0.6_f64;
    let skew = moved(
        g24(),
        |x, y| {
            (
                x * t.cos() - y * t.sin() + 7.0,
                x * t.sin() + y * t.cos() - 3.0,
            )
        },
        |s, e| (s + t, e + t),
    );
    assert_eq!(patterns(&skew), base);
}

#[test]
fn twice_the_size_is_a_different_view() {
    let doubled = moved(g24(), |x, y| (2.0 * x, 2.0 * y), |s, e| (s, e));
    let mut db = doubled;
    for e in model_space(&mut db) {
        match e {
            Entity::Circle(c) => {
                c.radius *= 2.0;
                // Kept within 5 mm of the outline, so the sheet still holds
                // one view.
                c.center.x = 11.0;
            }
            Entity::Arc(a) => a.radius *= 2.0,
            Entity::Line(l) if l.start_point.y == 6.0 => {
                l.start_point.y = 4.0;
                l.end_point.y = 4.0;
            }
            _ => {}
        }
    }
    let p = patterns(&db);
    assert_ne!(p, patterns(&g24()));
    // The arc's radius is 20 mm now (bin 5); the 200 and 180 mm sides are
    // bin 8.
    assert_eq!(count(round(&p, "r0"), "ARC:r5:a2"), 1);
    assert_eq!(count(round(&p, "r0"), "LINE:b8"), 2);
}

#[test]
fn a_bigger_hole_changes_the_hole_and_nothing_of_the_outline() {
    let mut db = g24();
    for e in model_space(&mut db) {
        if let Entity::Circle(c) = e {
            c.radius = 9.0; // 18 mm across: bin 5, not 4
        }
    }
    let (a, b) = (patterns(&g24()), patterns(&db));
    for r in ["r0", "r1", "r2"] {
        let strip = |m: &BTreeMap<String, u64>| {
            m.iter()
                .filter(|(k, _)| !k.starts_with("CIRCLE"))
                .map(|(k, n)| (k.clone(), *n))
                .collect::<BTreeMap<_, _>>()
        };
        assert_eq!(strip(round(&a, r)), strip(round(&b, r)), "{r}");
    }
    assert_eq!(count(round(&a, "r0"), "CIRCLE:b4"), 1);
    assert_eq!(count(round(&b, "r0"), "CIRCLE:b5"), 1);
}

#[test]
fn two_ends_closer_than_the_gap_meet_and_two_further_apart_do_not() {
    let base = patterns(&g24());
    // The left side starts at the top-left corner; lift its start by 50 and
    // by 150 micrometres from where the top side ends.
    let lifted = |dy: f64| {
        let mut db = g24();
        for e in model_space(&mut db) {
            if let Entity::Line(l) = e {
                if l.start_point.x == 0.0 && l.start_point.y == 60.0 {
                    l.start_point.y += dy;
                }
            }
        }
        patterns(&db)
    };
    assert_eq!(lifted(0.05), base, "50 um is within the gap");
    assert_ne!(lifted(0.15), base, "150 um is not");
}

#[test]
fn one_line_drawn_as_two_is_a_different_pattern() {
    // The bottom side as two halves meeting in a straight continuation: the
    // signature does not merge pieces, and says so by counting them as they
    // are drawn.
    let mut db = g24();
    let space = model_space(&mut db);
    let at = space
        .iter()
        .position(|e| matches!(e, Entity::Line(l) if l.end_point.x == 100.0 && l.end_point.y == 0.0 && l.start_point.x == 0.0))
        .expect("the bottom side");
    let Entity::Line(whole) = space[at].clone() else {
        unreachable!()
    };
    let mut first = whole.clone();
    first.end_point.x = 50.0;
    let mut second = whole;
    second.start_point.x = 50.0;
    space[at] = Entity::Line(first);
    space.push(Entity::Line(second));
    let p = patterns(&db);
    assert_ne!(p, patterns(&g24()));
    // Two 50 mm halves (bin 6) that meet smoothly.
    assert!(round(&p, "r1")
        .keys()
        .any(|k| k.starts_with("LINE:b6[") && k.contains("smooth LINE:b6")));
}

#[test]
fn a_view_with_more_pieces_than_the_limit_has_no_counts_and_says_why() {
    let mut db = g24();
    let line = model_space(&mut db)
        .iter()
        .find(|e| matches!(e, Entity::Line(_)))
        .cloned()
        .unwrap();
    // A block of one line more than the limit, placed once.
    let mut source = g1();
    let mut reference = model_space(&mut source)
        .iter()
        .find_map(|e| match e {
            Entity::Insert(i) => Some(i.clone()),
            _ => None,
        })
        .expect("G1 places a block");
    reference.attribs.clear();
    reference.block_name = Ref::Resolved("BIG".to_string());
    let mut block = source.tables.block_records["TITLEBLOCK"].clone();
    block.entities = (0..=MAX_NODES).map(|_| line.clone()).collect();
    db.tables.block_records.insert("BIG".to_string(), block);
    let space = model_space(&mut db);
    space.clear();
    space.push(Entity::Insert(reference));
    let s = signature(&db);
    let view = &s.views.expect("views")[0];
    assert!(view.wl.is_none());
    assert_eq!(view.wl_note.as_deref(), Some("too_large"));
}

#[test]
fn a_drawing_with_no_unit_has_no_views_and_so_no_counts() {
    let mut db = g24();
    db.header.insunits = None;
    assert!(signature(&db).views.is_none());
}

#[test]
fn the_same_drawing_twice_gives_the_same_bytes() {
    let one = serde_json::to_string(&signature(&g24())).unwrap();
    let two = serde_json::to_string(&signature(&g24())).unwrap();
    assert_eq!(one, two);
}
