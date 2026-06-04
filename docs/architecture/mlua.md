# Concern: mlua (host-opaque)

> Status: skeleton. Constraints and contracts are filled by `PLAN_M1_host_opaque`.
> This concern is entirely behind the optional `mlua` feature and absent from the
> default build.

## Purpose

- This concern owns the host-opaque layer: validating `mlua` values (functions, coroutines, userdata) via a dedicated `validate_lua` path, since these are not `Serialize`.
- This concern owns the `function` and `tvalue` types and their JSON Schema format markers.
- This concern owns the realisation of the host-opaque `ZerxValue` variant: a handle, not an owned value, treated as a leaf by the cycle guard and excluded from the serde roundtrip.

## Non-Goals

- This concern does NOT participate in the serde roundtrip — host-opaque values never serialise.
- This concern does NOT exist in the default build — it is feature-gated.

## Consumes from

- `value-model`: `ZerxValue::HostOpaque` variant (the gated slot `mlua` realises with a concrete handle type)

## Related Decisions

- Pending migration. This concern is governed by candidate decision C3
  in `docs/CONCEPT_zerx_foundation.md`; its `D-` slug ID is added here at
  Concept Closeout, once promoted to `docs/decisions.md`.
