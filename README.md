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

- `summarize(&db)` — entity counts by type, layers and block definitions with how much sits on each, every attribute value a block reference carries (by its tag), and loose text pairs that read as label and value by position (texts equally near a label are all listed, none picked). `Summary::attribute(tag)` and `Summary::labelled(label)` answer "what is the value of X" as exactly one value, none, or several listed — never a pick.
- `hit_test(&db, point, tolerance)` — every entity whose geometry passes within the tolerance, nearest first and never narrowed to one; closed entities that enclose the point, separately; and the entity types the crate cannot hit-test yet, named. What a block reference draws is searched where it is drawn (nested references compose), a hit inside a block names the chain of references it was reached through, and whatever could not be searched -- a reference to no block, a circle placed under a non-uniform scale, nesting too deep -- is listed with its reason.

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
