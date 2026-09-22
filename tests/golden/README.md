# Golden cases

Copies of what the `uncad-model` repository's golden writer produces, one expected model per case:

| File | What it is |
|---|---|
| `<case>.expected.json` | The model a reader must produce from that case's synthetic drawing; the spec is the oracle |

Cases here: `g1` (a general part with a title block), `g2` (blocks nested three deep -- a line reached through three references), `g6` (two coincident lines), `g7` (a title block of loose texts), `g9` (the same title block twice, two drawing numbers), `g10` (a reference to a block that does not exist). The tests read these models directly -- no parser is involved -- and check what this crate says about them against the spec.

The files are generated, not hand-written. To regenerate after a change to the writer or the spec, from a checkout of `uncad-model`:

```
cargo run -p uncad-model-golden --example write_case -- <case> <case>.dxf <case>.expected.json
```

and copy the JSON here. A tree that carries both repositories side by side checks that the copies have not drifted from the writer.
