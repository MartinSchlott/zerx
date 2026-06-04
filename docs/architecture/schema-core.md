# Concern: Schema Core

> Status: skeleton. Constraints and contracts are filled by `PLAN_F2_schema_core`.

## Purpose

- This concern owns the `Schema` representation: the enum-kind core, the shared modifier/validator carrier, and the typed builders that front it.
- This concern owns the parse flow — depth limit, circular-reference check, default/optional/nullable ordering — and lazy/cyclic schemas with their reentrance guard.
- This concern owns immutable chaining: every modifier returns a new `Schema` value.

## Non-Goals

- This concern does NOT define the individual types or validators — see `types`.
- This concern does NOT own JSON Schema export/import — see `json-schema`.
- This concern does NOT define the dynamic value it produces — see `value-model`.

## Related Decisions

- Pending migration. This concern is governed by candidate decisions C1, C7, C4
  in `docs/CONCEPT_zerx_foundation.md`; their `D-` slug IDs are added here at
  Concept Closeout, once promoted to `docs/decisions.md`.
