# iron-scout-cad

Deterministic eyes for CAD drawings: turns a parsed 2D drawing into a compact semantic summary and resolves "this spot" into an entity reference.

Built as a tool to be handed to an agent. It contains no AI of its own.

## What it does

- **Summarize** — compress a drawing's entity model into a token-efficient semantic description.
- **Point** — hit-test a coordinate (or stroke) to a stable entity reference, so that "here" means a geometric entity rather than a pixel.
- **Relate** — associate dimensions and tolerances with the geometry they describe.

## What it is not

- Not a CAD application, and not a geometry kernel.
- Not a file parser. It consumes an already-parsed [uncad-model](https://github.com/iyulab/uncad-model) entity model.
- Not an ML library. It performs no inference — no OCR, no vision models, no LLM calls. Raster input is out of scope.
- Not an editor. It never modifies the model.
- Not a differ. It reads one drawing state, not two.

## Status

0.x. Two read-only verbs:

- `summarize(&db)` — entity counts by type, layers and block definitions with how much sits on each, every attribute value a block reference carries (by its tag), loose text pairs that read as label and value by position, and every dimension as the file states it -- what it measures, the measurement it recorded, the text it shows (a literal also as plain text) and where that text sits, a point the hit test finds it at; nothing is recomputed, so a text that disagrees with the measurement is listed beside it as written; and where each space is -- model space and every paper-space sheet, each on its own, as the box around the points the crate measures entities by (an arc's exact reach, a text's anchor, a block reference's insertion point), with the entity types it took no point from named (texts equally near a label are all listed, none picked). Labels are matched, and values returned, as plain text -- what the text reads without the codes it is written in (`%%uDWG NO` answers to `DWG NO`, `%%c32` comes back as `⌀32`; `%%nnn`, whose character depends on the font, stays as written) -- and every text in the summary carries the text as written beside it. A loose text written on a tilted plane is not paired -- where it is drawn in plan is not measured -- and is listed as unplaced instead. `Summary::attribute(tag)` and `Summary::labelled(label)` answer "what is the value of X" as exactly one value, none, or several listed — never a pick.
- `hit_test(&db, point, tolerance)` — every entity whose geometry passes within the tolerance, nearest first and never narrowed to one; closed entities that enclose the point, separately; and the entity types the crate cannot hit-test yet, named. What a block reference draws is searched where it is drawn (nested references compose), a hit inside a block names the chain of references it was reached through, and whatever could not be searched -- a reference to no block, a circle, an arc or a polyline with arc segments placed under a non-uniform scale, a circle, arc, polyline, text or block reference written on a tilted plane, nesting too deep -- is listed with its reason. A polyline's arc segments are measured along their arcs, and a mirror copy (an entity or a block reference written in its own plane facing down) where it is drawn; loose texts are paired by where they are drawn too. A text, whose extent the model does not carry, is measured to its anchor and marked so: its start point, the point it is aligned on, or -- for text stretched between the two -- the baseline between them. A text block (MTEXT) and a feature control frame (TOLERANCE) are measured to their insertion point, marked the same way. A dimension is measured where its block draws it -- lines, arrows and text, which the file already places in world coordinates -- and at the middle of its text and the point it was built on, which the file states beside the block; the nearest counts, and the point the summary gives for a dimension's text always finds it. A filled area (SOLID, TRACE) is measured along its outline in the format's corner order and encloses what lies inside, as does a masked area (WIPEOUT); a 3D face is measured along the edges it draws, seen from above; a leader along its segments when the file says they run straight, and a multileader along its leader lines -- its text or block content is not in the model. An entity the drawing hides -- itself, or through a hidden block reference -- is still a hit, marked `invisible`; whether it counts is the caller's call. Every space is searched at once -- model space and every paper-space sheet, each with coordinates of its own -- and each hit names the space it is in, so a caller pointing into one keeps that one's hits.

The same drawing gives the same output, byte for byte; the input is never modified; no output
carries a higher confidence than the entities it came from. The design rules are in
[docs/principles.md](docs/principles.md).

```rust
let db: uncad_model::CadDatabase = /* from a parser, or from its JSON */;
let summary = iron_scout_cad::summarize(&db);
match summary.attribute("DWGNO") {
    iron_scout_cad::Lookup::Unique(number) => println!("{number}"),
    iron_scout_cad::Lookup::Absent => println!("no such attribute"),
    iron_scout_cad::Lookup::Ambiguous(all) => println!("several: {all:?}"),
}
let at = iron_scout_cad::hit_test(&db, uncad_model::Point2D { x: 25.0, y: 20.0 }, 0.01);
for hit in &at.hits {
    println!("{:?} {} at {}", hit.id, hit.entity_type, hit.distance);
}
```

## License

MIT
