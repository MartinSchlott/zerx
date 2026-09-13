# Verification

Local Cargo artifacts and runner output on this machine live under
`/Users/martinschlott/Documents/MyProjects/RustBuildTargets/zerx`. Resolve output
through Cargo metadata; older `target/` examples below refer to that output root.
See [the machine build-storage guide](../../RUST_BUILD_STORAGE.md).


`scripts/verify` is zerx's canonical verification entrypoint. `scripts/verify all` is the
pre-commit / final-validation gate — run it before considering any change done.

## Usage

```
scripts/verify all                  # lint, test (both variants), api
scripts/verify lint
scripts/verify test [FILTER]        # all-features variant
scripts/verify test core [FILTER]   # feature-off variant
scripts/verify api [FILTER]
scripts/verify --help
```

`FILTER` is passed through as the libtest name filter, e.g. `scripts/verify test strip_unknown`.

## Exit statuses

| Status | Meaning |
|---|---|
| `0` | every requested gate passed |
| `2` | `BUSY` — another verification run holds the lock; nothing was executed |
| `64` | usage error; nothing was executed |
| `70` | the runner could not establish process-group ownership of one of its owned subprocess groups; the lock is retained |
| `71` | the owned process group survived `TERM` **and** `KILL`; the lock is deliberately retained |
| `124` | a gate exceeded its timeout (300s) |
| other | the failing gate's child exit status, passed through verbatim (cargo typically `101`) |

## If a lock was left behind

`scripts/verify` serialises runs through `target/verify/.lock`. Four distinct situations can
leave that lock in place, and they are **not** interchangeable — each needs a different check
before removal.

- **`BUSY — stale lock from dead pid <n>`** — the recorded owner is provably dead, so nothing
  is holding the build directory. Safe to clear directly: `rm -rf target/verify/.lock`.
- **`BUSY — verification lock present with incomplete metadata`** — the runner cannot tell an
  abandoned lock from a healthy runner two instructions into its own startup. Check first
  whether a `scripts/verify` is actually running; clear the lock only if none is.
- **exit `71`** — retained deliberately, because something this run started survived `TERM`
  and `KILL`. **Not** a bare `rm -rf` case. The `FATAL` line names which target survived, and
  they carry different urgency:
  - *gate process group* — identifier also in `target/verify/logs/<gate>.pgid`. These are
    cargo/rustc processes that **do** hold this run's `CARGO_TARGET_DIR`. Clearing the lock
    while they live lets the next run share the build directory with them.
  - *watchdog process group* — identifier in `target/verify/logs/<gate>.wpgid`, a different
    file. Its members are a shell and a `sleep`; they do **not** hold `CARGO_TARGET_DIR`. The
    lock is still retained because the runner cannot prove what it cannot kill, but the
    remediation is a stuck `sleep`, not a build hazard.
  - *bare child* — a PID rather than a PGID, on the path where the group was never read.

  Recovery in all three cases: inspect the survivors with `pgrep -g <pgid> .` (the trailing
  pattern is required on macOS) or `ps -p <pid>`, remediate by hand, confirm independently
  that `kill -0 -<pgid>` — or `kill -0 <pid>` — now fails, and only then remove the lock.
- **exit `70`** — the runner could not establish process-group ownership of one of its two
  owned subprocess groups and aborted. Which one is named in the `FATAL` line:
  - *gate child* — the direct cargo child was reaped, but any rustc descendants it had already
    forked could not be enumerated or killed; they are re-parented and no longer reachable
    from the runner.
  - *watchdog* — the gate group was swept normally, but the watchdog's `sleep` child could not
    be reached. Harmless to the build directory, but still unproven.

  This is the least safe case to clear casually: unlike exit `71` there is no group identifier
  to re-check. Recovery: verify nothing is still writing under `target/verify/` — inspect
  candidates by hand (`ps`/`lsof +D target/verify`) rather than trusting a bare
  `pgrep -f 'rustc|cargo'`, which also matches unrelated developer and agent processes —
  terminate what belongs to the aborted run, and only then remove the lock. Exit `70` also
  signals a host-environment problem worth reporting, not just a lock to clear.

## Gates

| Gate | Command | Covers | Excludes | Cold / warm | When |
|---|---|---|---|---|---|
| `lint` | `scripts/verify lint` | clippy over lib + tests, all features, warnings are errors | run-time behaviour | ~3 s / <1 s | inner loop, `all` |
| `test` | `scripts/verify test` | all in-module tests with `lua` enabled | doctests, feature-off compile | ~7 s / <1 s | inner loop, `all` |
| `test core` | `scripts/verify test core` | all in-module tests with `lua` disabled — proves the feature-off configuration compiles and passes | the `lua`-gated tests | cold cost close to `test`'s / <1 s warm | `all`, and any plan touching `#[cfg(feature = "lua")]` |
| `api` | `scripts/verify api` | the `compile_fail` doctests — public builder types reject foreign methods | run-time behaviour | ~1 s / <1 s | `all`, and any plan changing the public API surface |
| *(all)* | `scripts/verify all` | the four rows above, once each | — | ~15 s / ~2 s | pre-commit, final validation |

`lint` runs in the all-features configuration only; the feature-off configuration is covered
structurally by `test core` (it proves feature-off compiles and passes), not by a second lint
pass.

## Test inventory by area

zerx keeps tests in-module (`#[cfg(test)] mod tests`) inside the file they test; there is no
`tests/` integration directory.

| Area | Boundary | Concern | Perspectives |
|---|---|---|---|
| `value` | unit | `value-model` | serde bridge round-trip, type mapping, serialization failure modes |
| `schema` | unit | `schema-core` | modifier composition, parse flow, lazy & cyclic schemas, `MAX_PARSE_DEPTH` boundary |
| `delta` | unit | `schema-core` | `parse_delta` and `replace` happy path, partial-update semantics, failure modes |
| `types` | unit | `types` | per-type happy & sad paths, validator boundaries, special-type composition |
| `json_schema` | unit | `json-schema` | export/import round-trip, draft 2020-12 conformance, `$defs`/`$ref`, `strip_unknown`, error paths |
| `policy` | unit | `policy` | pipeline composition, transform ordering, error propagation |
| `error` | unit | `errors` | code taxonomy, path aggregation, JSON shape, `std::error::Error` propagation |
| `lua` (feature-gated) | unit | `lua` | schema-directed validation, error precedence, feature contract |
| doctests | compile-time | public API surface | negative type-safety of the builder types |

## Rule for new tests

Name the boundary (unit / compile-time) and the perspective (happy path, failure mode,
boundary, round-trip, conformance, feature contract) the new test covers. If both are already
covered for that area, extend the existing `mod tests` block in the owning module; otherwise
state in the plan why a new suite is needed. Duplicating an already-covered boundary+perspective
combination is a Reviewer quality finding, not rigor. zerx has no `tests/` directory — a plan
that wants one justifies it.

## Targeted runs and logs

`scripts/verify test <filter>` (and the `test core`/`api` equivalents) runs a libtest name
filter for diagnosis. Each gate's full output lands in `target/verify/logs/<gate>.log`,
truncated and overwritten on the next run of that gate. A filtered run, or a raw `cargo`
invocation run by hand, is a diagnostic aid — it does not satisfy a required gate; the
unfiltered gate still has to run.
