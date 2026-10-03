//! The shape signature, checked on the first golden case (a plate with four
//! holes, four dimensions and a title block reference carrying attributes)
//! and on edits of it made directly in the model.

use iron_scout_cad::{signature, summarize, Signature, SIGNATURE_VERSION};
use std::collections::BTreeMap;
use uncad_model::model::Entity;
use uncad_model::CadDatabase;

fn g1() -> CadDatabase {
    serde_json::from_str(include_str!("golden/g1.expected.json"))
        .expect("the golden model deserializes")
}

fn model_space(db: &mut CadDatabase) -> &mut Vec<Entity> {
    &mut db
        .tables
        .block_records
        .get_mut("*Model_Space")
        .expect("model space")
        .entities
}

/// The names of the components in which `a` and `b` differ.
fn differing(a: &Signature, b: &Signature) -> Vec<String> {
    let (a, b) = (
        serde_json::to_value(a).unwrap(),
        serde_json::to_value(b).unwrap(),
    );
    a.as_object()
        .unwrap()
        .iter()
        .filter(|(k, v)| b.get(k.as_str()) != Some(v))
        .map(|(k, _)| k.clone())
        .collect()
}

#[test]
fn the_plate_counts_its_outline_holes_and_dimensions_and_reports_the_title_block() {
    let s = signature(&g1());
    assert_eq!(s.signature_version, SIGNATURE_VERSION);
    let entities: BTreeMap<String, u64> = [
        ("CIRCLE", 4),
        ("LWPOLYLINE", 1),
        ("POLYLINE_LINE_SEGMENT", 4),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    assert_eq!(s.entities, entities);
    // Holes of radius 5 mm: four of 10 mm across.
    assert_eq!(
        s.circle_diameters_um,
        Some([(10_000, 4)].into_iter().collect())
    );
    // Two edges of 200 mm (bin 8: 128..256 mm), two of 100 mm (bin 7).
    assert_eq!(s.line_length_bins, Some(vec![0, 0, 0, 0, 0, 0, 0, 2, 2]));
    assert_eq!(s.arc_radius_bins, Some(vec![]));
    let dimensions: BTreeMap<String, u64> = [("DIAMETER", 1), ("ROTATED", 3)]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    assert_eq!(s.dimensions, dimensions);
    assert_eq!(
        s.excluded_inserts,
        [("TITLEBLOCK".to_string(), 1)].into_iter().collect()
    );
    assert!(s.not_measured.is_empty(), "{:?}", s.not_measured);
    // The summary carries the same signature.
    assert_eq!(summarize(&g1()).signature, s);
}

#[test]
fn one_hole_made_larger_differs_in_the_diameters_alone() {
    let before = g1();
    let mut after = before.clone();
    let hole = model_space(&mut after)
        .iter_mut()
        .find_map(|e| match e {
            Entity::Circle(c) => Some(c),
            _ => None,
        })
        .expect("a hole");
    hole.radius = 6.0;
    let (a, b) = (signature(&before), signature(&after));
    assert_eq!(differing(&a, &b), ["circle_diameters_um"]);
    assert_eq!(
        b.circle_diameters_um,
        Some([(10_000, 3), (12_000, 1)].into_iter().collect())
    );
}

#[test]
fn a_reference_carrying_attributes_changes_only_what_was_left_out() {
    let with = g1();
    let mut without = with.clone();
    model_space(&mut without).retain(|e| !matches!(e, Entity::Insert(_)));
    let (a, b) = (signature(&with), signature(&without));
    assert_eq!(differing(&a, &b), ["excluded_inserts"]);
    assert!(b.excluded_inserts.is_empty());
}

#[test]
fn without_a_stated_unit_no_length_is_given() {
    let mut db = g1();
    db.header.insunits = None;
    let s = signature(&db);
    assert_eq!(s.circle_diameters_um, None);
    assert_eq!(s.line_length_bins, None);
    assert_eq!(s.arc_radius_bins, None);
    // What is counted does not need a unit.
    assert_eq!(s.entities, signature(&g1()).entities);
}

#[test]
fn a_block_reference_is_expanded_and_its_scale_applied() {
    // The title block without its attributes: a plain reference, so its
    // frame is counted -- at twice the size.
    let mut db = g1();
    for e in model_space(&mut db).iter_mut() {
        if let Entity::Insert(i) = e {
            i.attribs.clear();
            i.scale = uncad_model::Point3D {
                x: 2.0,
                y: 2.0,
                z: 2.0,
            };
        }
    }
    let plain = signature(&g1());
    let s = signature(&db);
    assert!(s.excluded_inserts.is_empty());
    assert_eq!(
        s.entities["POLYLINE_LINE_SEGMENT"],
        plain.entities["POLYLINE_LINE_SEGMENT"] + 4
    );
    assert_eq!(s.entities["LWPOLYLINE"], 2);
    // Under a scale that differs between axes, a circle is not measured.
    let mut skewed = g1();
    let block = skewed
        .tables
        .block_records
        .get_mut("TITLEBLOCK")
        .expect("the title block");
    let hole = model_space(&mut g1())
        .iter()
        .find(|e| matches!(e, Entity::Circle(_)))
        .cloned()
        .unwrap();
    block.entities.push(hole);
    for e in model_space(&mut skewed).iter_mut() {
        if let Entity::Insert(i) = e {
            i.attribs.clear();
            i.scale = uncad_model::Point3D {
                x: 2.0,
                y: 1.0,
                z: 1.0,
            };
        }
    }
    let s = signature(&skewed);
    assert_eq!(s.not_measured["CIRCLE_NON_UNIFORM_SCALE"], 1);
    assert_eq!(s.circle_diameters_um, plain.circle_diameters_um);
}

#[test]
fn the_same_drawing_gives_the_same_bytes() {
    let a = serde_json::to_vec(&signature(&g1())).unwrap();
    let b = serde_json::to_vec(&signature(&g1())).unwrap();
    assert_eq!(a, b);
}

#[test]
fn dimension_kinds_are_named_as_the_model_spells_them() {
    use uncad_model::model::DimensionKind::*;
    for kind in [
        Rotated,
        Aligned,
        Angular2Line,
        Diameter,
        Radius,
        Angular3Point,
        Ordinate,
        ArcLength,
    ] {
        let mut db = g1();
        for e in model_space(&mut db).iter_mut() {
            if let Entity::Dimension(d) = e {
                d.kind = Some(kind);
            }
        }
        let spelled = serde_json::to_value(kind).unwrap();
        let s = signature(&db);
        assert_eq!(
            s.dimensions.keys().collect::<Vec<_>>(),
            [spelled.as_str().unwrap()],
            "{kind:?}"
        );
    }
}

/// A chain of `depth` block definitions, each placing the next `fan` times,
/// the last holding one circle; model space places the first once.
fn nested(depth: usize, fan: usize) -> CadDatabase {
    let mut db = g1();
    let hole = model_space(&mut db)
        .iter()
        .find(|e| matches!(e, Entity::Circle(_)))
        .cloned()
        .unwrap();
    let reference = model_space(&mut db)
        .iter()
        .find_map(|e| match e {
            Entity::Insert(i) => Some(i.clone()),
            _ => None,
        })
        .unwrap();
    let template = db.tables.block_records["TITLEBLOCK"].clone();
    let place = |name: String| {
        let mut i = reference.clone();
        i.attribs.clear();
        i.block_name = uncad_model::model::Ref::Resolved(name);
        Entity::Insert(i)
    };
    for level in 0..depth {
        let mut block = template.clone();
        block.entities = if level + 1 == depth {
            vec![hole.clone()]
        } else {
            (0..fan).map(|_| place(format!("L{}", level + 1))).collect()
        };
        db.tables.block_records.insert(format!("L{level}"), block);
    }
    let space = model_space(&mut db);
    space.clear();
    space.push(place("L0".to_string()));
    db
}

#[test]
fn nesting_deeper_than_the_limit_stops_and_says_so() {
    let s = signature(&nested(64, 1));
    assert_eq!(s.not_measured.get("INSERT_TOO_DEEP"), Some(&1));
    assert_eq!(s.entities.get("CIRCLE"), None);
}

#[test]
fn a_reference_chain_that_multiplies_stops_at_the_budget_and_says_so() {
    // 10 levels placing the next 10 times each would be 10^9 circles; the
    // walk meets at most ten million entities inside expanded blocks.
    let s = signature(&nested(10, 10));
    assert!(
        s.not_measured.contains_key("INSERT_BUDGET_EXHAUSTED"),
        "{:?}",
        s.not_measured
    );
    let circles = s.entities.get("CIRCLE").copied().unwrap_or(0);
    assert!(circles > 0 && circles < 10_000_000, "{circles}");
}
