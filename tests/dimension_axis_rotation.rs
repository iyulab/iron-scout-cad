//! What a dimension measures along, in the summary: the coordinate an
//! ordinate dimension measures (G16) and the angle a rotated linear
//! dimension is measured along (G25) -- both as the file states them, and
//! absent where the group means nothing.

use iron_scout_cad::summarize;
use uncad_model::model::OrdinateAxis;
use uncad_model::CadDatabase;

fn golden(json: &str) -> CadDatabase {
    serde_json::from_str(json).expect("the golden model deserializes")
}

#[test]
fn an_ordinate_dimension_says_which_coordinate_it_measures() {
    let db = golden(include_str!("golden/g16.expected.json"));
    let axes: Vec<_> = summarize(&db)
        .dimensions
        .iter()
        .map(|d| d.ordinate_axis)
        .collect();
    assert_eq!(
        axes,
        [
            Some(OrdinateAxis::X),
            Some(OrdinateAxis::X),
            Some(OrdinateAxis::X),
            Some(OrdinateAxis::Y),
            Some(OrdinateAxis::Y)
        ]
    );
}

#[test]
fn only_a_rotated_dimension_carries_a_rotation() {
    let summary = summarize(&golden(include_str!("golden/g25.expected.json")));
    for d in &summary.dimensions {
        match d.kind {
            Some(uncad_model::model::DimensionKind::Rotated) => {
                assert_eq!(d.rotation, Some(0.0), "{:?}", d.plain)
            }
            // The model holds 0 for a group the kind does not use; that 0 is not a statement.
            _ => assert_eq!(d.rotation, None, "{:?}", d.plain),
        }
        assert_eq!(d.ordinate_axis, None, "not an ordinate dimension");
    }
}

#[test]
fn a_rotation_the_file_states_is_reported_as_stated() {
    let mut value: serde_json::Value =
        serde_json::from_str(include_str!("golden/g25.expected.json")).unwrap();
    let rotated = value["entities"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|e| e["kind"] == "ROTATED")
        .expect("the case has a rotated dimension");
    rotated["rotation"] = serde_json::json!(0.5);
    let db: CadDatabase = serde_json::from_value(value).unwrap();
    let angles: Vec<_> = summarize(&db)
        .dimensions
        .iter()
        .filter_map(|d| d.rotation)
        .collect();
    assert!(angles.contains(&0.5), "{angles:?}");
}
