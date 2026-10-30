//! Layer 2 of the shape signature, checked on the sheet of three views (a
//! frame, a front view with two holes, the top view below it, a side view
//! beside it, and a line past the frame) and on edits of it made directly in
//! the model.

use iron_scout_cad::{signature, SheetFit, Signature, VIEW_GAP_UM};
use std::collections::BTreeMap;
use uncad_model::model::Entity;
use uncad_model::CadDatabase;

fn g22() -> CadDatabase {
    serde_json::from_str(include_str!("golden/g22.expected.json"))
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

fn counts(pairs: &[(&str, u64)]) -> BTreeMap<String, u64> {
    pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
}

fn is_a3_with_a_filing_margin_at_full_size(fit: &SheetFit) -> bool {
    fit.paper == "A3"
        && fit.orientation == "landscape"
        && fit.margins == "20+10"
        && fit.scale == "1:1"
}

/// Moves by `dx` every line whose two ends lie in the given ranges.
fn shift_lines_with_ends_in(
    db: &mut CadDatabase,
    x_range: std::ops::RangeInclusive<f64>,
    y_range: std::ops::RangeInclusive<f64>,
    dx: f64,
) {
    for e in model_space(db).iter_mut() {
        if let Entity::Line(l) = e {
            let inside = |x: f64, y: f64| x_range.contains(&x) && y_range.contains(&y);
            if inside(l.start_point.x, l.start_point.y) && inside(l.end_point.x, l.end_point.y) {
                l.start_point.x += dx;
                l.end_point.x += dx;
            }
        }
    }
}

#[test]
fn a_sheet_of_three_views_gives_three_views_largest_first() {
    let s = signature(&g22());
    let views = s.views.expect("the drawing states its unit");
    assert_eq!(s.view_gap_um, VIEW_GAP_UM);
    assert_eq!(s.views_truncated, 0);
    assert_eq!(views.len(), 3);

    // Front: 120 by 60 mm, two holes of 10 mm.
    assert_eq!(views[0].bbox_um, [120_000, 60_000]);
    assert_eq!(
        views[0].entities,
        counts(&[
            ("CIRCLE", 2),
            ("LWPOLYLINE", 1),
            ("POLYLINE_LINE_SEGMENT", 4)
        ])
    );
    assert_eq!(
        views[0].circle_diameters_um,
        [(10_000, 2)].into_iter().collect()
    );
    // 2 * pi/4 * 10^2 mm^2 over 120 * 60 mm^2.
    assert_eq!(views[0].hole_ratio_ppm, 21_817);

    // Top: 120 by 40 mm, the outline and two hidden lines, no circle.
    assert_eq!(views[1].bbox_um, [120_000, 40_000]);
    assert_eq!(
        views[1].entities,
        counts(&[("LINE", 2), ("LWPOLYLINE", 1), ("POLYLINE_LINE_SEGMENT", 4)])
    );
    assert!(views[1].circle_diameters_um.is_empty());
    assert_eq!(views[1].hole_ratio_ppm, 0);

    // Side: 40 by 60 mm, five lines.
    assert_eq!(views[2].bbox_um, [40_000, 60_000]);
    assert_eq!(views[2].entities, counts(&[("LINE", 5)]));
}

#[test]
fn the_frame_is_a_sheet_the_size_says_and_the_line_past_it_belongs_to_no_view() {
    let s = signature(&g22());
    let sheet = s.sheet_match.expect("a frame");
    assert_eq!(sheet.frame_um, [390_000, 277_000]);
    assert!(
        sheet
            .fits
            .iter()
            .any(is_a3_with_a_filing_margin_at_full_size),
        "{:?}",
        sheet.fits
    );
    assert_eq!(sheet.items_outside, 1);
    // The frame is not a view of its own, nor the line past it.
    let in_views: u64 = s
        .views
        .unwrap()
        .iter()
        .map(|v| v.entities.values().sum::<u64>())
        .sum();
    let total: u64 = s.entities.values().sum();
    // Layer 1 counts the frame (a polyline and its four sides) and the
    // line past it as well.
    assert_eq!(total - in_views, 5 + 1);
}

#[test]
fn moving_a_view_on_the_sheet_changes_nothing_in_the_signature() {
    let before = g22();
    let mut after = before.clone();
    // The side view, 20 mm to the right and still inside the frame.
    shift_lines_with_ends_in(&mut after, 200.0..=240.0, 150.0..=210.0, 20.0);
    let (a, b) = (signature(&before), signature(&after));
    assert_eq!(a.views, b.views);
    assert_eq!(a.sheet_match, b.sheet_match);
}

#[test]
fn the_order_of_the_entities_does_not_matter() {
    let before = g22();
    let mut after = before.clone();
    model_space(&mut after).reverse();
    let (a, b) = (signature(&before), signature(&after));
    assert_eq!(a.views, b.views);
    assert_eq!(a.sheet_match, b.sheet_match);
}

#[test]
fn one_hole_made_larger_differs_in_one_view_alone() {
    let before = g22();
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
    let (va, vb) = (a.views.unwrap(), b.views.unwrap());
    assert_eq!(va.len(), vb.len());
    assert_ne!(va[0].circle_diameters_um, vb[0].circle_diameters_um);
    assert_ne!(va[0].hole_ratio_ppm, vb[0].hole_ratio_ppm);
    assert_eq!(va[0].bbox_um, vb[0].bbox_um);
    assert_eq!(va[1..], vb[1..]);
    assert_eq!(a.sheet_match, b.sheet_match);
}

#[test]
fn a_frame_drawn_with_lines_is_not_found_and_the_views_inside_it_stay() {
    let before = g22();
    let mut after = before.clone();
    // Draw the frame as four lines instead of one closed polyline.
    let entities = model_space(&mut after);
    let at = entities
        .iter()
        .position(|e| {
            matches!(e, Entity::LwPolyline(p) if p.vertices.len() == 4
            && p.vertices[0].point.x == 20.0 && p.vertices[0].point.y == 10.0)
        })
        .expect("the frame");
    let mut line = entities
        .iter()
        .find(|e| matches!(e, Entity::Line(_)))
        .expect("a line")
        .clone();
    entities.remove(at);
    for (x0, y0, x1, y1) in [
        (20.0, 10.0, 410.0, 10.0),
        (410.0, 10.0, 410.0, 287.0),
        (410.0, 287.0, 20.0, 287.0),
        (20.0, 287.0, 20.0, 10.0),
    ] {
        if let Entity::Line(l) = &mut line {
            l.start_point.x = x0;
            l.start_point.y = y0;
            l.end_point.x = x1;
            l.end_point.y = y1;
        }
        entities.push(line.clone());
    }
    let (a, b): (Signature, Signature) = (signature(&before), signature(&after));
    assert!(a.sheet_match.is_some());
    assert_eq!(b.sheet_match, None);
    // The four sides join into a view of their own, the sheet's, and the
    // part views are the ones they were -- with the line past the old frame
    // now a view too.
    let (va, vb) = (a.views.unwrap(), b.views.unwrap());
    assert_eq!(vb.len(), 5);
    assert_eq!(vb[0].bbox_um, [390_000, 277_000]);
    assert_eq!(vb[0].entities, counts(&[("LINE", 4)]));
    for view in &va {
        assert!(vb.contains(view), "{view:?}");
    }
}

#[test]
fn without_a_stated_unit_there_are_no_views() {
    let mut db = g22();
    db.header.insunits = None;
    let s = signature(&db);
    assert_eq!(s.views, None);
    assert_eq!(s.sheet_match, None);
    assert_eq!(s.views_truncated, 0);
}

#[test]
fn views_past_the_limit_are_counted_not_listed() {
    let mut db = g22();
    // Without the frame, nothing is left out of the views.
    model_space(&mut db)
        .retain(|e| !matches!(e, Entity::LwPolyline(p) if p.vertices[0].point.x == 20.0));
    // Eight more lines, each far from everything else.
    let template = model_space(&mut db)
        .iter()
        .find_map(|e| match e {
            Entity::Line(l) => Some(l.clone()),
            _ => None,
        })
        .expect("a line");
    for n in 0..8 {
        let mut line = template.clone();
        line.start_point.x = 1000.0 + 100.0 * f64::from(n);
        line.start_point.y = 0.0;
        line.end_point.x = 1010.0 + 100.0 * f64::from(n);
        line.end_point.y = 0.0;
        model_space(&mut db).push(Entity::Line(line));
    }
    let s = signature(&db);
    assert_eq!(s.views.unwrap().len(), 8);
    // Three views of the sheet, the line that was past the frame, and eight
    // more: twelve, of which eight are listed.
    assert_eq!(s.views_truncated, 12 - 8);
}

/// The second moments are worked by hand from the outline each view draws,
/// about the centre of its box, over the box's size.
///
/// The front view is a 120 by 60 rectangle: along its sides E[x^2] is
/// (2 * 120 * 120^2 / 12 + 2 * 60 * 60^2) / 360 = 2000 mm^2 = 0.138889 w^2 and
/// E[y^2] is (2 * 120 * 30^2 + 2 * 60 * 60^2 / 12) / 360 = 700 mm^2 =
/// 0.194444 h^2; the product vanishes by symmetry. The two holes are circles
/// and do not weigh.
#[test]
fn the_outline_of_each_view_is_weighed_by_length() {
    let views = signature(&g22()).views.unwrap();
    assert_eq!(views[0].outline_moments, Some([138_889, 194_444, 0]));
    // The top view adds two vertical lines 30 mm either side of the centre.
    assert_eq!(views[1].outline_moments, Some([112_500, 183_333, 0]));
    // The side view adds a line across its middle.
    assert_eq!(views[2].outline_moments, Some([166_667, 125_000, 0]));
}

fn scaled_by_two(mut db: CadDatabase) -> CadDatabase {
    for e in model_space(&mut db).iter_mut() {
        match e {
            Entity::Line(l) => {
                for p in [&mut l.start_point, &mut l.end_point] {
                    p.x *= 2.0;
                    p.y *= 2.0;
                }
            }
            Entity::Circle(c) => {
                c.center.x *= 2.0;
                c.center.y *= 2.0;
                c.radius *= 2.0;
            }
            Entity::LwPolyline(p) => {
                for v in &mut p.vertices {
                    v.point.x *= 2.0;
                    v.point.y *= 2.0;
                }
            }
            _ => {}
        }
    }
    db
}

#[test]
fn the_same_shape_twice_the_size_has_the_same_moments() {
    let (a, b) = (signature(&g22()), signature(&scaled_by_two(g22())));
    let (va, vb) = (a.views.unwrap(), b.views.unwrap());
    // The frame no longer fits a sheet at this size, so it is a view of its
    // own: the three views of the part are the next three by size.
    let part = |views: &[iron_scout_cad::View]| -> Vec<Option<[i64; 3]>> {
        let mut found: Vec<_> = views
            .iter()
            .filter(|v| v.bbox_um[0] <= 240_000 && v.bbox_um[1] <= 120_000)
            .map(|v| (v.bbox_um, v.outline_moments))
            .collect();
        found.sort_by_key(|(bbox, _)| std::cmp::Reverse(bbox[0] * bbox[1]));
        found.into_iter().map(|(_, m)| m).collect()
    };
    assert_eq!(part(&va).len(), 3);
    assert_eq!(part(&va), part(&vb));
}

/// An upper semicircle of radius 10 from a bulged polyline segment: about the
/// centre of its box (20 by 10), E[x^2] = r^2 / 2 = 50 = 0.125 w^2,
/// E[(y - 5)^2] = 50 - 10 * (2r / pi) + 25 = 11.338 mm^2 = 0.11338 h^2 and the
/// product vanishes.
#[test]
fn an_arc_is_weighed_along_its_length() {
    let mut db = g22();
    let entities = model_space(&mut db);
    entities.retain(|e| matches!(e, Entity::LwPolyline(p) if p.vertices[0].point.x == 50.0 && p.vertices[0].point.y == 150.0));
    let Entity::LwPolyline(p) = &mut entities[0] else {
        panic!("a polyline");
    };
    let mut start = p.vertices[0];
    start.point.x = 10.0;
    start.point.y = 0.0;
    start.bulge = 1.0;
    let mut end = p.vertices[1];
    end.point.x = -10.0;
    end.point.y = 0.0;
    end.bulge = 0.0;
    p.vertices = vec![start, end];
    p.closed = false;
    let views = signature(&db).views.unwrap();
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].bbox_um, [20_000, 10_000]);
    assert_eq!(views[0].outline_moments, Some([125_000, 113_380, 0]));
}

#[test]
fn a_view_of_circles_alone_has_no_outline_moments() {
    let mut db = g22();
    model_space(&mut db).retain(|e| matches!(e, Entity::Circle(_)));
    let views = signature(&db).views.unwrap();
    assert!(!views.is_empty());
    assert!(views.iter().all(|v| v.outline_moments.is_none()));
}
