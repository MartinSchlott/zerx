# Concern: Policy

> Status: skeleton. Constraints and contracts are filled by `PLAN_P1_policy_pipeline`.

## Purpose

- This concern owns the composable import pipeline: schema transforms (pre-parse, on the input JSON Schema) and type transforms (post-parse, on the produced `Schema` values).
- This concern owns the built-in `sql` policy (PostgreSQL-focused) and policy registration (`register_policy`).
- This concern owns the deref hook for external `$ref`.

## Non-Goals

- This concern does NOT own the base JSON Schema walk it wraps — see `json-schema`.
- This concern does NOT define the types the transforms produce — see `types`.

## Related Decisions

- (none yet — policy behaviour is settled within the policy plan)
