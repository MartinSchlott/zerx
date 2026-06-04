# Concern: Value Model

> Status: skeleton. Constraints and contracts are filled by `PLAN_F1_value_model`
> (host-opaque variant realised by `PLAN_M1_host_opaque`).

## Purpose

- This concern owns `ZerxValue`: the validated dynamic value spanning serde's full data model (including bytes) plus the host-opaque layer.
- This concern owns the serde bridge — accepting any `T: Serialize` and serialising any `ZerxValue` to any serde format.

## Non-Goals

- This concern does NOT own validation rules or schema shape — see `schema-core`.
- This concern does NOT own JSON Schema representation — see `json-schema`.

## Related Decisions

- Pending migration. This concern is governed by candidate decisions C5, C2, C3
  in `docs/CONCEPT_zerx_foundation.md`; their `D-` slug IDs are added here at
  Concept Closeout, once promoted to `docs/decisions.md`.
