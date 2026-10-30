//! Layer 2 of the shape signature: the views a sheet holds and, when one is
//! drawn, the sheet frame they sit in.
//!
//! A drawing of a part usually puts several views of it on one sheet. Layer 1
//! counts them as one lump; this layer separates them, by a rule that needs
//! no interpretation:
//!
//! - An *item* is a top-level entity of model space that layer 1 measures (a
//!   line, a circle, an arc, a polyline, an ellipse, a spline, or a block
//!   reference without attributes, which is one item whatever it draws), with
//!   the box around the points it is measured by ([`crate::SpaceExtent`]'s
//!   rule), in whole micrometres.
//! - Two items are *joined* when their boxes lie within [`VIEW_GAP_UM`] of each
//!   other on both axes (touching or overlapping included). A *view* is a set
//!   of items joined directly or through others. Every integer below is a
//!   function of the set of item boxes: order, handles and layers play no part.
//! - A *sheet frame* is a top-level closed polyline of four straight sides
//!   along the axes whose box is the drawing space of an A-series sheet
//!   ([`SheetFit`]). It is a fact about sizes, not a judgement that the
//!   polyline is a frame: the caller decides what it is. When a frame is
//!   found, only the items inside it form views; the frame itself and the
//!   items outside it do not. A frame drawn with line segments, or inside a
//!   block, is not found: its sides are items like the rest, they join one
//!   another into a view of their own (the sheet's, the largest), and the
//!   views inside it are the ones they would have been.
//!
//! A view is described by its size, never its place, so moving a view on the
//! sheet or copying the part to another sheet changes nothing here.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// How far apart, in micrometres on either axis, two item boxes may lie and
/// still belong to one view. A parameter of the quantization: a drawing
/// standard does not state a distance between views, and a different value is
/// a different [`SIGNATURE_VERSION`](crate::SIGNATURE_VERSION).
pub const VIEW_GAP_UM: u64 = 5_000;

/// The most views a signature lists, largest first; the rest are counted in
/// [`Signature::views_truncated`](crate::Signature::views_truncated).
pub const MAX_VIEWS: usize = 8;

/// The most items the view walk takes. A drawing with more is not separated
/// into views (`views` is `None` and `not_measured` says so): the walk joins
/// boxes pairwise along one axis, which is quadratic in the worst case.
pub const MAX_ITEMS: usize = 30_000;

/// A view: one set of joined items, described by what it holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct View {
    /// The box around the view's items: width and height in micrometres.
    pub bbox_um: [u64; 2],
    /// Geometry by entity type name, as layer 1 counts it, restricted to the
    /// view's items.
    pub entities: BTreeMap<String, u64>,
    /// Circle diameters in micrometres, each with how many circles have it.
    pub circle_diameters_um: BTreeMap<u64, u64>,
    /// The share of the view's box the circles cover, in parts per million:
    /// the sum of the circles' areas over the box's area, rounded.
    pub hole_ratio_ppm: u64,
}

/// A closed polyline that is the drawing space of an A-series sheet, and
/// every way it can be.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SheetMatch {
    /// The polyline's box: width and height in micrometres.
    pub frame_um: [u64; 2],
    /// Every sheet, margin and scale under which the box is the drawing
    /// space, in a fixed order: papers largest first, landscape before
    /// portrait, margins as listed at [`SheetFit`], scales enlarging first.
    /// More than one is possible: a size does not tell scales apart.
    pub fits: Vec<SheetFit>,
    /// How many items lie outside the frame and so belong to no view.
    pub items_outside: u64,
}

/// One way a frame is the drawing space of a sheet.
///
/// The sheets are ISO 216's A0 to A4 trimmed sizes, landscape (the long side
/// across) or portrait. The drawing space is the sheet less its borders, in
/// one of two arrangements: `"10"` -- 10 mm on every side -- and `"20+10"` --
/// 20 mm on the left, for filing, and 10 mm on the others. The scale is a
/// ratio of ISO 5455's preferred series (`"1:1"`, `"1:2"`, `"1:5"` ...,
/// `"2:1"` ...): the frame is the drawing space times the real length over
/// the drawn length. A box is a fit when each side is within 0.2 % of the
/// drawing space.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SheetFit {
    pub paper: String,
    pub orientation: String,
    pub margins: String,
    pub scale: String,
}

/// An item's box in whole micrometres.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Rect {
    pub min_x: i64,
    pub min_y: i64,
    pub max_x: i64,
    pub max_y: i64,
}

impl Rect {
    fn width(&self) -> u64 {
        (self.max_x - self.min_x) as u64
    }

    fn height(&self) -> u64 {
        (self.max_y - self.min_y) as u64
    }

    fn area(&self) -> u128 {
        u128::from(self.width()) * u128::from(self.height())
    }

    fn union(&self, other: &Rect) -> Rect {
        Rect {
            min_x: self.min_x.min(other.min_x),
            min_y: self.min_y.min(other.min_y),
            max_x: self.max_x.max(other.max_x),
            max_y: self.max_y.max(other.max_y),
        }
    }

    fn within(&self, outer: &Rect) -> bool {
        self.min_x >= outer.min_x
            && self.min_y >= outer.min_y
            && self.max_x <= outer.max_x
            && self.max_y <= outer.max_y
    }

    /// Whether the gap between the boxes is at most `gap` on both axes.
    fn joins(&self, other: &Rect, gap: i64) -> bool {
        self.min_x <= other.max_x + gap
            && other.min_x <= self.max_x + gap
            && self.min_y <= other.max_y + gap
            && other.min_y <= self.max_y + gap
    }
}

/// One top-level entity layer 1 measures, with what it counted.
pub(crate) struct Item {
    pub rect: Rect,
    pub entities: BTreeMap<String, u64>,
    pub circle_diameters_um: BTreeMap<u64, u64>,
    /// Whether this item is a closed four-sided polyline along the axes: a
    /// candidate for a sheet frame.
    pub frame_candidate: bool,
}

/// What layer 2 adds to a signature.
pub(crate) struct Layer2 {
    pub views: Vec<View>,
    pub truncated: u64,
    pub sheet_match: Option<SheetMatch>,
}

const PAPERS: [(&str, u64, u64); 5] = [
    ("A0", 841, 1189),
    ("A1", 594, 841),
    ("A2", 420, 594),
    ("A3", 297, 420),
    ("A4", 210, 297),
];

/// (label, millimetres taken off the width, millimetres taken off the height)
/// of the sheet's side the filing margin is on, which is the left of a sheet
/// in either orientation.
const MARGINS: [(&str, u64, u64); 2] = [("10", 20, 20), ("20+10", 30, 20)];

/// (drawn, real) of ISO 5455's preferred scales, enlarging first.
const SCALES: [(u64, u64); 18] = [
    (50, 1),
    (20, 1),
    (10, 1),
    (5, 1),
    (2, 1),
    (1, 1),
    (1, 2),
    (1, 5),
    (1, 10),
    (1, 20),
    (1, 50),
    (1, 100),
    (1, 200),
    (1, 500),
    (1, 1000),
    (1, 2000),
    (1, 5000),
    (1, 10_000),
];

/// Every way a box of `width` by `height` micrometres is a drawing space.
fn fits(width: u64, height: u64) -> Vec<SheetFit> {
    let near = |actual: u64, expected: u128| {
        let actual = u128::from(actual);
        actual.abs_diff(expected) * 500 <= expected
    };
    let mut out = Vec::new();
    for (paper, short, long) in PAPERS {
        for (orientation, sheet_w, sheet_h) in
            [("landscape", long, short), ("portrait", short, long)]
        {
            for (margins, off_w, off_h) in MARGINS {
                let space_w = u128::from((sheet_w - off_w) * 1000);
                let space_h = u128::from((sheet_h - off_h) * 1000);
                for (drawn, real) in SCALES {
                    let expect = |space: u128| space * u128::from(real) / u128::from(drawn);
                    if near(width, expect(space_w)) && near(height, expect(space_h)) {
                        out.push(SheetFit {
                            paper: paper.to_string(),
                            orientation: orientation.to_string(),
                            margins: margins.to_string(),
                            scale: format!("{drawn}:{real}"),
                        });
                    }
                }
            }
        }
    }
    out
}

struct Joined {
    parent: Vec<usize>,
}

impl Joined {
    fn new(n: usize) -> Self {
        Joined {
            parent: (0..n).collect(),
        }
    }

    fn find(&mut self, mut i: usize) -> usize {
        while self.parent[i] != i {
            self.parent[i] = self.parent[self.parent[i]];
            i = self.parent[i];
        }
        i
    }

    fn join(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.parent[a.max(b)] = a.min(b);
        }
    }
}

/// The views of `items`, and the sheet frame among them.
pub(crate) fn separate(items: &[Item]) -> Layer2 {
    // The frame: of the candidates that fit a sheet, the largest box, the
    // lowest corner on a tie.
    let frame = items
        .iter()
        .enumerate()
        .filter(|(_, item)| item.frame_candidate)
        .filter_map(|(at, item)| {
            let found = fits(item.rect.width(), item.rect.height());
            (!found.is_empty()).then_some((at, found))
        })
        .max_by(|(a, _), (b, _)| {
            let (ra, rb) = (&items[*a].rect, &items[*b].rect);
            ra.area()
                .cmp(&rb.area())
                .then(rb.min_x.cmp(&ra.min_x))
                .then(rb.min_y.cmp(&ra.min_y))
                .then(b.cmp(a))
        });
    let frame_rect = frame.as_ref().map(|(at, _)| items[*at].rect);

    let members: Vec<usize> = (0..items.len())
        .filter(|at| match (&frame, frame_rect) {
            (Some((frame_at, _)), Some(outer)) => {
                *at != *frame_at && items[*at].rect.within(&outer)
            }
            _ => true,
        })
        .collect();
    let outside = items.len() - members.len() - usize::from(frame.is_some());

    let gap = VIEW_GAP_UM as i64;
    let mut joined = Joined::new(items.len());
    let mut order = members.clone();
    order.sort_by_key(|&at| (items[at].rect.min_x, items[at].rect.min_y, at));
    // Along x, an item can only join items whose box reaches back to it.
    let mut active: Vec<usize> = Vec::new();
    for &at in &order {
        active.retain(|&other| items[other].rect.max_x + gap >= items[at].rect.min_x);
        for &other in &active {
            if joined.find(other) != joined.find(at)
                && items[at].rect.joins(&items[other].rect, gap)
            {
                joined.join(other, at);
            }
        }
        active.push(at);
    }

    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for &at in &members {
        let root = joined.find(at);
        groups.entry(root).or_default().push(at);
    }
    let mut views: Vec<(Rect, View)> = groups
        .into_values()
        .map(|group| {
            let rect = group
                .iter()
                .map(|&at| items[at].rect)
                .reduce(|a, b| a.union(&b))
                .expect("a group holds an item");
            let mut entities: BTreeMap<String, u64> = BTreeMap::new();
            let mut diameters: BTreeMap<u64, u64> = BTreeMap::new();
            for &at in &group {
                for (name, n) in &items[at].entities {
                    *entities.entry(name.clone()).or_insert(0) += n;
                }
                for (d, n) in &items[at].circle_diameters_um {
                    *diameters.entry(*d).or_insert(0) += n;
                }
            }
            let covered: f64 = diameters
                .iter()
                .map(|(d, n)| std::f64::consts::FRAC_PI_4 * (*d as f64) * (*d as f64) * (*n as f64))
                .sum();
            let box_area = rect.area() as f64;
            let hole_ratio_ppm = if box_area > 0.0 {
                (covered / box_area * 1e6).round() as u64
            } else {
                0
            };
            (
                rect,
                View {
                    bbox_um: [rect.width(), rect.height()],
                    entities,
                    circle_diameters_um: diameters,
                    hole_ratio_ppm,
                },
            )
        })
        .collect();
    // Largest box first; a place settles a tie so the order does not depend
    // on the order of the entities.
    views.sort_by(|(a, _), (b, _)| {
        b.area()
            .cmp(&a.area())
            .then(a.min_x.cmp(&b.min_x))
            .then(a.min_y.cmp(&b.min_y))
    });
    let truncated = views.len().saturating_sub(MAX_VIEWS) as u64;
    views.truncate(MAX_VIEWS);

    Layer2 {
        views: views.into_iter().map(|(_, v)| v).collect(),
        truncated,
        sheet_match: frame.map(|(at, found)| SheetMatch {
            frame_um: [items[at].rect.width(), items[at].rect.height()],
            fits: found,
            items_outside: outside as u64,
        }),
    }
}
