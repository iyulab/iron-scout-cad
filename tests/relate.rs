//! Relating, checked on the general part (G1): its overall dimensions stand
//! on the outline's corners, the hole-to-hole distance on the holes'
//! centres, the diameter on the hole's edge -- and what changes when the
//! geometry moves, is placed by a block reference, or is in another space.

use iron_scout_cad::{relate, Anchor, PointRole, Relate, RelationKind};
use uncad_model::model::{DimensionKind, Entity, EntityId, Ref};
use uncad_model::tables::BlockRecord;
use uncad_model::{CadDatabase, Point3D};

fn g1() -> CadDatabase {
    serde_json::from_str(include_str!("golden/g1.expected.json"))
        .expect("the golden model deserializes")
}

/// Each point of dimension `id` as (role, [(entity, relation, anchor, via)]).
type Related = Vec<(
    PointRole,
    Vec<(u64, RelationKind, Option<Anchor>, Vec<u64>)>,
)>;

fn related(r: &Relate, id: u64) -> Related {
    let d = r
        .dimensions
        .iter()
        .find(|d| d.id.value() == id)
        .unwrap_or_else(|| panic!("dimension {id} is reported"));
    d.points
        .iter()
        .map(|p| {
            (
                p.role,
                p.relations
                    .iter()
                    .map(|r| {
                        (
                            r.entity.value(),
                            r.relation,
                            r.anchor,
                            r.via.iter().map(|v| v.value()).collect(),
                        )
                    })
                    .collect(),
            )
        })
        .collect()
}

fn entity_mut(db: &mut CadDatabase, id: u64) -> &mut Entity {
    db.entities
        .iter_mut()
        .find(|e| e.common().id.value() == id)
        .unwrap_or_else(|| panic!("entity {id} is in the drawing"))
}

#[test]
fn the_parts_dimensions_stand_on_its_corners_centres_and_edge() {
    use Anchor::*;
    use RelationKind::*;
    let r = relate(&g1(), None).unwrap();
    // The outline (288) runs (0,0) (200,0) (200,100) (0,100).
    assert_eq!(
        related(&r, 293),
        [
            (
                PointRole::Extension1,
                vec![(288, AtPoint, Some(Vertex(0)), vec![])]
            ),
            (
                PointRole::Extension2,
                vec![(288, AtPoint, Some(Vertex(1)), vec![])]
            ),
        ]
    );
    assert_eq!(
        related(&r, 294),
        [
            (
                PointRole::Extension1,
                vec![(288, AtPoint, Some(Vertex(0)), vec![])]
            ),
            (
                PointRole::Extension2,
                vec![(288, AtPoint, Some(Vertex(3)), vec![])]
            ),
        ]
    );
    // Hole to hole: the two lower holes' centres.
    let centres = related(&r, 295);
    assert_eq!(centres.len(), 2);
    for (_, relations) in &centres {
        assert_eq!(relations.len(), 1, "{centres:?}");
        assert_eq!((relations[0].1, relations[0].2), (AtPoint, Some(Center)));
    }
    assert_ne!(centres[0].1[0].0, centres[1].1[0].0, "two different holes");
    // The diameter's two points are on the hole's edge, at none of its
    // defined points.
    let diameter = related(&r, 296);
    assert_eq!(diameter.len(), 2);
    assert_eq!(diameter[0].0, PointRole::Radial);
    assert_eq!(diameter[1].0, PointRole::Definition);
    for (_, relations) in &diameter {
        assert_eq!(relations.len(), 1, "{diameter:?}");
        assert_eq!((relations[0].1, relations[0].2), (OnCurve, None));
        assert_eq!(relations[0].0, centres[0].1[0].0, "the hole at (20, 20)");
    }
    assert!(r.external_references.is_empty() && r.unmodeled.is_empty());
    // The default tolerance: a millionth of the outline's 200.
    assert!((r.tolerance - 200e-6).abs() < 1e-15, "{}", r.tolerance);
}

#[test]
fn a_moved_corner_leaves_the_dimension_on_nothing() {
    let mut db = g1();
    let Entity::LwPolyline(outline) = entity_mut(&mut db, 288) else {
        panic!("288 is the outline")
    };
    // Up off the bottom edge: (200, 0) is then on no edge and at no corner.
    outline.vertices[1].point.y = 5.0;
    let r = relate(&db, None).unwrap();
    let overall = related(&r, 293);
    assert_eq!(overall[0].1.len(), 1);
    assert!(overall[1].1.is_empty(), "{overall:?}");
}

#[test]
fn a_point_on_an_edge_between_corners_is_on_the_curve() {
    let mut db = g1();
    let Entity::Dimension(d) = entity_mut(&mut db, 293) else {
        panic!("293 is a dimension")
    };
    d.points.extension2 = Some(Point3D {
        x: 120.0,
        y: 0.0,
        z: 0.0,
    });
    let r = relate(&db, None).unwrap();
    assert_eq!(
        related(&r, 293)[1].1,
        [(288, RelationKind::OnCurve, None, vec![])]
    );
}

#[test]
fn geometry_a_block_reference_places_is_related_through_it() {
    let mut db = g1();
    // The hole at (20, 20) moved into a block of its own, centred on the
    // block's base point, and placed back where it was.
    let hole_at =
        |e: &Entity| matches!(e, Entity::Circle(c) if c.center.x == 20.0 && c.center.y == 20.0);
    let index = db
        .entities
        .iter()
        .position(hole_at)
        .expect("the hole at (20, 20)");
    let Entity::Circle(mut hole) = db.entities.remove(index) else {
        unreachable!()
    };
    let hole_id = hole.common.id.value();
    hole.center = Point3D::default();
    db.tables.block_records.insert(
        "HOLE".to_string(),
        BlockRecord {
            name: "HOLE".to_string(),
            entities: vec![Entity::Circle(hole)],
            base_point: Point3D::default(),
            external_reference: None,
        },
    );
    let Some(Entity::Insert(mut insert)) = db
        .entities
        .iter()
        .find(|e| e.common().id.value() == 297)
        .cloned()
    else {
        panic!("297 is the title block's reference")
    };
    insert.common.id = EntityId::new(9000);
    insert.block_name = Ref::Resolved("HOLE".to_string());
    insert.insertion_point = Point3D {
        x: 20.0,
        y: 20.0,
        z: 0.0,
    };
    insert.attribs.clear();
    insert.scale = Point3D {
        x: 1.0,
        y: 1.0,
        z: 1.0,
    };
    insert.rotation = 0.0;
    insert.extrusion = Point3D {
        x: 0.0,
        y: 0.0,
        z: 1.0,
    };
    // The drawing's space holds what its entities are, as the reader fills
    // both.
    let space = db
        .tables
        .block_records
        .values_mut()
        .find(|b| b.name.eq_ignore_ascii_case("*Model_Space"))
        .expect("G1 has a model space");
    space.entities.retain(|e| e.common().id.value() != hole_id);
    space.entities.push(Entity::Insert(insert.clone()));
    db.entities.push(Entity::Insert(insert));
    let r = relate(&db, None).unwrap();
    // The reference's insertion point is not a point of the geometry: only
    // the circle it draws is related.
    assert_eq!(
        related(&r, 295)[0].1,
        [(
            hole_id,
            RelationKind::AtPoint,
            Some(Anchor::Center),
            vec![9000]
        )]
    );
    assert_eq!(related(&r, 296)[0].1.len(), 1);
    assert_eq!(related(&r, 296)[0].1[0].3, [9000]);
}

#[test]
fn a_block_standing_for_another_drawing_is_named_with_its_references() {
    let mut db = g1();
    let name = match &db.entities.iter().find(|e| e.common().id.value() == 297) {
        Some(Entity::Insert(i)) => i.block_name.resolved().expect("resolved").to_string(),
        _ => panic!("297 is the title block's reference"),
    };
    let block = db.tables.block_records.get_mut(&name).unwrap();
    block.external_reference = Some(uncad_model::tables::ExternalReference {
        path: "frame.dwg".to_string(),
        overlay: false,
    });
    let entities = block.entities.len();
    let r = relate(&db, None).unwrap();
    assert_eq!(r.external_references.len(), 1);
    let x = &r.external_references[0];
    assert_eq!(
        (x.block.as_str(), x.path.as_str(), x.entities),
        (name.as_str(), "frame.dwg", entities)
    );
    assert_eq!(x.inserts.len(), 1);
    assert_eq!(x.inserts[0].id.value(), 297);
}

#[test]
fn a_dimension_without_a_kind_names_no_points() {
    let mut db = g1();
    let Entity::Dimension(d) = entity_mut(&mut db, 293) else {
        panic!("293 is a dimension")
    };
    d.kind = None;
    let r = relate(&db, None).unwrap();
    let d = r.dimensions.iter().find(|d| d.id.value() == 293).unwrap();
    assert_eq!(d.kind, None);
    assert!(d.points.is_empty());
}

#[test]
fn a_kind_changes_which_points_are_the_geometrys() {
    let mut db = g1();
    let Entity::Dimension(d) = entity_mut(&mut db, 296) else {
        panic!("296 is a dimension")
    };
    d.kind = Some(DimensionKind::Radius);
    // A radius dimension's DXF 10 is the centre.
    d.definition_point = Some(Point3D {
        x: 20.0,
        y: 20.0,
        z: 0.0,
    });
    let r = relate(&db, None).unwrap();
    let radius = related(&r, 296);
    assert_eq!(radius[1].0, PointRole::Definition);
    assert_eq!(
        (radius[1].1[0].1, radius[1].1[0].2),
        (RelationKind::AtPoint, Some(Anchor::Center))
    );
}

#[test]
fn a_tolerance_is_positive_and_finite() {
    let db = g1();
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(relate(&db, Some(bad)).is_err(), "{bad}");
    }
    assert_eq!(relate(&db, Some(0.5)).unwrap().tolerance, 0.5);
}

#[test]
fn the_answer_does_not_depend_on_the_order_of_the_entities() {
    let db = g1();
    let mut reversed = db.clone();
    reversed.entities.reverse();
    let a = serde_json::to_string(&relate(&db, None).unwrap()).unwrap();
    let b = serde_json::to_string(&relate(&reversed, None).unwrap()).unwrap();
    assert_eq!(a, b);
}

#[test]
fn one_dimension_is_kept_with_what_is_said_about_the_drawing() {
    let db = g1();
    let whole = relate(&db, None).unwrap();
    let wanted = whole.dimensions[0].clone();
    let one = whole.clone().of_dimension(wanted.id);
    assert_eq!(one.dimensions, vec![wanted]);
    assert_eq!(one.tolerance, whole.tolerance);
    assert_eq!(one.unmodeled, whole.unmodeled);
    assert_eq!(one.external_references, whole.external_references);
    // An ID that is no dimension's keeps none.
    let other = db
        .entities
        .iter()
        .find(|e| !matches!(e, Entity::Dimension(_)))
        .expect("the part draws more than dimensions");
    assert!(whole.of_dimension(other.common().id).dimensions.is_empty());
}
