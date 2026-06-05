# Concern: Schema Core

## Purpose

- This concern owns the `Schema` representation: the enum-kind core, the shared modifier/validator carrier, and the typed builders that front it.
- This concern owns the parse flow: depth limit, default/optional/nullable ordering, validators, and refinement predicates.
- This concern owns immutable chaining: deriving a modified schema is clone-and-return; the original is never mutated.
- This concern owns `lazy` schemas: memoised thunk resolution with a synchronous reentrance guard.

## Non-Goals

- This concern does NOT define the individual types or concrete validators — see `types`.
- This concern does NOT own JSON Schema export/import — see `json-schema`.
- This concern does NOT define the dynamic value it produces — see `value-model`.
- This concern does NOT implement data-value cycle detection — see `mlua` (`PLAN_M1`).

## Consumes from

- `errors`: `ZerxError` (structured validation failure returned by the parse flow)
- `value-model`: `ZerxValue` (the validated dynamic value produced by the parse flow)
- `value-model`: `Map` (the ordered object map carried inside `ZerxValue::Object`)

## Provides to

- `types`: `Schema` (core representation that type variants extend by adding `SchemaKind` variants)
- `types`: `Validator` (trait contract that concrete type validators implement; storage field owned here, implementations in `types`)
- `json-schema`: `Schema` (schema value that export reads and import reconstructs)

## External Contracts

None.

## External Services

None.

## Architecturally Significant Dependencies

- `serde` — `Schema::validate` accepts any serialisable value; removing `serde` would require redesigning the public validation entry point.
- `serde_json` — the `Validator` trait's JSON Schema contribution method returns a `serde_json::Map`; this return type is part of the public trait signature and cannot change without redesigning the validator contract.

## Constraints

- `Schema` MUST be a single concrete struct carrying a kind discriminant, a shared modifier carrier, and a validator storage field; it MUST NOT be a bare trait object.
- `Schema` MUST implement `Clone`; every modifier method MUST return a new schema-bearing value without mutating the receiver (clone-and-return).
- `Schema` MUST implement `Debug` via a hand-written implementation rendering kind name, modifier flags, and validator count; closures and thunk contents MUST be rendered as placeholders. `Schema` MUST NOT implement `PartialEq` or `Eq`.
- `SchemaKind` is owned by this concern as a container; each variant and its payload are owned by the plan that introduces the corresponding type. This concern seeds `Any` and `Lazy`; type plans extend the enum.
- Adding a new type MUST require exactly one new `SchemaKind` variant plus one delegating arm in each dispatch method (`check_type`, `parse_inner`); no new parse-flow control logic MAY be introduced per type.
- The universal modifier set (`optional`, `nullable`, `default`, `describe`, `refine`, `format`, `mime_format`, `deprecated`, `read_only`, `write_only`, `meta`, `example`, `title`) MUST be exposed on every schema builder via a blanket `Modify` trait; each method MUST return the builder type to preserve chain continuity. `Schema` itself MUST also implement `Modify`.
- The `Validator` trait and the validator storage field on `Schema` are owned by this concern; this concern provides the storage contract only. All concrete `Validator` implementations are owned by `types`.
- `Schema::validate` MUST accept any serialisable value, convert it via `ZerxValue::from_serialize`, and then run the parse flow on the resulting `ZerxValue`.
- The parse flow for a **present** value MUST proceed in this fixed order: depth guard → `nullable` null short-circuit → type check (`check_type`) → validators in storage order → type-specific logic (`parse_inner`) → refinement predicates.
- The parse flow for a **missing** value MUST proceed in this fixed order: if `default` is set, parse the default and return it; else if `optional` is set, omit the field from output; else return `Err(REQUIRED)`.
- A missing optional field MUST be omitted from output; it MUST NOT be emitted as `Null`.
- An explicit `Null` value MUST NOT trigger `default`; `default` applies only when the value is absent.
- `ParseContext` MUST track a depth counter incremented by every `parse_present` invocation, including `lazy` wrapper frames; `MAX_PARSE_DEPTH = 100`. A `parse_present` invocation that would exceed this limit MUST return `Err(PARSE_DEPTH_EXCEEDED)` and MUST leave the depth counter unchanged (decrement before returning the error) so the context remains reusable across a failed call.
- The depth guard counts recursion frames, not concrete-node levels; a `lazy` frame MUST consume one depth level in the same way as any other kind frame.
- A `lazy` schema MUST memoise its resolved inner `Schema`; the thunk MUST be called at most once, and the cached result MUST be shared across all clones of the same `lazy` schema instance.
- A `lazy` schema MUST detect synchronous self-resolution reentrance and MUST return `Err(LAZY_REENTRANCE)` instead of recursing.
- Data-value cycle detection is deferred to `PLAN_M1`; `ParseContext` is the designated seam for attaching a visited-set without signature changes.
- `Schema` MUST NOT be `Send` or `Sync`; shared pieces (validators, refinement predicates, `lazy` thunk and resolution cache) MUST be held via single-threaded reference-counted smart pointers.
- Error codes introduced by this concern (`PARSE_DEPTH_EXCEEDED`, `REQUIRED`, `REFINEMENT_FAILED`, `LAZY_REENTRANCE`) MUST be declared as associated constants in a separate `impl ErrorCode` block in `src/schema.rs`; no edit to `src/error.rs` is permitted.

**Delta / Replace** (`src/delta.rs`) — constraints for the schema-only delta validation and the immutable-replace-and-revalidate operations:

- Both operations MUST return `Result<ZerxValue, ZerxError>`; no throwing variant, no `safe*` or `try*` counterpart MUST exist (candidate C7).
- The addressing scheme MUST be JSON Pointer (RFC 6901): an empty string addresses the root; any non-empty pointer MUST begin with `'/'`; segments MUST be split on `'/'`; un-escaping MUST proceed `~1`→`'/'` then `~0`→`'~'` in this order. A non-empty pointer not starting with `'/'` MUST be rejected with `INVALID_POINTER`.
- `parse_delta` MUST navigate the schema by kind only (no instance), then run `parse_present` against the located sub-schema. Navigation MUST be a closed per-kind dispatch over `SchemaKind` with no catch-all arm, so a future variant is a compile error until handled.
- `replace` MUST navigate the instance by value kind only (schema-independent), rebuild the tree immutably via clone-and-return (candidate C1), then run full root revalidation via `parse_present`; revalidation MUST include refinement predicates and `default` application.
- Descending into a `union` or `discriminated_union` kind during `parse_delta` navigation MUST be rejected with `UNION_PATH_REQUIRES_INSTANCE`; variant selection requires an instance and is not schema-navigable alone.
- `replace` MUST NOT create absent positions: an object-key segment absent from the instance MUST be rejected with `MISSING_PARENT` at every traversal depth, including the final segment, regardless of whether the schema declares the key optional.
- `replace` does not support deletion; it always sets a position. Deletion is out of scope (candidate C7 forbids a second API shape for the same operation family).
- Host-opaque descent errors for trees containing host-opaque leaves are deferred to `PLAN_M1_host_opaque` (candidate C3); the public `replace` signature's `Serialize` bound permanently excludes host-opaque instances from its argument domain.
- Error codes introduced by this sub-concern (`INVALID_POINTER`, `INVALID_PATH`, `INDEX_OUT_OF_RANGE`, `UNION_PATH_REQUIRES_INSTANCE`, `MISSING_PARENT`) MUST be declared in an `impl ErrorCode` block in `src/delta.rs`; no edit to `src/error.rs` is permitted.

## Related Decisions

- Pending migration. This concern is governed by candidate decisions C1, C3, C7, C4 in `docs/CONCEPT_zerx_foundation.md`; their `D-` slug IDs are added here at Concept Closeout, once promoted to `docs/decisions.md`.
