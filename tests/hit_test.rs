//! Pointing, checked on the golden cases: a hole of the general part (G1),
//! two coincident lines (G6), a line three block references deep (G2), a
//! reference to a block that does not exist (G10), and mirror copies of a
//! circle, an arc and a bulged polyline (G7).

use iron_scout_cad::{hit_test, Hit, NotSearched, NotSearchedReason};
use uncad_model::model::{Confidence, Entity, EntityId, Ref};
use uncad_model::{CadDatabase, Point2D};

fn golden(json: &str) -> CadDatabase {
    serde_json::from_str(json).expect("the golden model deserializes")
}

fn g1() -> CadDatabase {
    golden(include_str!("golden/g1.expected.json"))
}

fn g6() -> CadDatabase {
    golden(include_str!("golden/g6.expected.json"))
}

fn p(x: f64, y: f64) -> Point2D {
    Point2D { x, y }
}

/// The hole centred at (20, 20), radius 5.
fn first_hole(db: &CadDatabase) -> EntityId {
    db.entities
        .iter()
        .find_map(|e| match e {
            Entity::Circle(c) if c.center.x == 20.0 && c.center.y == 20.0 => Some(c.common.id),
            _ => None,
        })
        .expect("G1 has a hole at (20, 20)")
}

#[test]
fn a_point_on_a_circles_edge_hits_it_at_distance_zero() {
    let db = g1();
    let result = hit_test(&db, p(25.0, 20.0), 1e-9);
    // The hole's diameter dimension draws its line out to the same edge:
    // two hits, by reference ID, neither preferred.
    let types: Vec<&str> = result.hits.iter().map(|h| h.entity_type.as_str()).collect();
    assert_eq!(types, ["CIRCLE", "DIMENSION"], "{:?}", result.hits);
    assert_eq!(result.hits[0].id, first_hole(&db));
    assert_eq!(result.hits[0].entity_type, "CIRCLE");
    assert_eq!(result.hits[0].distance, 0.0);
    assert!(!result.hits[0].anchored);
    assert_eq!(result.hits[0].confidence, Confidence::High);
    // The point is inside the part's outline, which encloses it without
    // being near it.
    let enclosing: Vec<&str> = result
        .enclosing
        .iter()
        .map(|h| h.entity_type.as_str())
        .collect();
    assert_eq!(enclosing, ["LWPOLYLINE"]);
}

#[test]
fn a_point_inside_a_hole_is_enclosed_by_it_not_on_it() {
    let db = g1();
    let result = hit_test(&db, p(20.0, 20.0), 0.5);
    // The hole itself is not hit -- but two dimensions draw through its
    // centre: the diameter's line, and the hole-to-hole distance's
    // extension line, which starts there.
    let ids: Vec<u64> = result.hits.iter().map(|h| h.id.value()).collect();
    assert_eq!(ids, [295, 296], "{:?}", result.hits);
    assert!(result.hits.iter().all(|h| h.entity_type == "DIMENSION"));
    // Enclosed by the hole and by the outline around it, by reference ID
    // (the outline was written first).
    assert_eq!(result.enclosing.len(), 2, "{:?}", result.enclosing);
    let hole = result
        .enclosing
        .iter()
        .find(|h| h.id == first_hole(&db))
        .expect("the hole encloses its centre");
    assert_eq!(hole.distance, 5.0);
    assert_eq!(result.enclosing[0].entity_type, "LWPOLYLINE");
    assert_eq!(result.enclosing[1].entity_type, "CIRCLE");
}

#[test]
fn a_point_on_the_outline_is_inside_no_hole_and_on_the_outline() {
    let db = g1();
    let result = hit_test(&db, p(100.0, 0.0), 1e-9);
    let types: Vec<&str> = result.hits.iter().map(|h| h.entity_type.as_str()).collect();
    // The overall width's dimension states the middle of its text there too,
    // so it is a second hit -- at its anchor, not on its drawn lines.
    assert_eq!(types, ["LWPOLYLINE", "DIMENSION"]);
    assert!(!result.hits[0].anchored && result.hits[1].anchored);
    assert!(
        result.enclosing.is_empty(),
        "the outline is hit, not merely enclosing"
    );
    // A point well inside the outline and outside every hole encloses only
    // the outline.
    let result = hit_test(&db, p(100.0, 50.0), 1e-9);
    assert!(result.hits.is_empty());
    let types: Vec<&str> = result
        .enclosing
        .iter()
        .map(|h| h.entity_type.as_str())
        .collect();
    assert_eq!(types, ["LWPOLYLINE"]);
}

#[test]
fn every_entity_type_of_the_general_part_is_searched() {
    // A type this crate cannot search is named in `unsupported` (see the
    // leader cases in `hit_test_annotations.rs`); the general part has none.
    let db = g1();
    let result = hit_test(&db, p(0.0, 0.0), 1e-9);
    assert!(result.unsupported.is_empty(), "{:?}", result.unsupported);
}

#[test]
fn two_coincident_lines_are_two_hits_neither_preferred() {
    let db = g6();
    let ids: Vec<EntityId> = db.entities.iter().map(|e| e.common().id).collect();
    let result = hit_test(&db, p(25.0, 0.0), 1e-9);
    let hit_ids: Vec<EntityId> = result.hits.iter().map(|h| h.id).collect();
    assert_eq!(hit_ids, ids, "both lines, by reference ID");
    assert!(result.hits.iter().all(|h| h.distance == 0.0));
    // Nearest first when the distances differ.
    let result = hit_test(&db, p(25.0, 0.5), 1.0);
    assert_eq!(result.hits.len(), 2);
    assert_eq!(result.hits[0].distance, 0.5);
}

#[test]
fn a_text_is_hit_at_its_anchor_and_says_so() {
    let db = g1();
    // The first ATTRIB of the title block sits at (125, -45).
    let result = hit_test(&db, p(125.0, -45.0), 1e-9);
    let attribs: Vec<&iron_scout_cad::Hit> = result
        .hits
        .iter()
        .filter(|h| h.entity_type == "ATTRIB")
        .collect();
    assert_eq!(attribs.len(), 1);
    assert!(attribs[0].anchored);
}

// --- block references -------------------------------------------------

fn g2() -> CadDatabase {
    serde_json::from_str(include_str!("golden/g2.expected.json"))
        .expect("the golden model deserializes")
}

fn g10() -> CadDatabase {
    serde_json::from_str(include_str!("golden/g10.expected.json"))
        .expect("the golden model deserializes")
}

fn ids(hits: &[Hit]) -> Vec<(EntityId, Vec<EntityId>)> {
    hits.iter().map(|h| (h.id, h.via.clone())).collect()
}

#[test]
fn a_line_three_blocks_deep_is_hit_where_the_composed_placement_draws_it() {
    // G2: a line (0,0)-(10,0) in block C, placed in B at (5,0) turned a
    // quarter, B in A scaled 2, A in the drawing at (100,100): the line is
    // drawn from (110,100) to (110,120).
    let db = g2();
    let r = hit_test(&db, Point2D { x: 110.0, y: 110.0 }, 0.5);
    assert_eq!(r.hits.len(), 1, "{:?}", r.hits);
    let hit = &r.hits[0];
    assert_eq!(hit.entity_type, "LINE");
    assert_eq!(hit.distance, 0.0);
    assert!(!hit.anchored);
    assert_eq!(
        hit.via.len(),
        3,
        "reached through three INSERTs: {:?}",
        hit.via
    );
    assert!(r.not_searched.is_empty());
    assert!(r.unsupported.is_empty());

    // Just beside the line: nothing.
    let r = hit_test(&db, Point2D { x: 112.0, y: 110.0 }, 0.5);
    assert!(r.hits.is_empty(), "{:?}", r.hits);
}

#[test]
fn nested_block_references_are_hit_at_their_placed_insertion_points() {
    let db = g2();
    // A is inserted at (100,100); B sits at A's origin, so its placed
    // insertion point is (100,100) too. Both are anchored hits; the outer
    // one has no chain, the inner one is reached through the outer.
    let r = hit_test(&db, Point2D { x: 100.0, y: 100.0 }, 0.5);
    assert_eq!(r.hits.len(), 2, "{:?}", r.hits);
    assert!(r
        .hits
        .iter()
        .all(|h| h.entity_type == "INSERT" && h.anchored && h.distance == 0.0));
    // Equally near, so ordered by reference ID -- not by depth.
    assert!(r.hits[0].id < r.hits[1].id);
    let outer = r
        .hits
        .iter()
        .find(|h| h.via.is_empty())
        .expect("the drawing's own INSERT");
    let inner = r
        .hits
        .iter()
        .find(|h| !h.via.is_empty())
        .expect("the nested INSERT");
    assert_eq!(inner.via, [outer.id]);

    // C sits at (5,0) in B, scaled 2 by A: placed at (110,100), where the
    // line's start point also is. Nearest first, then by ID.
    let r = hit_test(&db, Point2D { x: 110.0, y: 100.0 }, 0.5);
    let kinds: Vec<&str> = r.hits.iter().map(|h| h.entity_type.as_str()).collect();
    assert_eq!(kinds.len(), 2, "{:?}", r.hits);
    assert!(kinds.contains(&"LINE") && kinds.contains(&"INSERT"));
    assert!(r.hits.iter().all(|h| h.distance == 0.0));
    let line = r.hits.iter().find(|h| h.entity_type == "LINE").unwrap();
    let insert = r.hits.iter().find(|h| h.entity_type == "INSERT").unwrap();
    assert_eq!(line.via.len(), 3);
    assert_eq!(insert.via.len(), 2);
    assert_eq!(&line.via[..2], &insert.via[..]);
}

#[test]
fn a_reference_to_no_block_is_reported_not_skipped() {
    // G10: an INSERT naming a block the file never defines, so the
    // reference is unresolved and keeps that name. The reference itself is
    // still anchored at its insertion point; what it would draw is reported
    // as not searched, with the reason -- and the reason distinguishes this
    // from a file that points at no block at all.
    let db = g10();
    let insert = db
        .entities
        .iter()
        .find_map(|e| match e {
            Entity::Insert(i) => Some(i),
            _ => None,
        })
        .expect("G10 has a block reference");
    let at = Point2D {
        x: insert.insertion_point.x,
        y: insert.insertion_point.y,
    };
    let r = hit_test(&db, at, 0.5);
    assert_eq!(ids(&r.hits), [(insert.common.id, vec![])]);
    assert_eq!(
        r.not_searched,
        [NotSearched {
            id: insert.common.id,
            entity_type: "INSERT".into(),
            via: vec![],
            reason: NotSearchedReason::BlockReferenceUnresolved,
        }]
    );
    // Far away: the reason is still reported -- it does not depend on the point.
    let r = hit_test(&db, Point2D { x: -1e6, y: -1e6 }, 0.5);
    assert_eq!(r.not_searched.len(), 1);
}

/// A drawing whose one block holds a circle, placed by one INSERT with the
/// given per-axis scale.
fn circle_in_a_block(x_scale: f64, y_scale: f64) -> CadDatabase {
    use uncad_model::model::{CircleEntity, EntityCommon, InsertEntity, Origin, Point3D};
    use uncad_model::tables::BlockRecord;
    let common = |id: u64| EntityCommon {
        id: EntityId::new(id),
        origin: Origin::Vector,
        confidence: Confidence::High,
        source_handle: Ref::Absent,
        layer: Ref::Resolved("0".into()),
        color_index: 256,
        true_color: None,
        invisible: false,
        linetype: uncad_model::model::EntityLinetype::ByLayer,
        linetype_scale: 1.0,
        lineweight: Some(-1),
        transparency: Some(0),
    };
    let circle = Entity::Circle(CircleEntity {
        common: common(1),
        center: Point3D {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        radius: 5.0,
        extrusion: Point3D {
            x: 0.0,
            y: 0.0,
            z: 1.0,
        },
    });
    let insert = Entity::Insert(InsertEntity {
        common: common(2),
        block_name: Ref::Resolved("HOLE".into()),
        insertion_point: Point3D {
            x: 50.0,
            y: 50.0,
            z: 0.0,
        },
        scale: Point3D {
            x: x_scale,
            y: y_scale,
            z: 1.0,
        },
        rotation: 0.0,
        attribs: Vec::new(),
        extrusion: Point3D {
            x: 0.0,
            y: 0.0,
            z: 1.0,
        },
    });
    let mut db = CadDatabase {
        entities: vec![insert],
        tables: Default::default(),
        read_diagnostics: Default::default(),
    };
    db.tables.block_records.insert(
        "HOLE".into(),
        BlockRecord {
            name: "HOLE".into(),
            entities: vec![circle],
        },
    );
    db
}

#[test]
fn a_circle_in_a_uniformly_scaled_block_is_a_circle_of_the_scaled_radius() {
    let db = circle_in_a_block(2.0, 2.0);
    // Radius 5 scaled 2 around (50,50): the edge passes through (60,50).
    let r = hit_test(&db, Point2D { x: 60.0, y: 50.0 }, 0.01);
    assert_eq!(r.hits.len(), 1, "{:?}", r.hits);
    assert_eq!(r.hits[0].entity_type, "CIRCLE");
    assert_eq!(r.hits[0].via, [EntityId::new(2)]);
    // The centre is enclosed, not hit.
    let r = hit_test(&db, Point2D { x: 50.0, y: 50.0 }, 0.01);
    assert_eq!(r.enclosing.len(), 1);
    assert_eq!(r.hits.len(), 1, "the INSERT's own anchor");
    assert!(r.hits[0].anchored);
}

#[test]
fn a_circle_in_a_stretched_block_is_not_guessed_at() {
    let db = circle_in_a_block(2.0, 1.0);
    let r = hit_test(&db, Point2D { x: 60.0, y: 50.0 }, 0.01);
    assert!(r.hits.is_empty(), "{:?}", r.hits);
    assert_eq!(
        r.not_searched,
        [NotSearched {
            id: EntityId::new(1),
            entity_type: "CIRCLE".into(),
            via: vec![EntityId::new(2)],
            reason: NotSearchedReason::NonSimilarPlacement,
        }]
    );
}

#[test]
fn a_block_that_references_itself_ends_with_the_depth_reported() {
    use uncad_model::model::{EntityCommon, InsertEntity, Origin, Point3D};
    use uncad_model::tables::BlockRecord;
    let common = |id: u64| EntityCommon {
        id: EntityId::new(id),
        origin: Origin::Vector,
        confidence: Confidence::High,
        source_handle: Ref::Absent,
        layer: Ref::Resolved("0".into()),
        color_index: 256,
        true_color: None,
        invisible: false,
        linetype: uncad_model::model::EntityLinetype::ByLayer,
        linetype_scale: 1.0,
        lineweight: Some(-1),
        transparency: Some(0),
    };
    let refer = |id: u64| {
        Entity::Insert(InsertEntity {
            common: common(id),
            block_name: Ref::Resolved("LOOP".into()),
            insertion_point: Point3D {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            scale: Point3D {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            },
            rotation: 0.0,
            attribs: Vec::new(),
            extrusion: Point3D {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
        })
    };
    let mut db = CadDatabase {
        entities: vec![refer(1)],
        tables: Default::default(),
        read_diagnostics: Default::default(),
    };
    db.tables.block_records.insert(
        "LOOP".into(),
        BlockRecord {
            name: "LOOP".into(),
            entities: vec![refer(2)],
        },
    );
    let r = hit_test(&db, Point2D { x: 0.0, y: 0.0 }, 0.5);
    assert!(r
        .not_searched
        .iter()
        .any(|n| n.reason == NotSearchedReason::NestingTooDeep));
    assert!(r.hits.len() > 1 && r.hits.len() < 100, "{}", r.hits.len());
}

#[test]
fn a_hidden_entity_is_still_hit_and_says_it_is_hidden() {
    // G2's line three block references deep, drawn at (110, 100)-(110, 120).
    let at = Point2D { x: 110.0, y: 110.0 };
    let db = g2();
    let shown = hit_test(&db, at, 0.5);
    assert!(!shown.hits[0].invisible, "nothing in G2 is hidden");

    // The line itself hidden: still hit, marked.
    let mut hidden_line = g2();
    for block in hidden_line.tables.block_records.values_mut() {
        for e in &mut block.entities {
            if let Entity::Line(l) = e {
                l.common.invisible = true;
            }
        }
    }
    let r = hit_test(&hidden_line, at, 0.5);
    assert_eq!(r.hits.len(), 1, "{:?}", r.hits);
    assert!(r.hits[0].invisible);

    // Only the outermost block reference hidden: everything it draws is.
    let mut hidden_ref = g2();
    let outer = shown.hits[0].via[0];
    for e in &mut hidden_ref.entities {
        if e.common().id == outer {
            if let Entity::Insert(i) = e {
                i.common.invisible = true;
            }
        }
    }
    let r = hit_test(&hidden_ref, at, 0.5);
    assert_eq!(r.hits.len(), 1, "{:?}", r.hits);
    assert!(r.hits[0].invisible, "hidden through its reference");
    assert_eq!(r.hits[0].via, shown.hits[0].via);
}

fn g7() -> CadDatabase {
    golden(include_str!("golden/g7.expected.json"))
}

fn types_hit(r: &iron_scout_cad::HitTest) -> Vec<&str> {
    r.hits.iter().map(|h| h.entity_type.as_str()).collect()
}

#[test]
fn a_mirror_copied_circle_is_hit_where_it_is_drawn() {
    // G7's mirror copy of a circle: written at (-170, -50) with extrusion
    // (0, 0, -1), drawn at (170, -50), radius 3.
    let r = hit_test(&g7(), p(173.0, -50.0), 0.5);
    assert!(types_hit(&r).contains(&"CIRCLE"), "{:?}", r.hits);
    assert!(r.not_searched.is_empty(), "{:?}", r.not_searched);
    // Where it is written, nothing is drawn.
    let r = hit_test(&g7(), p(-173.0, -50.0), 0.5);
    assert!(!types_hit(&r).contains(&"CIRCLE"), "{:?}", r.hits);
}

#[test]
fn a_mirror_copied_arc_keeps_its_side_of_the_circle() {
    // G7's mirror copy of an arc from 30 to 150 degrees about (-110, -50),
    // radius 4: drawn about (110, -50), still over the top of its circle.
    let top = hit_test(&g7(), p(110.0, -46.0), 0.5);
    assert!(types_hit(&top).contains(&"ARC"), "{:?}", top.hits);
    let bottom = hit_test(&g7(), p(110.0, -54.0), 0.5);
    assert!(!types_hit(&bottom).contains(&"ARC"), "{:?}", bottom.hits);
}

#[test]
fn a_mirror_copied_polyline_arc_is_measured_along_the_arc() {
    // G7's mirror-copied triangle, drawn through (150, -58), (140, -58) and
    // (145, -53); the edge from (140, -58) to (145, -53) bulges 0.5 in its
    // own plane, outward from the triangle once drawn. A point 0.6 off the
    // chord toward the arc is 0.6 from the chord but about 1.17 from the
    // arc: not on the outline, and inside the shape the arc bounds.
    let k = 0.6 / 2f64.sqrt();
    let r = hit_test(&g7(), p(142.5 - k, -55.5 + k), 0.7);
    assert!(!types_hit(&r).contains(&"LWPOLYLINE"), "{:?}", r.hits);
    assert!(
        r.enclosing.iter().any(|h| h.entity_type == "LWPOLYLINE"),
        "{:?}",
        r.enclosing
    );
    // The arc's middle, the sagitta (0.5 * chord / 2) off the chord.
    let sag = 0.5 * 50f64.sqrt() / 2.0 / 2f64.sqrt();
    let r = hit_test(&g7(), p(142.5 - sag, -55.5 + sag), 1e-9);
    assert!(types_hit(&r).contains(&"LWPOLYLINE"), "{:?}", r.hits);
}

#[test]
fn a_mirror_copied_block_is_hit_where_it_is_drawn() {
    // G7's mirror copy of a block: its line (0, 0)-(8, 0), placed at
    // (-175, -66) turned 30 degrees in a system whose x is the world's -x,
    // is drawn from (175, -66) to (175 - 8 cos 30, -66 + 8 sin 30).
    let (sin, cos) = 30f64.to_radians().sin_cos();
    let mid = p(175.0 - 4.0 * cos, -66.0 + 4.0 * sin);
    let r = hit_test(&g7(), mid, 1e-6);
    assert!(types_hit(&r).contains(&"LINE"), "{:?}", r.hits);
    assert!(r.not_searched.is_empty(), "{:?}", r.not_searched);
    // Its unmirrored place, reflected across x = 175, is empty.
    let r = hit_test(&g7(), p(175.0 + 4.0 * cos, -66.0 + 4.0 * sin), 0.5);
    assert!(!types_hit(&r).contains(&"LINE"), "{:?}", r.hits);
}

#[test]
fn a_block_on_a_tilted_plane_is_not_searched_and_says_why() {
    let mut db = g7();
    for e in &mut db.entities {
        if let Entity::Insert(i) = e {
            i.extrusion = uncad_model::Point3D {
                x: 1.0,
                y: 0.0,
                z: 1.0,
            };
        }
    }
    let r = hit_test(&db, p(175.0, -66.0), 0.5);
    assert!(
        r.not_searched.iter().any(
            |n| n.entity_type == "INSERT" && n.reason == NotSearchedReason::NonSimilarPlacement
        ),
        "{:?}",
        r.not_searched
    );
}

#[test]
fn a_circle_on_a_tilted_plane_is_not_searched_and_says_why() {
    // Seen from above, a circle on a tilted plane is an ellipse; this crate
    // does not guess where its outline is drawn.
    let mut db = g7();
    for e in &mut db.entities {
        if let Entity::Circle(c) = e {
            c.extrusion = uncad_model::Point3D {
                x: 1.0,
                y: 0.0,
                z: 1.0,
            };
        }
    }
    let r = hit_test(&db, p(173.0, -50.0), 0.5);
    assert!(
        r.not_searched.iter().any(
            |n| n.entity_type == "CIRCLE" && n.reason == NotSearchedReason::NonSimilarPlacement
        ),
        "{:?}",
        r.not_searched
    );
}

#[test]
fn a_centered_caption_is_found_at_the_point_it_is_centered_on() {
    // G7's caption is centered on (140, -67); the start point the file
    // states, (128.8, -68.75), is where the writing program computed it to
    // begin. Pointing at the middle of the text finds it.
    let r = hit_test(&g7(), p(140.0, -67.0), 0.5);
    let caption: Vec<&Hit> = r.hits.iter().filter(|h| h.entity_type == "TEXT").collect();
    assert_eq!(caption.len(), 1, "{:?}", r.hits);
    assert!(caption[0].anchored);
    assert_eq!(caption[0].distance, 0.0);
}

#[test]
fn an_aligned_text_is_found_anywhere_along_its_baseline() {
    // A text aligned from (0, 0) to (10, 0) runs along that baseline:
    // pointing at its middle, far from both ends, still finds it.
    let mut db = g7();
    for e in &mut db.entities {
        if let Entity::Text(t) = e {
            if t.text == "PLATE" {
                t.start_point = p(0.0, 0.0);
                t.alignment_point = Some(p(10.0, 0.0));
                t.horizontal_justification = uncad_model::model::HorizontalJustification::Aligned;
                t.vertical_justification = uncad_model::model::VerticalJustification::Baseline;
            }
        }
    }
    let r = hit_test(&db, p(5.0, 0.3), 0.5);
    let found: Vec<&Hit> = r.hits.iter().filter(|h| h.entity_type == "TEXT").collect();
    assert_eq!(found.len(), 1, "{:?}", r.hits);
    assert!((found[0].distance - 0.3).abs() < 1e-12, "{:?}", found[0]);
}

#[test]
fn a_mirror_copied_text_is_found_where_it_is_drawn() {
    // G7's mirror copy of a text: written at (-170, -72) with extrusion
    // (0, 0, -1), anchored at (170, -72) in the world.
    let r = hit_test(&g7(), p(170.0, -72.0), 0.1);
    assert!(types_hit(&r).contains(&"TEXT"), "{:?}", r.hits);
    let r = hit_test(&g7(), p(-170.0, -72.0), 0.1);
    assert!(!types_hit(&r).contains(&"TEXT"), "{:?}", r.hits);
}
