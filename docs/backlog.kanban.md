# backlog

id: zrx9backlog0board000001

> Pending work and ideas for Zerx. The `Someday` column parks speculative items
> that probably won't be implemented, so the committed columns stay honest.

## Backlog
id: zrx9backlog0col0backlog1

- [ ] Built-in OpenAPI import policy
  > Deferred from CONCEPT_zerx_foundation (scope boundary). The policy pipeline and
  > `register_policy` already support out-of-tree policies; only the `sql` policy
  > ships built-in for v1. An OpenAPI policy would join `sql` as a second built-in.

## Next
id: zrx9backlog0col0next0001

## In Progress
id: zrx9backlog0col0inprog01

## Done
id: zrx9backlog0col0done0001

## Someday
id: zrx9backlog0col0someday1

- [ ] Zero-copy Lua input via a borrowing `LuaSource` trait
  > v1 ships `validate_lua` over an owned `zerx::lua::LuaValue` (D-lua-schema-directed,
  > KISS). A borrowing `LuaSource` trait would let a consumer validate its native Lua
  > value in place without first building an owned `lua::LuaValue`. Only worth it if a
  > consumer shows the conversion copy is a measured bottleneck.

- [ ] Async / streaming validation
  > Explicitly out of scope for the foundation (CONCEPT scope boundary). Parked here
  > as a speculative direction; no consumer demand yet.
