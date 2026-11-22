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

## Consumes from

- `errors`: `ZerxError` (structured validation failure returned by the parse flow)
- `value-model`: `ZerxValue` (the validated dynamic value produced by the parse flow)
- `value-model`: `Map` (the ordered object map carried inside `ZerxValue::Object`)
- `types`: `per-kind check and parse delegates` (dispatch targets of the kind arms)
- `errors`: `ErrorCode` (open catalogue extended by this concern)

## Provides to

- `types`: `Schema` (core representation that type variants extend by adding `SchemaKind` variants)
- `types`: `Validator` (trait contract that concrete type validators implement)
- `json-schema`: `Schema` (schema value that export reads and import reconstructs)
- `policy`: `Schema` (the value the pipeline produces and type transforms receive)
- `lua`: `Schema` + parse flow (`validate_lua` feeds `parse_present` after the transform pass)
- `value-model`: `MAX_PARSE_DEPTH` (nesting bound enforced by the serde-in bridge)
- `policy`: `MAX_PARSE_DEPTH` (bound on external ref nesting)

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
- `Schema` MUST implement `Debug` via a hand-written implementation rendering the kind name without payload, the modifier carrier with refinements as a count, and the validator count; closures and thunk contents MUST NOT be rendered.
- `Schema` MUST NOT implement `PartialEq` or `Eq`.
- `SchemaKind` is owned by this concern as a closed container; each variant's payload and semantics are owned by `types`, except `Any` and `Lazy`.
- Adding a type MUST require exactly one new `SchemaKind` variant plus one arm in every exhaustive per-kind dispatch (type check, type-specific parse, debug rendering, export, delta navigation, Lua transform); it MUST NOT add parse-flow control logic.
- The universal modifier set (`optional`, `nullable`, `default`, `describe`, `refine`, `format`, `mime_format`, `deprecated`, `read_only`, `write_only`, `meta`, `example`, `title`) MUST be exposed on every schema builder via a blanket `Modify` trait; each method MUST return the builder type to preserve chain continuity.
- `Schema` itself MUST also implement `Modify`.
- The `Validator` trait and the validator storage field on `Schema` are owned by this concern; this concern provides the storage contract only. All concrete `Validator` implementations are owned by `types`.
- `Schema::validate` MUST accept any serialisable value, convert it via `ZerxValue::from_serialize`, and then run the parse flow on the resulting `ZerxValue`.
- The parse flow for a **present** value MUST proceed in this fixed order: depth guard → `nullable` null short-circuit → builder-value defect gate → type check (`check_type`) → validators in storage order → type-specific logic (`parse_inner`) → refinement predicates.
- For a `lazy` schema the outer lazy's own `nullable` null short-circuit and builder-value defect gate MUST run before resolution; the resolved inner schema then MUST run its own full present-value parse flow, followed by the outer lazy's refinement predicates. A `lazy` carries its own modifiers like every other schema; delegating before the short-circuit would discard the outer `nullable`.
- The parse flow for a **missing** value MUST proceed in this fixed order: if the node carries a builder-value defect, return `Err(PARSE_DEPTH_EXCEEDED)`; else if `default` is set, parse the default and return it; else if `optional` is set, omit the field from output; else return `Err(REQUIRED)`.
- A missing optional field MUST be omitted from output; it MUST NOT be emitted as `Null`.
- A value passed to `default`, `meta`, `example` or `literal` that is nested deeper than `MAX_PARSE_DEPTH` containers MUST NOT be kept; it MUST be torn down without recursion, and the node MUST carry a builder-value defect, because builders are infallible (`D-result-only-api`) and a kept deep value would overflow the stack on clone, export or drop.
- A node carrying a builder-value defect MUST reject every non-null value with `PARSE_DEPTH_EXCEEDED` and MUST take the fail-closed export of `json-schema`; `null` under `nullable` stays accepted.
- An explicit `Null` value MUST NOT trigger `default`; `default` applies only when the value is absent.
- For a missing field whose schema is a `lazy`, only the outer lazy's `default` and `optional` apply; the resolved inner schema's are not consulted, so such a field is required. This is an accepted deviation (`docs/bug.kanban.md`), not a binding rule.
- The parse flow MUST track a depth counter incremented by every present-value parse, including `lazy` wrapper frames; `MAX_PARSE_DEPTH = 100`, and a parse that would exceed it MUST return `Err(PARSE_DEPTH_EXCEEDED)`.
- A parse refused by the depth guard MUST leave the depth counter unchanged, so the context remains reusable across a failed call.
- The depth guard counts recursion frames, not concrete-node levels; a `lazy` frame MUST consume one depth level in the same way as any other kind frame.
- A `lazy` schema MUST memoise its resolved inner `Schema`; the thunk MUST be called at most once, and the cached result MUST be shared across all clones of the same `lazy` schema instance.
- A `lazy` schema MUST detect synchronous self-resolution reentrance and MUST return `Err(LAZY_REENTRANCE)` instead of recursing.
- `Schema` MUST NOT be `Send` or `Sync`; shared pieces (validators, refinement predicates, `lazy` thunk and resolution cache) MUST be held via single-threaded reference-counted smart pointers.
- Error codes introduced by this concern (`PARSE_DEPTH_EXCEEDED`, `REQUIRED`, `REFINEMENT_FAILED`, `LAZY_REENTRANCE`) MUST be declared by this concern, not in the `errors` catalogue.

**Delta / Replace** — constraints for the schema-only delta validation and the immutable-replace-and-revalidate operations:

- Both operations MUST return `Result<ZerxValue, ZerxError>`; no throwing variant, no `safe*` or `try*` counterpart MUST exist (`D-result-only-api`).
- The addressing scheme MUST be JSON Pointer (RFC 6901): an empty string addresses the root; any non-empty pointer MUST begin with `'/'`; segments MUST be split on `'/'`; un-escaping MUST proceed `~1`→`'/'` then `~0`→`'~'` in this order. A non-empty pointer not starting with `'/'` MUST be rejected with `INVALID_POINTER`.
- A `~` not followed by `0` or `1`, and an array or tuple index with a leading zero, MUST be rejected with `INVALID_POINTER`, as RFC 6901 does not admit them.
- `parse_delta` MUST navigate the schema by kind only (no instance), then run `parse_present` against the located sub-schema. Navigation MUST be a closed per-kind dispatch over `SchemaKind` with no catch-all arm, so a future variant is a compile error until handled.
- `parse_delta` MUST reject a segment naming a prestripped object key with `INVALID_PATH`; validation treats that key as absent, so no value supplied there survives.
- `parse_delta` MUST navigate a key that a `passthrough` object does not declare to `any()`, as validation keeps any value there; a `strict` or `strip` object MUST reject it with `UNKNOWN_PROPERTY`.
- Unwrapping `lazy` layers during navigation MUST be bounded by the parse-depth limit and MUST return `PARSE_DEPTH_EXCEEDED` beyond it, because a lazy may resolve only to further lazies.
- `replace` MUST navigate the instance by value kind only (schema-independent), rebuild the tree without mutating the supplied instance, then run full root revalidation via `parse_present`; revalidation MUST include refinement predicates and `default` application.
- Descending into a `union`, `discriminated_union` or the crate-internal `OneOf` kind during `parse_delta` navigation MUST be rejected with `UNION_PATH_REQUIRES_INSTANCE`; variant selection requires an instance and is not schema-navigable alone.
- `replace` MUST NOT create absent positions: an object-key segment absent from the instance MUST be rejected with `MISSING_PARENT` at every traversal depth, including the final segment, regardless of whether the schema declares the key optional.
- `replace` MUST refuse a rebuilt tree nested deeper than `MAX_PARSE_DEPTH` containers with `PARSE_DEPTH_EXCEEDED`, because instance and new value are each bounded but their combination is not.
- `replace` does not support deletion; it always sets an existing position.
- `replace` operates only on serde-bridgeable trees; Lua values (`LuaValue`) are not `Serialize` and cannot be passed as the `new` argument.
- Error codes introduced by this sub-concern (`INVALID_POINTER`, `INVALID_PATH`, `INDEX_OUT_OF_RANGE`, `UNION_PATH_REQUIRES_INSTANCE`, `MISSING_PARENT`) MUST be declared by this sub-concern, not in the `errors` catalogue.

## Related Decisions

- `D-modifier-composition`
- `D-lua-schema-directed`
- `D-result-only-api`
- `D-strict-by-default`
