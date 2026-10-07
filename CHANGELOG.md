# Changelog

Notable changes to this project are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versioning follows
[Semantic Versioning](https://semver.org/). While the version is 0.x, a breaking change
bumps the minor version.

## [Unreleased]

### Added

- `BlockSummary::external_reference`: a block that states it is an external reference says which
  drawing it stands for (its path as written, and whether it is an overlay). That drawing's content
  is not in the file, so the block's `entity_count` counts only what was bound or cached into it.

## [0.9.0] - 2026-10-07

### Changed

- Built on `uncad-model` 0.8.0.

## [0.8.0] - 2026-10-07

### Changed

- Built on `uncad-model` 0.7.0.

## [0.7.0] - 2026-10-07

### Added

- `HitTest::table_cells`: for each table (ACAD_TABLE) whose grid holds the point -- in a space, or
  drawn by a block reference -- the row and column of the cell holding it (`TableCellHit`, with the
  chain of references it was reached through and its space), counted from the table's first row
  in the way its rows run and from its left column. A merged cell is named by its first row and
  column; a point on the edge between cells names each. A table whose grid or flow is not known is
  listed in `not_searched` with the new reason `NotSearchedReason::TableCellsUnknown` when the point
  is at it.
- `NotSearchedReason::BlockReferenceCycle`: a block reference that draws a block already being
  searched -- one that draws itself.

### Changed

- A block that draws itself is searched once, where the outer reference puts it, and the inner
  reference is reported as `BlockReferenceCycle` -- before, the search followed it again down to
  the depth limit, finding the same entities once per level. The hit test, the signature and the
  extents now decide in one place whether a block reference is followed.
- A space's extent (`Summary::extents`) and a selection's per-entity bounds (`select`) now take in
  what a block reference draws: the entities of its block, placed where the reference puts them
  (nested references compose), beside its insertion point. A table (ACAD_TABLE) is measured by its
  block the same way, and a dimension by its block of lines, arrows and text. Arcs, circles,
  ellipses and polyline arc segments are measured exactly under any placement -- turned, mirrored,
  or scaled differently along each axis. `bounded_by` still names the top-level entity, so a side
  set by something inside a block names the reference that draws it.
- An ordinate dimension's definition point (DXF 10) is no longer measured: it is the datum the
  dimension measures from, not a point it draws.
- `not_measured` also names entity types inside expanded blocks that give no point, and a block
  reference whose block could not be measured, by its type and the reason (`INSERT_UNRESOLVED`,
  `_CYCLE`, `_TOO_DEEP`, `_BUDGET_EXHAUSTED`, `_TILTED`); its insertion point is still measured.

- Built on `uncad-model` 0.6.0.

## [0.6.0] - 2026-10-05

### Added

- `Summary::tables`: every table (ACAD_TABLE) of the drawing's own spaces, by reference ID -- where
  it is inserted, the block that draws it, and its cells (`TableCells`: the row and column counts
  and every cell with text, by row and column, as written and as plain text, with its span).
  `cells` is `None` when the drawing's reader did not read the table's cells, so a table whose
  contents are unknown does not read as an empty one. Absent from the JSON when the drawing has no
  table.

### Changed

- `hit_test` searches a table as the block reference it is: the table is found at its insertion
  point, and what its block draws -- the lines and texts of its cells -- is hit through it (`via`
  names the table), placed as the table places it. A table is no longer listed as `unsupported`;
  one naming a block the drawing lacks is listed in `not_searched` with its reason.

- Built on `uncad-model` 0.5.0 (a table's grid).

## [0.5.0] - 2026-10-04

### Added

- A MULTILEADER is found at what it points out: its text's or block's location is an anchor, as an
  MTEXT's insertion point is, and a point nearer it than the leader lines meets the multileader
  there -- also when its lines are a curve the file does not define, which is not searched. The
  location counts in the drawing's extent.
- `Summary::drawing_ids` (`DrawingIds`): the header's `$FINGERPRINTGUID` and `$VERSIONGUID`
  as stated, each absent when not stated -- to ask whether two files are one drawing before
  comparing them. The fingerprint is kept by copies and by drawings made from one template: it
  says where a drawing came from, not which drawing it is.
- `Summary::signature` and `signature(&db)`: the drawing's shape signature -- what model space
  draws, block references expanded (those carrying attributes left out and named), counted into
  integers: geometry by type, circle diameters in micrometres, line lengths and arc radii in
  power-of-two millimetre bins, dimensions by kind, where dimensions state a tolerance, and what
  was not measured. Length components are absent when the header states no unit. The
  quantization is versioned (`signature_version` 1). Expansion is bounded: a reference nested
  inside 20 others (`INSERT_TOO_DEEP`), or whose block would take the walk past ten million
  entities met inside expanded blocks (`INSERT_BUDGET_EXHAUSTED`), is not followed and is counted
  as not measured.

### Changed

- The hit test's budget for following block references counts the entities met inside the
  blocks it expands (ten million in one call) instead of the references followed (a million): a
  block of many entities placed many times no longer runs unbounded. A block that does not fit
  in what is left is not entered (`BLOCK_REFERENCE_BUDGET_EXHAUSTED`); a reference whose
  placement is not measured no longer spends the budget.
- A MULTILEADER whose lines are a spline, or of a type the model does not know, is not
  searched (`CURVE_UNDEFINED`), as a LEADER's spline path already was. Lines of no type are not
  measured and not part of the extent; the doglegs still are.
- A layout's overall viewport -- the sheet as paper space shows it, not something drawn on it --
  is neither hit nor enclosing in a hit test, and is not listed as unsearched: as a closed frame
  it enclosed every point of the sheet. The other viewports are searched as before.
- Built on `uncad-model` 0.4.0 (a multileader's line type and content; the header's drawing
  identifiers).

## [0.4.0] - 2026-10-02

### Added

- `Summary::units`: the unit the drawing's header states, as a `DrawingUnits` (the
  `$INSUNITS` code and the name the DXF reference gives it, `"du"` for unitless or an
  undefined code); `None` -- `null` in JSON, always present -- when the header states none.
  Needs `uncad-model`'s `CadDatabase::header`.

### Changed

- Built on `uncad-model` 0.3.0 (its lower bound was 0.2.1): a multileader is pointed at
  along the lines its leader roots draw, and a summary reads the drawing's `header`.

## [0.3.0] - 2026-10-01

### Added

- `Selection`, `select` and `summarize_with`: a summary can carry the entities picked by
  type, layer, a window they reach into (crossing), space or reference ID -- each with its
  layer, space and box (the points the extents are measured by) and, with `with_detail`,
  its model record. `total` counts every entity kept; `limit` keeps the first by reference
  ID. An entity this crate takes no point from has no box, is left out of a window
  selection, and its type is named in `not_measured`. `Summary` gains `selection`, absent
  unless asked for.
- `SpaceExtent::bounded_by` names the entity that sets each side of the space's box (the
  lowest reference ID on a tie), so one stray entity far from the rest is told apart from the
  drawing's own extent. The box itself is unchanged: every entity measured still counts.
- `HitTest::limited(n)` keeps the `n` nearest hits; `HitTest` gains `hits_total`, present
  when hits were left out.
- `hit_test` measures more of what a drawing shows in plan, where it was listed in
  `unsupported` before: a RAY from its base point one way and an XLINE both ways (one that runs
  straight up is its base point), a 3D polyline seen from above, a viewport's frame on its
  sheet, a raster image's frame -- or its clip boundary, when clipping is on and keeps what is
  inside -- a light's position, and a REGION, 3DSOLID, polyface or polygon mesh whose edges all
  lie at one height. A closed frame encloses the points inside it. A body with depth, and an
  image with no size, stay in `unsupported`.
- `hit_test` measures a HATCH by its boundary paths, in the hatch's own plane: straight and
  bulged segments and arc edges exactly, elliptical and spline edges through chords (with
  `within`), and the area the paths bound -- islands alternating -- encloses the points inside
  it. A hatch on a tilted plane is `NON_SIMILAR_PLACEMENT`; one with a spline edge the file does
  not define, or a rational one, is `CURVE_UNDEFINED` or `CURVE_BOUND_UNKNOWN`.
- `hit_test` measures an MLINE by its lines: its style's offsets, at the MLINE's scale, from
  the centerline along each vertex's miter direction. `NotSearchedReason` gains
  `STYLE_UNDEFINED`, for an MLINE whose style is not in the drawing or whose scale is not
  known.
- A space's extent, and a window selection, take the same points as the hit test for the types
  it now measures: a hatch's boundary (the reach of its arcs and ellipses, a spline edge's
  control points), an MLINE's lines, a raster image's frame, a light's position, a 3D
  polyline's vertices and a body's edges when they lie at one height -- so what can be pointed
  at can also be found by where it is. A RAY or XLINE still has no box: it has no end.
- `DimensionSummary::length_factor`: the DIMLFAC in force for the dimension -- its style's, or
  its own override -- the factor its drawing distance is multiplied by for the measurement it
  shows. Absent when the style is not in the drawing or the dimension's overrides were not
  read.

### Changed

- The hit test's budget for following block references counts the entities met inside the
  blocks it expands (ten million in one call) instead of the references followed (a million): a
  block of many entities placed many times no longer runs unbounded. A block that does not fit
  in what is left is not entered (`BLOCK_REFERENCE_BUDGET_EXHAUSTED`); a reference whose
  placement is not measured no longer spends the budget.
- A MULTILEADER is pointed at, and boxed, where it is drawn: its lines on to their root's last
  leader line point, and its doglegs (`uncad-model`'s `MultiLeaderEntity::drawn_lines` and
  `doglegs`). A multileader whose lines were a single vertex each was `NO_GEOMETRY`.
- `HitTest::unsupported` names only types this crate does not measure. An entity of a type it
  does measure, but that the model gives nothing to measure -- a polyline with fewer than two
  vertices, a multileader whose every line is a single point, a body with no readable edge, a
  hatch with no boundary segment, an image frame that encloses nothing -- is listed in
  `not_searched` with the new reason `NO_GEOMETRY`, instead of its type in `unsupported`. A
  LEADER that does not say whether its path is straight or a spline is `CURVE_UNDEFINED`,
  like a spline one.

## [0.2.0] - 2026-09-29

### Changed

- **Breaking:** The result types (`Summary`, `HitTest`, `Hit`, `NotSearched`, `SpaceExtent`,
  `LayerSummary`, `BlockSummary`, `AttributeValue`, `TextRef`, `DimensionSummary`,
  `LabelledText`) and `NotSearchedReason` are `#[non_exhaustive]`: they are built by this crate
  and read by callers, so a field or reason added later is not a breaking change. Struct
  literals and exhaustive matches outside the crate no longer compile; read the fields, and
  add a wildcard arm.
- **Breaking:** `Summary::attribute` and `Summary::labelled` match and return plain text
  (see `plain_text`), not the string with its control codes: pass `Ø50` rather than
  `%%c50`, and expect the sign back. The text as written stays in `value` and `text`.
- **Breaking:** `Hit` gains `invisible`, `space` and `within`; `NotSearched` gains `space`;
  `TextRef` and `AttributeValue` gain `plain`; `Summary` gains `unplaced_texts`,
  `dimensions` and `extents`. Struct literals must set them; in JSON they are new keys.
- **Breaking:** `NotSearchedReason` gains `CurveUndefined` and `CurveBoundUnknown`; an
  exhaustive `match` needs arms for them.
- Built on the current `uncad-model` API, whose polyline vertex carries a point and a bulge.

### Added

- `Summary::unresolved_inserts` (`UnresolvedInsert`): every top-level block reference whose
  block the drawing does not hold -- a name the file never defines, or none -- with what it
  points at. Such a reference draws nothing and no `BlockSummary` counts it; it used to show
  only in `by_type`. Absent from the JSON form when empty.

- `Summary::tolerance_frames` lists every feature control frame (TOLERANCE, as
  `ToleranceFrame`) with its text as written, that text with its MTEXT codes read, its
  insertion point and its style. What its symbols and cells mean is not read.
- A dimension in the summary carries what is stated about its tolerances, each source as it
  is and none chosen over another: `style_tolerance` (its style's DIMTOL, DIMLIM, DIMTP, DIMTM
  and DIMTDEC, as `StyleTolerance`), `tolerance_overrides` (its own overrides of those
  variables; `null` when the reader did not read its overrides) and `text_stacks` (the
  `\S…;` stacks its literal text writes, as `TextStack`).
- `hit_test` finds dimensions (what their block draws, the middle of their text, the point
  they were built on), MTEXT and TOLERANCE (at their insertion point), SOLID, TRACE and
  WIPEOUT (along their outline, enclosing what lies inside), 3DFACE, LEADER and MULTILEADER.
- Ellipses and splines are hit-tested through chords of the curve; such a hit carries
  `within`, a bound on how far the curve can be from the chords. A curve the file does not
  define is listed as `CURVE_UNDEFINED`, a rational spline as `CURVE_BOUND_UNKNOWN`.
- A polyline's arc segments are measured along their arcs, and a closed polyline's inside
  includes or cuts away each arc's cap.
- Every hit, and every entity not searched, names the `space` it is in: model space or a
  paper-space sheet.
- `Hit::invisible` marks an entity the drawing hides, itself or through a hidden block
  reference. The entity is still reported.
- `Summary::dimensions` lists each dimension as the file states it: kind, recorded
  measurement, the text it shows, where that text sits and its style. Nothing is recomputed.
- `Summary::extents` gives, for each space, the box around the points entities are measured
  by (`SpaceExtent`, `Bounds`), and names the entity types it took no point from.
- `Summary::unplaced_texts` lists loose texts on a plane tilted out of the world's, which
  cannot be paired in plan, so a missing label is never silently absent.
- `plain_text` reads a TEXT or ATTRIB without its control codes: `%%d`, `%%p` and `%%c` as
  their signs, underline and overline switches dropped, `%%nnn` kept as written.

### Fixed

- A dimension's `plain` text reads its MTEXT codes: `\S+0.1^-0.05;` reads `+0.1/-0.05` and
  `\P` a space, where the codes were kept as written.
- An ARC whose start and end angles are equal is no longer taken for the whole circle. The
  format does not say whether such an arc is the whole circle or nothing, so it is left out of
  the extent (`not_measured`) and a point on its circle is answered with `CURVE_UNDEFINED`.
  Arc and ellipse sweeps and an ellipse's turning points now come from `uncad-model`, the same
  arithmetic every consumer of the model uses.
- A circle, arc or polyline written in its own plane is measured where it is drawn: a mirror
  copy through the model's coordinate system, and one on a tilted plane is listed as not
  searched rather than measured where it is not drawn.
- What a block reference draws is placed with the block's base point on the insertion point;
  a mirrored reference is searched where it is drawn, a tilted one listed as not searched.
- A text in a mirror copy's plane is pointed at and paired where it is drawn; aligned and fit
  text is measured to its alignment point or the baseline, not only to its start point.

## [0.1.0] - 2026-09-22

Initial release. Two read-only verbs over an [uncad-model](https://github.com/iyulab/uncad-model)
drawing. `summarize` gives entity counts by type, layers and block definitions with how much
sits on each, every attribute value a block reference carries, and loose text pairs that read
as label and value; `Summary::attribute` and `Summary::labelled` answer with exactly one value,
none, or several listed. `hit_test` returns every entity whose geometry passes within a
tolerance of a point, nearest first, the closed entities that enclose it, the entity types it
cannot hit-test yet, and whatever it could not search with the reason; block references are
followed where they are drawn. The same drawing gives the same output, byte for byte.
