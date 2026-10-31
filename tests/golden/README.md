# Golden cases

Copies of what the `uncad-model` repository's golden writer produces, one expected model per case:

| File | What it is |
|---|---|
| `<case>.expected.json` | The model a reader must produce from that case's synthetic drawing; the spec is the oracle |

Cases here: `g1` (a general part with a title block), `g2` (blocks nested three deep -- a line reached through three references), `g5` (a drawing that is mostly dimensions), `g6` (two coincident lines), `g7` (a title block of loose texts), `g8` (a title block in Korean, CP949 in the file), `g9` (the same title block twice, two drawing numbers), `g10` (a reference to a block that does not exist), `g21` (the ways a drawing states a tolerance) and `g22` (a sheet of three views inside a frame: a closed rectangle the size of an A3 drawing space, a plate with two holes seen from the front, the top and the side, and a line past the frame), `g23` (a title block of loose texts laid out as ISO 7200 draws its fields: each name a small text at the top left of its field, its value a larger text beneath it -- one value over two lines, two lines of one height, and a centred value among them), `g24` (one view whose outline is separate lines and an arc meeting at their ends -- a plate with one rounded corner, a hole and a line that touches nothing). The tests read these models directly -- no parser is involved -- and check what this crate says about them against the spec.

The files are generated, not hand-written. To regenerate after a change to the writer or the spec, from a checkout of `uncad-model`:

```
cargo run -p uncad-model-golden --example write_case -- <case> <case>.dxf <case>.expected.json
```

and copy the JSON here. A tree that carries both repositories side by side checks that the copies have not drifted from the writer.
