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
- Not an editor. It never modifies the model; see [iron-hand-cad](https://github.com/iyulab/iron-hand-cad) for that.
- Not a differ. Comparing two drawing states is [iron-diff-cad](https://github.com/iyulab/iron-diff-cad).

## Status

Pre-implementation. No code yet. The design principles are settled and documented in [docs/principles.md](docs/principles.md); read that before proposing anything.

## License

MIT
