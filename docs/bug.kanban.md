# bug

id: zrx9bug0board0tracking0001

> Issue and bug tracking for Zerx. Cards with `severity: accepted` document
> conscious deviations from the target vision (the "won't fix" bucket).

## Open
id: zrx9bug0col0open00000001

### Consumes / Provides duals differ in wording

id: {new}
severity: minor

The permanent-doc consistency check (M2) fails on existing text: the declarations for `types` ↔ `schema-core` (`Validator`), `types` ↔ `json-schema` (validator fragments) and `lua` ↔ `value-model` (`ZerxValue`) name equivalent contracts but do not match exactly. Found during plan review of PLAN_T5_literal_value_equality; pre-existing.

### Plan references in docs/architecture/types.md

id: {new}
severity: minor

The permanent-doc consistency check (M14) fails on existing text: `docs/architecture/types.md` keeps `PLAN_*` references in headings, error-code bullets, its Provides entry and the `all_optional` export rule. The fix drops each reference and keeps the rule. Found during plan review of PLAN_T5_literal_value_equality; pre-existing.

## In Progress
id: zrx9bug0col0inprogress01

## Done
id: zrx9bug0col0done00000001
