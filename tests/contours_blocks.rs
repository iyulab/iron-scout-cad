//! Which outlines close through block references: each instance of a block is
//! placed where its reference puts it, and a reference that cannot be placed
//! exactly is counted under the reason. The drawings are built here, with
//! their lengths worked out by hand.

use iron_scout_cad::{contours, ContourKind, Contours};
use serde_json::{json, Value};
use uncad_model::CadDatabase;

const JOIN_UM: u64 = 100;
const TAU: f64 = std::f64::consts::TAU;
const PI: f64 = std::f64::consts::PI;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

fn g26_json() -> Value {
    serde_json::from_str(include_str!("golden/g26.expected.json")).unwrap()
}

fn common(id: u64, layer: &str) -> Value {
    let mut c = g26_json()["entities"][0]["common"].clone();
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

fn arc(id: u64, layer: &str, centre: [f64; 2], r: f64, from: f64, to: f64) -> Value {
    json!({
        "type": "ARC", "common": common(id, layer),
        "center": {"x": centre[0], "y": centre[1], "z": 0.0}, "radius": r,
        "start_angle": from, "end_angle": to,
        "extrusion": {"x": 0.0, "y": 0.0, "z": 1.0},
    })
}

fn circle(id: u64, layer: &str, centre: [f64; 2], r: f64) -> Value {
    json!({
        "type": "CIRCLE", "common": common(id, layer),
        "center": {"x": centre[0], "y": centre[1], "z": 0.0}, "radius": r,
        "extrusion": {"x": 0.0, "y": 0.0, "z": 1.0},
    })
}

/// The square of side 10 with a corner at the origin, as four lines on layer
/// `layer`, with ids from `first`.
fn square(first: u64, layer: &str) -> Vec<Value> {
    let corners = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
    (0..4)
        .map(|i| line(first + i as u64, layer, corners[i], corners[(i + 1) % 4]))
        .collect()
}

struct Reference {
    id: u64,
    layer: &'static str,
    block: &'static str,
    at: [f64; 2],
    scale: [f64; 2],
    rotation: f64,
    extrusion: [f64; 3],
}

fn reference(id: u64, layer: &'static str, block: &'static str, at: [f64; 2]) -> Reference {
    Reference {
        id,
        layer,
        block,
        at,
        scale: [1.0, 1.0],
        rotation: 0.0,
        extrusion: [0.0, 0.0, 1.0],
    }
}

impl Reference {
    fn json(&self) -> Value {
        json!({
            "type": "INSERT", "common": common(self.id, self.layer),
            "block_name": {"type": "RESOLVED", "data": self.block},
            "insertion_point": {"x": self.at[0], "y": self.at[1], "z": 0.0},
            "scale": {"x": self.scale[0], "y": self.scale[1], "z": 1.0},
            "rotation": self.rotation,
            "attribs": [],
            "extrusion": {"x": self.extrusion[0], "y": self.extrusion[1], "z": self.extrusion[2]},
        })
    }
}

/// G26's header with `entities` in place of its drawing and `blocks` (name and
/// entities) as the drawing's block records.
fn drawing(entities: Vec<Value>, blocks: Vec<(&str, Vec<Value>)>) -> CadDatabase {
    let mut v = g26_json();
    v["entities"] = entities.into();
    for (name, inner) in blocks {
        v["tables"]["block_records"][name] = json!({
            "name": name, "entities": inner,
            "base_point": {"x": 0.0, "y": 0.0, "z": 0.0}, "external_reference": null,
        });
    }
    serde_json::from_value(v).unwrap()
}

fn counts(c: &Contours) -> (u32, u32, u32) {
    (c.loops, c.branched, c.open)
}

fn ids(c: &iron_scout_cad::Contour) -> Value {
    serde_json::to_value(&c.entities).unwrap()
}

/// A square drawn once in a block and referenced three times -- moved, scaled
/// and turned, and mirrored -- is three closed outlines, each as long as the
/// reference makes it, each on the layer of its reference (the block's lines
/// are on layer 0) and each owned by its reference.
#[test]
fn a_block_drawn_in_three_places_is_three_loops_each_placed_where_its_reference_puts_it() {
    let moved = reference(100, "CUT", "SQ", [100.0, 0.0]);
    let mut grown = reference(101, "BEND", "SQ", [0.0, 50.0]);
    grown.scale = [2.0, 2.0];
    grown.rotation = PI / 2.0;
    let mut mirrored = reference(102, "CUT", "SQ", [200.0, 0.0]);
    mirrored.scale = [-1.0, 1.0];
    let db = drawing(
        vec![moved.json(), grown.json(), mirrored.json()],
        vec![("SQ", square(1, "0"))],
    );
    let c = contours(&db, JOIN_UM).unwrap();
    assert_eq!(counts(&c), (3, 0, 0), "{:?}", c.components);
    assert!(c.not_read.is_empty(), "{:?}", c.not_read);
    let by_id = |id: &str| {
        c.components
            .iter()
            .find(|g| ids(g) == json!([id]))
            .unwrap_or_else(|| panic!("a group owned by {id}"))
    };
    let (a, b, m) = (by_id("100"), by_id("101"), by_id("102"));
    assert!(close(a.length_mm, 40.0) && close(a.length_mm_by_layer["CUT"], 40.0));
    assert!(close(b.length_mm, 80.0) && close(b.length_mm_by_layer["BEND"], 80.0));
    assert!(close(m.length_mm, 40.0) && a.pieces == 4 && b.pieces == 4);
    assert_eq!(b.layers, vec!["BEND".to_string()]);
    // The pieces stand where the reference puts them: the grown square's free
    // ends do not exist, and a drawn line shifted by the reference closes it.
    assert!(c.components.iter().all(|g| g.kind == ContourKind::Loop));
}

/// A block's own layer is kept where it is not 0, and a reference's pieces on
/// two layers are two owners of one entity, listed once.
#[test]
fn a_block_that_draws_on_two_layers_keeps_each_and_lists_its_reference_once() {
    let mut inner = square(1, "CUT");
    inner[3] = line(4, "BEND", [0.0, 10.0], [0.0, 0.0]);
    let db = drawing(
        vec![reference(100, "ANY", "TWO", [0.0, 0.0]).json()],
        vec![("TWO", inner)],
    );
    let c = contours(&db, JOIN_UM).unwrap();
    assert_eq!(c.components.len(), 1);
    let g = &c.components[0];
    assert_eq!(ids(g), json!(["100"]));
    assert_eq!(g.layers, vec!["BEND".to_string(), "CUT".to_string()]);
    assert!(close(g.length_mm_by_layer["CUT"], 30.0));
    assert!(close(g.length_mm_by_layer["BEND"], 10.0));
}

/// Lengths follow the scale: a circle and a half circle closed by its diameter
/// in a block scaled by 3.
#[test]
fn curves_in_a_scaled_block_are_as_long_as_the_scale_makes_them() {
    let mut r = reference(100, "CUT", "ROUND", [0.0, 0.0]);
    r.scale = [3.0, 3.0];
    let db = drawing(
        vec![r.json()],
        vec![(
            "ROUND",
            vec![
                circle(1, "0", [50.0, 0.0], 5.0),
                arc(2, "0", [0.0, 0.0], 5.0, 0.0, PI),
                line(3, "0", [-5.0, 0.0], [5.0, 0.0]),
            ],
        )],
    );
    let c = contours(&db, JOIN_UM).unwrap();
    assert_eq!(counts(&c), (2, 0, 0), "{:?}", c.components);
    let mut lengths: Vec<f64> = c.components.iter().map(|g| g.length_mm).collect();
    lengths.sort_by(f64::total_cmp);
    assert!(close(lengths[0], 3.0 * (5.0 * PI + 10.0)), "{lengths:?}");
    assert!(close(lengths[1], TAU * 5.0 * 3.0), "{lengths:?}");
}

/// A piece of an outline in the model space and the rest of it in a block
/// close one outline owned by both.
#[test]
fn an_outline_drawn_partly_in_a_block_closes_with_the_model_space_lines() {
    let db = drawing(
        vec![
            line(1, "CUT", [0.0, 10.0], [0.0, 0.0]),
            reference(100, "CUT", "THREE", [0.0, 0.0]).json(),
        ],
        vec![(
            "THREE",
            vec![
                line(10, "0", [0.0, 0.0], [10.0, 0.0]),
                line(11, "0", [10.0, 0.0], [10.0, 10.0]),
                line(12, "0", [10.0, 10.0], [0.0, 10.0]),
            ],
        )],
    );
    let c = contours(&db, JOIN_UM).unwrap();
    assert_eq!(counts(&c), (1, 0, 0), "{:?}", c.components);
    assert_eq!(ids(&c.components[0]), json!(["1", "100"]));
    assert!(close(c.components[0].length_mm, 40.0));
}

/// A block inside a block, each placed once: the outline is placed through
/// both.
#[test]
fn a_block_inside_a_block_is_placed_through_both() {
    let mut inner = reference(20, "0", "SQ", [100.0, 0.0]);
    inner.rotation = PI / 2.0;
    let mut outer = reference(100, "CUT", "OUTER", [0.0, 0.0]);
    outer.scale = [2.0, 2.0];
    let db = drawing(
        vec![outer.json()],
        vec![("SQ", square(1, "0")), ("OUTER", vec![inner.json()])],
    );
    let c = contours(&db, JOIN_UM).unwrap();
    assert_eq!(counts(&c), (1, 0, 0), "{:?}", c.components);
    assert!(close(c.components[0].length_mm, 80.0));
    assert_eq!(c.components[0].layers, vec!["CUT".to_string()]);
}

/// Whether a circle stays a circle is told by the placement a reference comes
/// to with the references around it: a block scaled evenly that holds a
/// stretched reference is read except for that reference.
#[test]
fn a_stretched_reference_inside_an_evenly_scaled_block_is_the_only_part_not_read() {
    let mut stretched = reference(20, "0", "SQ", [100.0, 0.0]);
    stretched.scale = [2.0, 1.0];
    let mut plain = square(40, "0");
    plain.push(stretched.json());
    let mut outer = reference(100, "CUT", "OUTER", [0.0, 0.0]);
    outer.scale = [2.0, 2.0];
    let db = drawing(
        vec![outer.json()],
        vec![("SQ", square(1, "0")), ("OUTER", plain)],
    );
    let c = contours(&db, JOIN_UM).unwrap();
    assert_eq!(counts(&c), (1, 0, 0), "{:?}", c.components);
    assert!(close(c.components[0].length_mm, 80.0));
    assert_eq!(c.not_read.get("INSERT_NON_UNIFORM_SCALE"), Some(&1));
    assert_eq!(c.not_read.len(), 1, "{:?}", c.not_read);
}

/// A reference that cannot be followed is counted under the reason, and what
/// it holds gives no piece.
#[test]
fn a_reference_that_cannot_be_followed_is_counted_under_the_reason() {
    let mut stretched = reference(100, "CUT", "SQ", [0.0, 0.0]);
    stretched.scale = [2.0, 1.0];
    let mut tilted = reference(101, "CUT", "SQ", [50.0, 0.0]);
    tilted.extrusion = [1.0, 0.0, 0.0];
    let missing = reference(102, "CUT", "NOWHERE", [0.0, 0.0]);
    let again = reference(103, "CUT", "LOOP", [0.0, 0.0]);
    let db = drawing(
        vec![
            stretched.json(),
            tilted.json(),
            missing.json(),
            again.json(),
        ],
        vec![
            ("SQ", square(1, "0")),
            ("LOOP", vec![reference(30, "0", "LOOP", [0.0, 0.0]).json()]),
        ],
    );
    let c = contours(&db, JOIN_UM).unwrap();
    assert_eq!(counts(&c), (0, 0, 0));
    assert!(c.components.is_empty());
    for (reason, how_many) in [
        ("INSERT_NON_UNIFORM_SCALE", 1),
        ("INSERT_TILTED_PLANE", 1),
        ("INSERT_UNRESOLVED", 1),
        ("INSERT_CYCLE", 1),
    ] {
        assert_eq!(
            c.not_read.get(reason),
            Some(&how_many),
            "{reason}: {:?}",
            c.not_read
        );
    }
    assert_eq!(c.not_read.len(), 4, "{:?}", c.not_read);
}

/// A reference sitting inside twenty others is not followed.
#[test]
fn references_nested_past_the_depth_bound_are_counted() {
    let mut blocks = vec![("SQ", square(1, "0"))];
    let names: Vec<&'static str> = (0..22)
        .map(|i| &*Box::leak(format!("B{i}").into_boxed_str()))
        .collect();
    for i in 0..22 {
        let next = if i == 21 { "SQ" } else { names[i + 1] };
        blocks.push((
            names[i],
            vec![reference(500 + i as u64, "0", next, [0.0, 0.0]).json()],
        ));
    }
    let db = drawing(
        vec![reference(100, "CUT", names[0], [0.0, 0.0]).json()],
        blocks,
    );
    let c = contours(&db, JOIN_UM).unwrap();
    assert_eq!(c.components.len(), 0);
    assert_eq!(
        c.not_read.get("INSERT_TOO_DEEP"),
        Some(&1),
        "{:?}",
        c.not_read
    );
}

/// What a block marks invisible (a hidden state of a dynamic block) is not
/// drawn: it neither opens an outline nor closes one, and is counted.
#[test]
fn an_entity_a_block_marks_invisible_is_not_read() {
    let mut inner = square(1, "0");
    inner.push(line(5, "0", [0.0, 0.0], [10.0, 10.0]));
    inner[4]["common"]["invisible"] = true.into();
    let db = drawing(
        vec![reference(100, "CUT", "SQ", [0.0, 0.0]).json()],
        vec![("SQ", inner)],
    );
    let c = contours(&db, JOIN_UM).unwrap();
    assert_eq!(counts(&c), (1, 0, 0), "{:?}", c.components);
    assert_eq!(c.components[0].pieces, 4);
    assert_eq!(c.not_read.get("INVISIBLE_IN_BLOCK"), Some(&1));
}

/// The same drawing in any order gives the same answer, references included.
#[test]
fn the_answer_does_not_depend_on_the_order_of_the_references() {
    let mut grown = reference(101, "BEND", "SQ", [0.0, 50.0]);
    grown.scale = [2.0, 2.0];
    let entities = vec![
        reference(100, "CUT", "SQ", [100.0, 0.0]).json(),
        grown.json(),
        line(7, "CUT", [300.0, 0.0], [310.0, 0.0]),
    ];
    let blocks = || vec![("SQ", square(1, "0"))];
    let forward = contours(&drawing(entities.clone(), blocks()), JOIN_UM).unwrap();
    let mut reversed = entities;
    reversed.reverse();
    assert_eq!(
        contours(&drawing(reversed, blocks()), JOIN_UM).unwrap(),
        forward
    );
}
