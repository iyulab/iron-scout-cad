# Changelog

Notable changes to this project are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versioning follows
[Semantic Versioning](https://semver.org/). While the version is 0.x, a breaking change
bumps the minor version.

## [Unreleased]

### Changed

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
