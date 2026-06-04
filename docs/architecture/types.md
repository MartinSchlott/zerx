# Concern: Types

> Status: skeleton. Constraints and contracts are filled by `PLAN_T1_basic_types`,
> `PLAN_T2_complex_types`, `PLAN_T3_object_utilities`, and `PLAN_T4_special_types`.

## Purpose

- This concern owns the type catalogue: basic (`string`, `number`, `boolean`, `enumerate`, `null`, `any`), complex (`object`, `array`, `record`, `tuple`, `union`, `discriminated_union`, `literal`, `lazy`), and special (`buffer`, `uri`, `url`, `json`, `jsonschema`).
- This concern owns the pluggable validators (`min`, `max`, `regex`, `int`, `multiline`, `email`, `uuid`, …) as a trait exposing `validate` and a JSON Schema contribution.
- This concern owns object utilities: the three modes (`strict`/`passthrough`/`strip`) and the shape/runtime helpers (`partial`, `extend`, `omit*`, `strip*`).

## Non-Goals

- This concern does NOT define the core `Schema` representation or parse flow — see `schema-core`.
- This concern does NOT own the host-opaque `function`/`tvalue` types — see `mlua`.

## Related Decisions

- Pending migration. This concern is governed by candidate decisions C4, C2
  in `docs/CONCEPT_zerx_foundation.md`; their `D-` slug IDs are added here at
  Concept Closeout, once promoted to `docs/decisions.md`.
