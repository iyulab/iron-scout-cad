# Principles

> This document describes the rules currently in force. A sentence here that is wrong is a bug.

## 1. Determinism is non-negotiable

The same input produces the same output. The library never calls an inference endpoint, never embeds a model, and never guesses.

When a value cannot be established, the library returns **"unknown"** as a first-class value. It does not fill the gap with a default or an estimate. "Unknown" is a normal result, not an error.

*What this costs:* coverage. A caller will see "unknown" in places where a heuristic would have produced something plausible. That is intended — a plausible wrong number is worse than no number.

## 2. Provenance and confidence travel with every entity

The entity model this library consumes carries, for every entity: a **reference ID**, a **provenance** (where the entity came from), and a **confidence** (how far its values can be trusted, including "unknown").

- Confidence never rises on its own. A value that arrived with low confidence is never reported with a higher one, whatever this library does to it.
- The library does not need to know *why* an entity has low confidence. It acts on the marker alone.
- Entities the upstream parser could not interpret arrive as "unrecognized" and are preserved as such. Nothing is silently dropped.

*What this costs:* every summary, reference and diff has to carry these markers through. There is no shortcut path that strips them.

## 3. One reference scheme

An entity reference issued here is accepted as-is by [iron-hand-cad](https://github.com/iyulab/iron-hand-cad). The two libraries do not maintain separate coordinate or reference systems. If "here" can mean two different things on the two sides, the design is wrong.

## 4. Verification is a numeric diff

The only accepted evidence that a change did what was intended is the numeric difference between two model states. Comparing rendered images is not verification: tolerances are invisible in a render, and "looks right" is not "is right".

Tests follow the same rule — no test decides pass/fail by comparing renders.

## 5. A small, fixed set of verbs

The public surface is a small fixed set of verbs extended through parameters, with schemas loadable on demand. A new capability is expressed with existing verbs and new parameters first. A new verb is proposed only with evidence that this is not possible.

*What this costs:* some capabilities will be more awkward to express than a dedicated verb would make them.

## 6. Domain neutrality

The library knows CAD. It does not know what the drawing is *for*. Concepts that only one consumer needs — business terms, workflow steps, labels — belong in that consumer's adapter, including when they are dressed in generic-sounding names.

The test: *would a third party using this library for the first time need the same thing in the same place?* If not, it does not belong here.

## 7. Trade-off order

When goals collide, the earlier one wins:

> API simplicity › coverage › development speed › backward compatibility

Determinism is not on this list because it is never traded. Licensing is not on this list because it is a constraint: the dependency tree of this crate is permissive-only (MIT / Apache-2.0 / BSD).

## 8. Compatibility

The crate is in 0.x. When a more correct design is found, a breaking change is the normal way to adopt it; it is not deferred for migration cost. Breaking changes bump the minor version. The major version is not bumped without an explicit maintainer decision.

## 9. Contributing

| Just do it | Propose first | Discuss before any work |
|---|---|---|
| Tests · bug fixes and refactors that leave the public API unchanged · docs | Public API changes · new dependencies · **new verbs** · changing the meaning of, or removing, an entity-model field | Anything that adds inference · anything in "What it is not" · copyleft dependencies · changes to the provenance/confidence contract, including new confidence values |

Adding an entity type or field is fine without prior discussion; mention it in the change description. If it is unclear which column a change falls in, treat it as the stricter one.

Tests for things the library **must not do** — dropping an entity silently, raising a confidence, modifying its input — are required, and their failure count is always zero.
