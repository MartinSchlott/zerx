# PLAN_V1 — Canonical verification entrypoint and `docs/tests.md`

## Context & Goal

zerx has no canonical verification entrypoint. Every archived plan improvised its own
command list, and those lists have drifted apart:

- `PLAN_J1_export.md`, `PLAN_J2_import.md`, `PLAN_P1_policy_pipeline.md` verify with
  `cargo build` + `cargo test` + `cargo clippy --all-targets -- -D warnings` +
  `cargo test --features mlua`. The `mlua` feature **no longer exists** — `Cargo.toml`
  declares exactly one feature, `lua`. Those command lists are unrunnable as written.
- `PLAN_J3_import_strip_unknown.md` verifies with `cargo build --all-features` +
  `cargo clippy --all-features -- -D warnings` + `cargo test --all-features` + four
  targeted `cargo test` filters.
- Flag drift between the two families: `--all-targets` vs. `--all-features`, and clippy
  is run *with* `--all-targets` in one family and *without* it in the other, so test code
  is linted in some plans and not in others.
- Redundant build step: `cargo build --all-features` immediately followed by
  `cargo test --all-features` builds the same artifacts twice.
- Configuration thrashing: alternating between the default (feature-off) and
  `--all-features` configurations in the shared `target/` directory invalidates the build
  cache on every switch. Measured on this machine: a feature-set switch costs ~6.1 s for
  `cargo test` and ~2.6 s for `cargo clippy`, against ~0.3 s and ~0.2 s warm.
- Success output is noise: `cargo test --all-features` prints **280 lines** of passing-test
  names for a green run.

The goal is the shape prescribed by the `standardize-verification` skill: **one canonical
runner, three stable gates, quiet on success, focused on failure**, plus a one-page
`docs/tests.md` that becomes the verification contract every future plan references instead
of listing raw commands.

This plan changes **no library code and no test code**.

## Breaking Changes

**No** — for the library. `src/` is untouched, the public API is untouched, no dependency
changes, `Cargo.toml` and `Cargo.lock` are untouched.

**Yes** — for the workflow. From this plan forward, the Verification section of every plan
references gates from `docs/tests.md` plus plan-specific checks only; raw `cargo`
sequences in plan Verification sections become a Reviewer finding. Archived plans in
`docs/archive/` are historical records and are **not** retrofitted.

Nothing for the Product Owner to reset or migrate.

## Dependencies

**None.** The runner is `bash` (macOS system bash 3.2 compatible) driving the `cargo`
toolchain the project already uses. No new crates, no `coreutils`, no `cargo-nextest`, no
CI service.

## Reference Patterns

There is no existing script in this repository to imitate. For the documentation voice,
`docs/tests.md` is **contributor guidance, not a permanent normative doc** (CLAUDE.md,
"Documentation Style"): plain descriptive prose and tables, **no RFC 2119 keywords**, not
subject to `permanent-doc-consistency-check`.

For `docs/backlog.kanban.md`, the reference pattern is **the existing file itself** — see
Step 7.

## Discovered state (measured, not assumed)

A fresh instance can rely on these facts; re-measure only if the numbers look wrong.

Test population, all in-module `#[cfg(test)] mod tests` blocks — there is no `tests/`
integration directory:

| File | Tests | Concern (`docs/architecture/`) |
|---|---|---|
| `src/types.rs` | 60 | `types.md` |
| `src/json_schema.rs` | 47 | `json-schema.md` |
| `src/lua.rs` | 47 (feature-gated) | `lua.md` |
| `src/delta.rs` | 42 | `schema-core.md` (delta/replace) |
| `src/policy.rs` | 33 | `policy.md` |
| `src/schema.rs` | 11 | `schema-core.md` |
| `src/value.rs` | 11 | `value-model.md` |
| `src/error.rs` | 8 | `errors.md` |

- `cargo test --all-features` → **259** lib tests + **6** doctests (1 ignored).
- `cargo test` (default, `lua` off) → **212** lib tests + **6** doctests (1 ignored).
- `src/lua.rs` is `#[cfg(feature = "lua")] pub mod lua;` in `src/lib.rs`, so its 47 tests
  exist only in the `--all-features` configuration.
- The 6 doctests are all ` ```compile_fail ` blocks on `BooleanSchema`, `BufferSchema`,
  `UriSchema`, `UrlSchema`, `JsonSchema`, `JsonschemaSchema`. They prove a distinct
  contract — the builder types **reject** methods that do not belong to them — at compile
  time, not at run time. This is why they get their own gate.
- `cargo clippy --all-features --all-targets -- -D warnings` is currently **clean**.
- `cargo fmt --check` currently **fails** with a 3216-line diff — the codebase has never
  been rustfmt-formatted. Out of scope; see Step 7.
- `git check-ignore -v target/verify/logs/test.log` → `.gitignore:60:/target/`. The runner's
  artifacts are **already ignored**; `.gitignore` needs no change.
- Host toolchain, verified on this machine: `GNU bash 3.2.57(1)-release (arm64-apple-darwin25)`.
  `set -m` gives a background child its own process group (`pgid == child pid`),
  `kill -0 -<pgid>` tests group liveness, and `kill -TERM -<pgid>` / `kill -KILL -<pgid>` reap
  the group. The runner's process-group design depends on all four.
- `pgrep -g <pgid>` **requires a trailing pattern argument** on macOS. Bare
  `pgrep -g 12345` exits `2` with a usage error, which silently reads as "the group is gone" —
  a false negative that would make every group-liveness assertion in this plan vacuous. The
  correct form is `pgrep -g <pgid> .`, and it is used only where survivors are *listed*;
  liveness *tests* use `kill -0 -<pgid>`.
- A cargo-style group leader can exit before its descendants. Reproduced on this machine: the
  leader exited with status `0` while a `TERM`-ignoring descendant was still a live member of
  the group; the descendant survived `kill -TERM -<pgid>` and died only to `kill -KILL -<pgid>`.
  This is why the per-gate sequence confirms group death independently of the child's exit and
  why watchdog cancellation is conditional (Step 1, items 12–13).

## Design

### Three top-level gates

| Gate | Command | Build configuration |
|---|---|---|
| `lint` | `cargo clippy --all-features --all-targets -- -D warnings` | all-features |
| `test` | `cargo test --lib --all-features` | all-features (variant `core`: `cargo test --lib`) |
| `api` | `cargo test --doc --all-features` | all-features |

Rationale for exactly these three:

- `lint`, `test`, and `api` cover three different contracts: static lint cleanliness,
  run-time behaviour, and compile-time public-API type safety. No gate re-proves another
  gate's contract.
- The feature-off build is a **declared variant of `test`**, not a fourth gate — per the
  skill, feature configurations are variants under a gate.
- `lint` runs in the all-features configuration only. The feature-off configuration is
  proven to *compile* by the `test core` variant, so this is not a coverage hole; it is
  recorded as such in `docs/tests.md` so it does not read like one.
- No separate `build` gate: `cargo test` builds. Adding `cargo build` would violate the
  skill's rule against a build step a test command in the same gate already performs.

### Cache isolation

Each build configuration gets its own target directory, so the two feature sets never
invalidate each other:

- all-features (`lint`, `test` default variant, `api`) → `CARGO_TARGET_DIR=target/verify/lua`
- feature-off (`test core`) → `CARGO_TARGET_DIR=target/verify/core`

Verified to work: `CARGO_TARGET_DIR=target/verify/lua cargo test --lib --all-features`
passes 259 tests. Cost: one cold build per directory (~7 s / ~6 s); afterwards every gate is
warm regardless of the order gates run in.

### `verify all`

`verify all` runs, in one invocation, exactly once each, with no agent decision in between:

```
lint → test (all-features) → test core → api
```

Cold total ≈ 15 s, warm total ≈ 2 s.

## Steps

The steps are split into two phases with a hard ordering gate between them. **Phase I** is
Implementation and ends at `<implementation_ready>`. **Phase II** is Doc Update (CLAUDE.md §6)
and starts only after the Reviewer's `<approved>`.

Within Phase I there is a second hard gate: `docs/tests.md` documents the runner's
**measured** behaviour, so it is written only after Verification A–F is green. This is how
Hard Rule 12 is satisfied for the plan's own deliverable.

---

### Phase I — Implementation

#### Step 1 — `scripts/verify`

Create `scripts/verify`, `chmod +x`. Shebang `#!/usr/bin/env bash`; must run under macOS
system bash 3.2, so **no** associative arrays, `mapfile`, or `${var^^}`.

**Usage**

```
scripts/verify all                  # lint, test (both variants), api
scripts/verify lint
scripts/verify test [FILTER]        # all-features variant
scripts/verify test core [FILTER]   # feature-off variant
scripts/verify api [FILTER]
```

`FILTER` is passed through as the libtest name filter (e.g. `scripts/verify test strip_unknown`).
A filtered run is a **targeted run for diagnosis**; `docs/tests.md` records that it does not
satisfy a required gate.

**Exit-status contract** — this table is normative for the script and is reproduced in
`docs/tests.md`:

| Status | Meaning |
|---|---|
| `0` | every requested gate passed |
| `2` | `BUSY` — another verification run holds the lock; nothing was executed |
| `64` | usage error; nothing was executed |
| `70` | the runner could not establish process-group ownership of one of its owned subprocess groups — the gate child (step 8, or the handler's discovery attempt) or the watchdog (step 10); the lock is retained |
| `71` | the owned process group survived `TERM` **and** `KILL`; the lock is deliberately retained |
| `124` | a gate exceeded its timeout |
| other | the failing gate's child exit status, passed through verbatim (cargo typically `101`) |

**CLI argument handling.** All of these print a one-line error plus the usage block to
**stderr** and exit `64`, executing nothing and taking no lock:

- no arguments at all;
- an unknown first argument (anything other than `all`, `lint`, `test`, `api`);
- `all` or `lint` followed by any further argument (neither accepts a filter or a variant);
- `test` or `api` followed by more than one argument, except the exact form
  `test core [FILTER]`;
- `api core …` — `core` is a variant of `test` only.

`scripts/verify test core` with no filter is valid. `--help`/`-h` prints the usage block to
**stdout** and exits `0`.

**Group-liveness primitive.** Wherever the runner or a verification step asks "is the owned
group still alive?", the answer comes from `kill -0 -<pgid>` (exit 0 ⇒ at least one member
remains). It forks nothing and works in bash 3.2. The runner is never a member of the child's
group, so it cannot signal itself.

Beware: `pgrep -g <pgid>` **requires a pattern argument** on macOS —
bare `pgrep -g 12345` exits `2` with a usage error, which naively reads as "group is gone".
Where a verification step wants to *list* survivors rather than test for them, the correct
form is `pgrep -g <pgid> .`. Verified on the target machine.

**Terminating a process, bounded.** `TERM` → `wait` → `KILL` is a **deadlock**, not a fallback:
`wait` does not return while the child is alive, so the `KILL` that would free it is
unreachable. Measured on the target machine — a `TERM`-ignoring child left `wait` blocked
until an external `KILL` arrived 3 s later; without that rescue it would have blocked
forever. Every termination in this runner therefore uses one shape:

> `kill -TERM <target>` → poll liveness at 0.1 s intervals up to 5 s → `kill -KILL <target>`
> if still alive → poll up to 2 s more → **branch on the final poll**:
> - **still alive** → take the terminal outcome *immediately*. Do **not** `wait`.
> - **gone** → `wait` (direct child only), which now returns promptly, then continue.

The final-poll branch is the whole point of the ordering and applies at **every** bare-PID
call site. Putting `wait` before the survival check reintroduces the deadlock by another
route: if the target survived `TERM` and `KILL`, `wait` blocks forever and the terminal
outcome is unreachable — the runner hangs instead of reporting `71`. `wait` is only ever a
reaping step for a target already proven dead, never a waiting step.

Liveness is `kill -0 <pid>` for a direct child and `kill -0 -<pgid>` for a group. Bash reaps
background children asynchronously, so `kill -0` on a child that has already exited correctly
reports "gone" — verified, no zombie false-positive, and therefore no wasted 5 s poll on the
normal path.

**Runner state.** Eleven variables, all initialised to empty/zero **before any other work**,
because the cleanup handler reads them and may run at any point after installation:
`lock_owned=0`, `child_pid=`, `child_pgid=`, `watchdog_pid=`, `watchdog_pgid=`,
`timed_out=0`, `cleanup_done=0`, and the pending-terminal quartet
`pending_status=`, `pending_kind=`, `pending_id=`, `pending_reason=`.

The pending-terminal quartet exists because the cleanup handler has **two** targets to clean
— the watchdog group and the gate group — and discovering that the first one is unprovable
must not stop it from cleaning the second, nor let it fall through to releasing the lock.
Instead of exiting mid-handler, a failed cleanup *records* its terminal outcome and the
handler finishes the remaining safe work before acting on it. Recording rule: the first
failure wins **unless** a later one is a gate-group failure, which takes precedence over a
watchdog failure because only the gate group holds `CARGO_TARGET_DIR`; when a gate failure
displaces a watchdog one, append `(watchdog cleanup was also unproven)` to the reason so
neither is lost.

**Startup sequence.** Order is normative; the numbered steps are the state machine the
cleanup handler is written against.

1. Validate arguments (see above). On any usage error, exit `64` **before** the trap is
   installed and before any lock exists.
2. `mkdir -p target/verify target/verify/logs`.
3. **Install the cleanup handler on `EXIT INT TERM HUP` — before attempting the lock.** This
   closes the interruption window that a post-`mkdir` installation would leave. It is safe
   because every destructive action in the handler is guarded: the handler removes the lock
   directory only when `lock_owned=1`. A BUSY invocation therefore never deletes another
   runner's lock.
4. Acquire the lock: `mkdir target/verify/.lock` (atomic). On success set `lock_owned=1`
   **as the very next command**, then write the requested scope to
   `target/verify/.lock/scope` and this shell's PID to `target/verify/.lock/owner`.
5. On `mkdir` failure the lock is held elsewhere. The metadata may be **absent or
   half-written** — either because a contender arrived while a perfectly valid owner was
   between step 4's `mkdir` and its `owner`/`scope` writes, or because a signal landed in
   that same window and left a lock nobody owns. These two causes are indistinguishable from
   outside, so read defensively and classify into exactly three cases. Read
   `target/verify/.lock/owner`; treat it as *usable* only if the file exists, is readable, is
   non-empty, and matches `^[0-9]+$`. Read `scope` the same way, substituting `unknown` when
   it is missing or empty.
   - **Owner unusable** (missing, empty, non-numeric, or unreadable): **do not run `kill -0`**
     — an invalid or empty operand is a usage error whose non-zero status would misread as
     "the owner is dead". Print `BUSY — verification lock present with incomplete metadata
     (target/verify/.lock); if no runner is active, remove it manually`. Claim nothing about
     staleness: a live runner mid-startup lands here too.
   - **Owner usable and `kill -0 <pid>` succeeds:** print
     `BUSY — verification already running: <scope>`.
   - **Owner usable and `kill -0 <pid>` fails:** the owner is provably dead. Print
     `BUSY — stale lock from dead pid <n>: remove target/verify/.lock`.

   All three exit `2` immediately, with `lock_owned` still `0` so the handler leaves the
   foreign lock untouched. **Do not wait, do not retry, and do not take over any lock
   automatically** — silent takeover could let two runners share one build directory.

The sub-millisecond window between step 4's `mkdir` succeeding and `lock_owned=1` executing
is real and cannot be closed: bash 3.2 offers no primitive that makes those atomic. Its
consequence is a lock with no owner file, which step 5 reports as *incomplete metadata* — not
as proven-stale, because the runner cannot tell that state apart from a healthy runner two
instructions into its startup. Record the recovery wording in `docs/tests.md`. Do not invent
automatic recovery to paper over it.

**Per-gate sequence.** For each gate the runner owns exactly one child process group.

6. Truncate `target/verify/logs/<gate>.log` (`<gate>` is `lint`, `test`, `test-core`, `api`)
   and remove any stale `target/verify/logs/<gate>.timeout` marker. Set `timed_out=0`.
7. `set -m` so the background child becomes its own process-group leader, launch the gate
   command with `CARGO_TERM_COLOR=never` and the gate's `CARGO_TARGET_DIR`, redirecting
   **both** stdout and stderr into the log file, in the background — and set `child_pid=$!`
   **immediately after the `&`, before `set +m`**. The assignment must be the next command
   executed, not the next one after restoring monitor mode: a signal delivered in between
   would enter the handler with no recorded PID while a live process group is already
   running, and the handler would then release the lock. Restore with `set +m` only after
   `child_pid` is recorded.
8. Read the child's process group: `ps -o pgid= -p "$child_pid"`. If it does not equal
   `child_pid`, the runner cannot group-kill its descendants. **Abort**: terminate the child
   that is already running using the bounded shape defined above — `TERM`, poll
   `kill -0 "$child_pid"` up to 5 s, `KILL` if needed, poll up to 2 s — then branch on that
   final poll. If the child is **still alive**, go straight to
   `terminal_exit 71 process "$child_pid" "child survived TERM+KILL"` **without waiting**; a
   `wait` here would block forever on exactly the child that made the branch necessary. Only
   if it is **gone** does `wait "$child_pid"` run, to reap. Either way, **not**
   `TERM` → `wait` → `KILL`, which cannot reach its own `KILL`.

   Once the child is reaped, call `terminal_exit 70 process "$child_pid" "process-group
   ownership could not be established"` — which **retains the lock**.

   The lock is retained *even when the direct child was reaped cleanly*, and that is the
   whole point of this branch. Group ownership is precisely the capability that failed, so
   reaping the cargo PID proves nothing about rustc processes it had already forked. Those
   descendants are re-parented when the child dies and are no longer enumerable from this
   runner — there is no PGID to sweep and no reliable parent link left to walk. Releasing the
   lock would hand the next invocation a `CARGO_TARGET_DIR` that unknown survivors may still
   be writing to, which is the exact hazard the lock exists to prevent. Retention is the only
   disposition that stays honest about what the runner cannot know.

   There is no degraded fallback. Letting rustc descendants survive a timeout or interrupt
   would violate the verification skill's process-group rule, so this is a Hard-Rule-7
   infeasibility, not a mode of operation. (Measured to hold on the target machine; this
   branch is a guard, not an expected path.)
9. Set `child_pgid` and persist the PGID to **two** places: `target/verify/.lock/pgid` (the
   handler's source of truth) and `target/verify/logs/<gate>.pgid` (survives the run so
   Verification E/F can assert on the group afterwards).
10. Start a watchdog **in its own process group** — `set -m`, launch the subshell in the
    background, set `watchdog_pid=$!` **immediately after the `&`, before `set +m`** (same
    reason as step 7), then `set +m`, read `watchdog_pgid` with
    `ps -o pgid= -p "$watchdog_pid"`, and persist it to `target/verify/logs/<gate>.wpgid` so
    verification can assert on it after the run. The subshell body is: `sleep 300`;
    `touch target/verify/logs/<gate>.timeout`; `kill -TERM -<child_pgid>`; `sleep 5`;
    `kill -KILL -<child_pgid>`. 300 s is ~20× the measured cold worst case.

    The own-group requirement is not symmetry with the gate child; it is a measured leak fix.
    A watchdog subshell forks `sleep` as a *child*, and killing plus `wait`ing the subshell
    PID alone leaves that `sleep` running: reproduced on the target machine, where the sleep
    child was still alive after the watchdog was reaped. Every normally-completed gate would
    otherwise strand a `sleep` for the remainder of the timeout — four per `verify all`.
    Group-killing the watchdog reaps coordinator and `sleep` together, also verified.

    **If `watchdog_pgid != watchdog_pid`** (or the `ps` read yields nothing), the watchdog's
    own descendants are unsweepable. Ownership of the *gate* group is proven at this point
    (step 8 aborted otherwise), so this branch is not symmetric with step 8 and must clean up
    both targets in order:
    1. Run the full bounded group escalation on `child_pgid` — it is known, so the gate child
       and its rustc descendants **can** be swept, and leaving them running would be a
       gratuitous second leak.
    2. Terminate the watchdog as far as is safely possible: the bounded shape on the bare
       `watchdog_pid`, branching on the final poll as that shape requires — if the coordinator
       is still alive, go straight to `terminal_exit 71 process "$watchdog_pid" "watchdog
       coordinator survived TERM+KILL"` without waiting; only if it is gone does `wait
       "$watchdog_pid"` reap it. Its `sleep` child cannot be reached either way — that is
       precisely what failed.
    3. `terminal_exit 70 process "$watchdog_pid" "watchdog process-group ownership could not
       be established"`. Report the **watchdog** PID, not the already-reaped cargo PID: the
       cargo child is provably gone here, and naming it would point recovery at the wrong
       process. The lock is retained because the watchdog's descendants are unprovable.
11. `wait "$child_pid"` and capture the exit status. Clear `child_pid`.
12. **Watchdog handling is conditional on whether it fired** — this is the crux, so it is
    spelled out. A cargo leader can exit *before* its descendants: measured on the target
    machine, the group leader exited with status `0` while a `TERM`-ignoring descendant was
    still a live member of the group and only died to `KILL`. Unconditionally cancelling the
    watchdog after `wait` would therefore abort the escalation mid-grace-period and leave
    that descendant orphaned, with `.lock/pgid` already removed and the handler blind to it.
    - **Marker absent — normal completion.** Cancel the watchdog by **its whole group**, never
      its bare PID (that is the leak documented in step 10), and in the bounded order, never
      `TERM` → `wait` → `KILL`:
      `disown "$watchdog_pid"` → `kill -TERM -"$watchdog_pgid"` → poll `kill -0
      -"$watchdog_pgid"` at 0.1 s up to 2 s → `kill -KILL -"$watchdog_pgid"` if anything
      remains → poll up to 2 s more. If it survives both bounds, call
      `terminal_exit 71 group "$watchdog_pgid" "watchdog group survived TERM+KILL"` **before**
      clearing anything — clearing first would lose the identifier the report needs. Only once
      the group is confirmed gone, clear `watchdog_pid` and `watchdog_pgid`.

      **Then re-read the timeout marker.** Selecting this branch proved only that the marker
      was absent *at the moment of the check*; the watchdog could have woken, created it, and
      signalled the gate group between that check and the `disown`. The runner would otherwise
      keep `timed_out=0` and report `PASS` for a gate its own watchdog had just killed. Once
      the watchdog group is confirmed gone, **no process can still create the marker**, so a
      re-read at that point is conclusive: if `target/verify/logs/<gate>.timeout` now exists,
      set `timed_out=1`. Step 13 then finishes the gate-group cleanup as usual and step 14
      reports `TIMEOUT`.

      There is deliberately **no `wait`** on this path, and the `disown` is load-bearing. Bash
      announces a killed job on stderr — measured: `bash: … Terminated: 15 … ( sleep 300 )` —
      whenever it notices the job died at a command boundary before `wait` consumes the
      status, which the poll loop between `TERM` and `wait` makes likely. Four gates × one
      such line would break the promised four-line success output before any gate failed.
      `disown` removes the job from the table so the notice is never emitted; verified on the
      target machine to leave neither a notice nor a zombie (`ps` shows nothing for the PID
      afterwards). The scoped group poll replaces `wait` as the reaping proof and is strictly
      stronger — `wait` only ever observed the coordinator, never the `sleep` descendant that
      caused this whole branch.
    - **Marker present — the watchdog fired.** Set `timed_out=1` and **do not cancel it**;
      let its `TERM` → `sleep 5` → `KILL` sequence run to completion, but bounded, not by an
      open `wait`: poll `kill -0 -"$watchdog_pgid"` up to 10 s (its own sequence plus margin).
      If it is still alive after that, run the bounded escalation on its group. **Check for
      survival before `wait` and before clearing anything** — if the group survives the
      escalation, call `terminal_exit 71 group "$watchdog_pgid" "watchdog group survived
      TERM+KILL"` at that point. Reaching `wait` first would let an unkillable coordinator
      block it indefinitely, and clearing `watchdog_pgid` first would discard the identifier
      the report needs. Only once the group is confirmed gone, `wait "$watchdog_pid"` to reap
      — it returns promptly now — and clear both variables.
      No `disown` here: this path ends in `TIMEOUT`/exit `124`, so a stray job-control line on
      stderr costs nothing, and keeping `wait` preserves the reap.
13. **Confirm the group is gone before releasing anything.** Poll `kill -0 -<pgid>` at 0.1 s
    intervals for up to 5 s. If members remain, `kill -KILL -<pgid>` and poll again for up to
    2 s. Only once the group is confirmed dead, remove `target/verify/.lock/pgid` and clear
    `child_pgid`. If the group *still* exists after both bounds, take the terminal branch
    below.
14. **Timeout detection is by marker file, not by exit status** — a `SIGTERM`'d cargo and a
    genuinely failing cargo can produce the same status. If `timed_out=1`, report
    `TIMEOUT <gate> — killed after 300s — log: <path>` and exit `124`. Otherwise the child's
    exit status from step 11 is authoritative for pass/fail.

Never decide pass/fail by reading the log. Do **not** build a live pipeline
(`cargo … | tee | tail`) — redirect to the file, then read the file.

**Terminal exits — `terminal_exit <status> <kind> <id> <reason>`.** Both `70` and `71` end the
run with the lock **retained**, so they share one routine with **exactly one owner per
invocation**. It is parameterised by target kind because it is reachable on paths where no
PGID exists: step 8 and the handler's direct-child branch have only a bare PID, and printing
`process group <pgid>` or running `pgrep -g <pgid> .` there would name an identifier the
runner never established.

`<kind>` is `group` or `process`; `<id>` is the PGID or the PID accordingly.

1. Print to stderr `FATAL: <reason>; lock retained at target/verify/.lock`.
2. Report the target, by kind:
   - `group`: `surviving process group <id>:` followed by `pgrep -g <id> .` (the trailing
     pattern is mandatory on macOS).
   - `process`: `direct child <id>:` followed by `ps -o pid=,stat=,command= -p <id>`, then the
     line `descendants of this child cannot be enumerated (no owned process group)`. On the
     step-8 path the child may already be reaped, so this listing is often empty — that is
     honest output, not a bug: what is unknown is the descendants, not the child.
3. **Leave the lock in place**, whatever `lock_owned` says.
4. Set `cleanup_done=1` — this marks terminal cleanup complete and is what stops the `EXIT`
   handler from repeating the whole escalation.
5. `exit <status>`.

Callers:

| Caller | Call |
|---|---|
| Step 8 — gate-child ownership check failed | `terminal_exit 70 process "$child_pid" "process-group ownership could not be established"` |
| Step 10 — watchdog ownership check failed | `terminal_exit 70 process "$watchdog_pid" "watchdog process-group ownership could not be established"` |
| Handler item 3 — watchdog PGID undiscoverable | `terminal_exit 70 process "$watchdog_pid" "watchdog process-group ownership could not be established during cleanup"` |
| Handler item 4 — gate PGID undiscoverable | `terminal_exit 70 process "$child_pid" "process-group ownership could not be established during cleanup"` |
| Step 13 / handler item 5 — gate group survived | `terminal_exit 71 group "$child_pgid" "process group survived TERM+KILL"` |
| Handler item 5 — bare child survived, PGID never read | `terminal_exit 71 process "$child_pid" "child survived TERM+KILL"` |
| Step 12 normal branch — watchdog group survived cancellation | `terminal_exit 71 group "$watchdog_pgid" "watchdog group survived TERM+KILL"` |
| Step 12 timeout branch — fired watchdog's group survived | `terminal_exit 71 group "$watchdog_pgid" "watchdog group survived TERM+KILL"` |
| Handler item 3 — known watchdog group survived | `terminal_exit 71 group "$watchdog_pgid" "watchdog group survived TERM+KILL"` |

The four handler rows are *recorded* into the pending-terminal quartet by handler items 3 and
4 and dispatched by handler item 5; the table lists the effective call so every terminal
outcome is visible in one place.

A surviving **watchdog** group is materially different from a surviving **gate** group, and
the reports must not blur them: the watchdog's identifier is recorded in
`target/verify/logs/<gate>.wpgid`, not `<gate>.pgid`, and its members are a shell and a
`sleep` — they do **not** hold `CARGO_TARGET_DIR`. The lock is still retained (the runner
cannot prove what it cannot kill), but the recovery is less urgent and looks at a different
place. `docs/tests.md` records both variants under exit `71`.

Without step 4 the main path would re-enter: `exit` fires the `EXIT` trap, the handler would
find the same `child_pgid` still set, run a second full `TERM`/poll/`KILL`/poll cycle — up to
seven more seconds — and print the fatal report a second time. `cleanup_done` makes the first
caller the terminal owner and turns the handler into a no-op.

A retained lock is the correct outcome for both statuses: the next invocation must be refused
while processes that this run started may still hold its build directory — proven survivors on
the `71` path, unenumerable ones on the `70` path. Recovery is manual, differs per status, is
**not** a bare `rm -rf`, and is documented in `docs/tests.md` (Step 4a). Neither branch is
expected to fire.

**Cleanup handler** (`EXIT INT TERM HUP`), safe to run in any state, including before the
lock exists and between child launch and PGID discovery:

Items 3 and 4 both **record** into the pending-terminal quartet instead of exiting; item 5 is
the single place that acts on it. That is what lets a failed watchdog cleanup still be
followed by a full gate-group cleanup, and what stops either failure from silently reaching
the lock release in item 6.

1. Disarm itself (`trap '' EXIT INT TERM HUP`) so a second signal cannot re-enter it.
2. **If `cleanup_done=1`, do nothing else and re-exit with the original status.** Terminal
   cleanup already ran and already decided the lock's fate.
3. **Watchdog cleanup.** If `watchdog_pgid` is set: `disown "$watchdog_pid"`,
   `kill -TERM -"$watchdog_pgid"`, poll `kill -0 -"$watchdog_pgid"` up to 2 s, `KILL` the group
   if anything remains, poll up to 2 s more. The whole group, not the bare PID — otherwise the
   watchdog's `sleep` outlives the run (step 10) — and in the bounded order, never
   `TERM` → `wait` → `KILL`. If the group **survives** both bounds, record
   `71 group "$watchdog_pgid" "watchdog group survived TERM+KILL"` and **do not clear**
   `watchdog_pgid` — the report needs that identifier. Otherwise clear the watchdog variables.

   If only `watchdog_pid` is set (signal landed between launch and the PGID read), first try
   to discover the group as step 10 would (`ps -o pgid= -p "$watchdog_pid"`) and use the group
   path if it resolves. Otherwise apply the bounded shape to the bare PID and branch on its
   final poll: **still alive** → record `71 process "$watchdog_pid" "watchdog coordinator
   survived TERM+KILL"` and do **not** `wait`; **gone** → `wait "$watchdog_pid"` to reap, then
   record `70 process "$watchdog_pid" "watchdog process-group ownership could not be
   established during cleanup"` — its `sleep` may survive, so the run is unprovable.

   Either way, **continue to item 4**. A recorded failure never short-circuits the gate-group
   cleanup, which is the more important of the two.
4. **Gate cleanup.** If `child_pgid` is set: run the step 13 escalation — `kill -TERM -<pgid>`,
   poll `kill -0 -<pgid>` at 0.1 s up to 5 s, `kill -KILL -<pgid>`, poll up to 2 s. If the
   group survives both bounds, record `71 group "$child_pgid" "process group survived
   TERM+KILL"` and do not clear `child_pgid`.

   Else if `child_pid` is set (launched, PGID not yet read): **attempt PGID discovery here
   rather than assuming the bare PID is enough** — read `ps -o pgid= -p "$child_pid"`. If it
   resolves and equals `child_pid`, take the group path above; the window is short but cargo
   may already have forked rustc, so a bare-PID kill would strand them.
   If discovery fails or the PGID does not match, the runner cannot prove the spawned tree is
   gone. Apply the bounded shape to the bare PID — `kill -TERM "$child_pid"`, poll
   `kill -0 "$child_pid"` up to 5 s, `kill -KILL` if needed, poll up to 2 s — then branch on
   that final poll, **never** `TERM` → `wait` → `KILL`:
   - **Still alive:** record `71 process "$child_pid" "child survived TERM+KILL"` and do
     **not** `wait`. Waiting on a child that survived `KILL` blocks forever and the recorded
     outcome would never be dispatched by item 5.
   - **Gone:** `wait "$child_pid"` to reap, then record `70 process "$child_pid"
     "process-group ownership could not be established during cleanup"`.

   **Direct-PID death is not
   sufficient to release the lock**, for exactly the reason step 8 retains it: the descendants
   are unenumerable. This is cleanup for a child whose group is not yet known, **not** the
   degraded per-gate fallback that step 8 rejects.
5. **If anything is pending, act on it now:** `terminal_exit "$pending_status" "$pending_kind"
   "$pending_id" "$pending_reason"`. Both cleanups have already run, so this is the last thing
   the handler does, and the recorded kind/id always match the branch that actually failed —
   the report can never name a PGID that path never established.
6. Otherwise, if `lock_owned=1`, remove the lock directory — **last**, only after **both** the
   watchdog group and the gate group are confirmed gone. A Ctrl-C must never release the lock
   while this run's cargo/rustc processes are alive. If `lock_owned=0`, leave the directory
   alone; it belongs to someone else.
7. Re-exit with the original status.

**Success output.** Exactly one line per gate:

- `test`, `test core`, `api`: `PASS <gate> — <n> tests, <duration>`, where `<n>` is summed
  from the `test result: ok. <n> passed` lines in the log. This is presentation only; the
  exit status already decided the verdict. If no such line parses, print
  `PASS <gate> — <duration>` — an unparseable count must never turn a pass into a failure.
- `lint`: `PASS lint — <duration>` (no test count).

`<duration>` is whole seconds, e.g. `3s`, derived from bash's `SECONDS`.

Nothing else may reach stdout or stderr on a successful run — in particular no job-control
notices from the runner's own bookkeeping. `disown`ing the watchdog before cancelling it
(step 12) is what guarantees that; B6 counts the lines and would catch a regression.

**Failure output.** Print, in this order, and nothing more:

1. `FAIL <gate> — <duration>`
2. A diagnostic, chosen by the first rule that yields at least one line — every branch is
   capped at 20 lines, with `… and <k> more` when the cap bites:
   - **(a)** the test-name lines of the log's `failures:` block (test gates: normal
     assertion failures);
   - **(b)** otherwise, lines matching `^error` — `error:`, `error[E0308]:`,
     `error: could not compile …` (clippy findings, and compile / link / dependency-resolution
     / Cargo failures, which produce no `failures:` block at all);
   - **(c)** otherwise, the last 15 non-empty lines of the log, introduced by
     `no diagnostic matched; log tail:`. This branch is a safety net for an unanticipated
     failure mode: on the current toolchain essentially every cargo failure emits an
     `^error` line, so (c) is not reachable through project code. It is retained rather than
     dropped because its absence is exactly the "`FAIL` plus a bare log path" hole, and it is
     proved by D14 with a runner-level fixture instead of a natural failure.
3. `log: target/verify/logs/<gate>.log`

Never print the full log. `verify all` stops at the first failing gate and exits with that
gate's status.

#### Step 2 — `.gitignore` (no change expected)

`/target/` at `.gitignore:60` already covers `target/verify/`, confirmed by
`git check-ignore -v target/verify/logs/test.log`. **This plan makes no `.gitignore` change.**
Re-run that check once; only if it unexpectedly reports the path as *not* ignored, add
`target/verify/` with a one-line comment and say so in the handoff.

Note for the Coder: the worktree already carries **unrelated, pre-existing** modifications to
`.gitignore` (2 insertions, from the "ignore locally synced agent rule and skill assets"
work). Do not attribute them to this plan and do not revert them.

#### Step 3 — Verify the runner

Run Verification groups **A–F**. All must be green before Step 4. If any group fails, fix
`scripts/verify` and re-run — do not proceed with documentation.

#### Step 4 — `docs/tests.md`

Write the one-page contract, describing the behaviour Step 3 just proved. Required content:

**a) Entrypoint** — `scripts/verify`, the usage block, the exit-status table from Step 1, and
the statement that `verify all` is the pre-commit / final-validation gate.

Include a short **"a lock was left behind"** subsection covering all four cases. They are not
interchangeable, and the differences matter:

- `BUSY — stale lock from dead pid <n>` — the recorded owner is provably dead, so nothing is
  holding the build directory. Safe to clear directly: `rm -rf target/verify/.lock`.
- `BUSY — verification lock present with incomplete metadata` — the runner cannot tell an
  abandoned lock from a healthy runner two instructions into startup. Check first whether a
  `scripts/verify` is actually running; clear it only if none is.
- **exit `71`** — the lock is retained *deliberately*, because something this run started
  survived `TERM` and `KILL`. **Not** a `rm -rf` case. The `FATAL` line names which of three
  targets survived, and they are not equally urgent:
  - *gate process group* — identifier also in `target/verify/logs/<gate>.pgid`. These are
    cargo/rustc processes that **do** hold this run's `CARGO_TARGET_DIR`. Clearing the lock
    while they live lets the next runner share the build directory with them, which is exactly
    the corruption the retention prevents.
  - *watchdog process group* — identifier in `target/verify/logs/<gate>.**w**pgid`, a
    different file. Its members are a shell and a `sleep`; they do **not** hold
    `CARGO_TARGET_DIR`, so nothing is being corrupted. The lock is still retained because the
    runner cannot prove what it cannot kill, but the remediation is a stuck `sleep`, not a
    build hazard.
  - *bare child* — a PID rather than a PGID, on the path where the group was never read.

  Recovery in all three: inspect the survivors with `pgrep -g <pgid> .` (the trailing pattern
  is required on macOS) or `ps -p <pid>`, remediate by hand, confirm independently that
  `kill -0 -<pgid>` — or `kill -0 <pid>` — now **fails**, and only then remove the lock.
- **exit `70`** — the runner could not establish process-group ownership of **one of its two
  owned subprocess groups**, and aborted. Which one is named in the `FATAL` line, and it
  changes what to look for:
  - *gate child* (step 8, or the cleanup handler's discovery attempt) — the direct cargo child
    was reaped, but any **rustc descendants it had already forked** could not be enumerated or
    killed; they are re-parented and no longer reachable from the runner.
  - *watchdog* (step 10) — the gate group was swept normally, but the watchdog's **`sleep`
    child** could not be reached. Harmless to the build directory, but still unproven.

  Either way the lock is retained, and this is the *least* safe case to clear casually: unlike
  exit `71` there is no group identifier to re-check, so confirming the tree is gone is
  genuinely manual. Recovery: verify nothing is still writing under `target/verify/` — inspect
  candidates by hand (`ps`/`lsof +D target/verify`) rather than trusting a bare
  `pgrep -f 'rustc|cargo'`, which also matches unrelated developer and agent processes —
  terminate what belongs to the aborted run, and only then remove the lock. Exit `70` also
  signals a host-environment problem worth reporting, not just a lock to clear: the runner's
  process-group assumption did not hold.

**b) Gate table** — one row per gate *and variant*, with the exact runner invocation:

| Gate | Command | Covers | Excludes | Cold / warm | When |
|---|---|---|---|---|---|
| `lint` | `scripts/verify lint` | clippy over lib + tests, all features, warnings are errors | run-time behaviour | ~3 s / <1 s | inner loop, `all` |
| `test` | `scripts/verify test` | all in-module tests with `lua` enabled | doctests, feature-off compile | ~7 s / <1 s | inner loop, `all` |
| `test core` | `scripts/verify test core` | all in-module tests with `lua` disabled — proves the feature-off configuration compiles and passes | the `lua`-gated tests | ~6 s / <1 s | `all`, and any plan touching `#[cfg(feature = "lua")]` |
| `api` | `scripts/verify api` | the `compile_fail` doctests — public builder types reject foreign methods | run-time behaviour | ~1 s / <1 s | `all`, and any plan changing the public API surface |
| *(all)* | `scripts/verify all` | the four rows above, once each | — | ~15 s / ~2 s | pre-commit, final validation |

**Describe inclusions structurally, never as a test count.** No `259`, `212`, `47`, or `6`
anywhere in `docs/tests.md`: the runner reports the live count on every PASS line, and a
hardcoded number would make the supposedly stable contract stale the moment anyone adds a
test. The exact counts belong to this plan's baseline verification (group A) and stay there.

**c) Test inventory by area** — one row per area with boundary and perspective. **No
individual test names, and no counts.** Use the "Discovered state" table above for the
area→concern mapping, and give each area its perspectives, e.g.:

- `value` — unit / `value-model` — serde bridge round-trip, type mapping, serialization failure modes
- `schema` — unit / `schema-core` — modifier composition, parse flow, lazy & cyclic schemas, `MAX_PARSE_DEPTH` boundary
- `delta` — unit / `schema-core` — `parse_delta` and `replace` happy path, partial-update semantics, failure modes
- `types` — unit / `types` — per-type happy & sad paths, validator boundaries, special-type composition
- `json_schema` — unit / `json-schema` — export/import round-trip, draft 2020-12 conformance, `$defs`/`$ref`, `strip_unknown`, error paths
- `policy` — unit / `policy` — pipeline composition, transform ordering, error propagation
- `error` — unit / `errors` — code taxonomy, path aggregation, JSON shape, `std::error::Error` propagation
- `lua` (feature-gated) — unit / `lua` — schema-directed validation, error precedence, feature contract
- doctests — compile-time / public API surface — negative type-safety of the builder types

Spot-check each perspective claim against the actual test names in that module before writing
the row; do not copy a perspective the module does not cover, and add one it does.

**d) Rule for new tests** — intent, in descriptive contributor voice (no RFC 2119 keywords):
*name the boundary (unit / compile-time) and the perspective (happy path, failure mode,
boundary, round-trip, conformance, feature contract). If both are already covered for that
area, extend the existing `mod tests` block in the owning module; otherwise state in the plan
why a new suite is needed. Duplicating an already-covered boundary+perspective combination is
a Reviewer quality finding, not rigor.* Also record that zerx keeps tests in-module
(`#[cfg(test)] mod tests`) and has no `tests/` directory — a plan that wants one justifies it.

**e) Targeted runs and logs** — the `scripts/verify test <filter>` syntax, the log location
`target/verify/logs/<gate>.log`, and — phrased descriptively, **not** with `MAY`/`MUST NOT` —
that a filtered run or a raw `cargo` invocation is a diagnostic aid and does not satisfy a
required gate; the unfiltered gate still has to run.

**f) Feature-coverage note** — `lint` runs in the all-features configuration only; the
feature-off configuration is covered by `test core`.

#### Step 5 — Verify the documentation

Run Verification group **G**. Then emit `<implementation_ready>`.

---

### Phase II — Doc Update (only after the Reviewer's `<approved>`)

#### Step 6 — `README.md`

`README.md` is a two-half document: a human half, then the `## LLM Reference` hinge whose
final block is `**Invariants — things that will bite you if you assume otherwise:**`, then
`## License`.

Insert a compact `## Verification` section **between the end of the Invariants block and
`## License`**. This keeps Invariants last within the LLM Reference and preserves the
two-half hinge. Two or three sentences: `scripts/verify all` is the canonical entrypoint,
`docs/tests.md` is the contract. **No gate table** — the contract lives in exactly one place.

#### Step 7 — `docs/backlog.kanban.md`

**Format decision — this plan does not migrate the board.** `docs/backlog.kanban.md` is in a
legacy representation: `- [ ]` checklist items with a `>` blockquote body, `id:` metadata on
board and columns only, no `###` cards, no `priority` field, no `template:` field, no trailing
`<!-- markdown-kanban -->` self-description, and columns `Backlog / Next / In Progress / Done /
Someday` where the bundled template defines `Someday / Open / In Progress / Done`. Migrating it
is a structural change to a file this plan otherwise only appends to — out of scope under Hard
Rule 8. **Match the existing representation exactly** (checklist item + indented blockquote,
no new metadata keys, no new columns) and do not invoke the `markdown-kanban` creation
procedure. The migration itself becomes one of the cards below.

Append three items:

- **`## Backlog` — `rustfmt` gate.** `cargo fmt --check` currently fails with a 3216-line
  diff; the codebase has never been formatted. A `fmt` gate requires a one-time whole-repo
  reformat, which is a separate, Product-Owner-approved change. Deliberately excluded from V1.
- **`## Backlog` — migrate `backlog.kanban.md`/`bug.kanban.md` to the current three-heading
  format.** Surfaced during V1 plan review: the boards predate the `markdown-kanban` template
  (no `###` cards, no `template:`/`priority` fields, no embedded self-description, divergent
  column set). Cosmetic today, but it blocks any tooling that parses the template block.
- **`## Someday` — CI wiring.** There is no `.github/workflows/`. `scripts/verify all` is the
  natural single CI step if the project ever wants CI.

#### Step 8 — Confirm the permanent docs need no change

This plan changes no library behaviour, so `definition.md`, `decisions.md`, and
`docs/architecture/*` need **no** update. `docs/tests.md` is contributor guidance, not a
permanent normative doc, and is deliberately absent from `architecture/_overview.md`'s concern
list — that file's invariant is "every concern file in this directory MUST be listed", and
`docs/tests.md` is not in that directory. Confirm this explicitly rather than editing those
files.

#### Step 9 — Final gate and archive

Run Verification group **H**, then `scripts/verify all` once more as the final gate. Move
`docs/PLAN_V1_verification_gates.md` to `docs/archive/` and commit.

## Assumptions & Risks

- **Assumption:** macOS system `bash` 3.2 is the floor, with no `setsid` and no `timeout`.
  `set -m` + `kill -- -PGID` is therefore the only available group-kill mechanism. This was
  measured to work on the target machine (see "Discovered state"). If it ever does not,
  Step 1.3 stops the implementation as infeasible — there is no fallback that lets rustc
  descendants survive.
- **Assumption:** the whole suite is fast (cold ≈ 15 s, warm ≈ 2 s), so `verify all` is cheap
  enough to be the default inner-loop command. If test volume grows past ~60 s, the gate split
  needs revisiting; not a V1 concern.
- **Risk:** the split `CARGO_TARGET_DIR` means `target/` on disk grows by roughly two extra
  build trees. Acceptable for a dependency-light crate; `target/` is gitignored.
- **Risk:** the PASS-line test count is parsed from `test result: ok. <n> passed`. A future
  toolchain wording change loses the count, not the verdict — Step 1 requires the runner to
  degrade to `PASS <gate> — <duration>` rather than fail.
- **Risk:** a lock can outlive its runner in four defined ways, all reported by name rather
  than silently recovered, and **not** all cleared the same way. A dead recorded owner is
  provably safe to `rm -rf`. A lock with incomplete metadata — the `mkdir`/`lock_owned=1`
  window, indistinguishable from a runner mid-startup — is cleared only after confirming no
  runner is active. The exit-`71` retention may be removed only after the surviving group or
  child has been remediated by hand and `kill -0` on the reported identifier independently
  fails. The exit-`70` retention is the least safe of the four: there is no identifier at all,
  because ownership failure is what caused it, so the descendants are unenumerable and
  clearing the lock requires manually confirming nothing is still writing under
  `target/verify/`. In every retained case, removing the lock early hands the next runner a
  `CARGO_TARGET_DIR` that live processes may still be writing to — the exact hazard the
  retention prevents. All four are spelled out in `docs/tests.md`. Automatic takeover is
  rejected throughout: two runners sharing one build directory is a worse failure than a
  manual unlock.
- **Risk:** PGID reuse. `kill -0 -<pgid>` could in principle match a recycled group. The
  window is the few seconds between a gate's child dying and its PGID being cleared, and the
  consequence is a bounded extra wait, not a wrong verdict. Not mitigated.
- **Out of scope (deliberate, YAGNI):** rustfmt gate, CI, `cargo-public-api` surface diffing,
  coverage, benchmarks, retrofitting archived plans, migrating the kanban boards, creating
  `docs/conventions.md`.

## Verification

`docs/tests.md` does not exist yet at implementation time, so this plan's Verification is
stated in raw commands. It is the last plan that may do so.

Groups **A–F** run at Step 3, **G** at Step 5, **H** at Step 9.

**A. Baseline unchanged** — the runner must not alter what the project proves.

1. `cargo clippy --all-features --all-targets -- -D warnings` → exit 0.
2. `cargo test --all-features` → `259 passed; 0 failed` + `6 passed; 0 failed; 1 ignored`.
3. `cargo test` → `212 passed; 0 failed` + `6 passed; 0 failed; 1 ignored`.
4. `git diff --stat -- src/ Cargo.toml Cargo.lock` → **empty**, and stays empty at every later
   checkpoint. This plan touches no library code and no manifest.

**B. Gates agree with the baseline**

5. `scripts/verify all` → exit 0, printing **exactly four** lines: `PASS lint — …`,
   `PASS test — 259 tests, …`, `PASS test core — 212 tests, …`, `PASS api — 6 tests, …`.
   The counts must equal those from A2/A3.
6. Output volume, preserving the runner's verdict and avoiding a live pipeline:
   `scripts/verify all > /tmp/v1_all.out 2>&1; st=$?; wc -l < /tmp/v1_all.out` → `st` is `0`
   and the count is `4`. Success output must not exceed one line per gate.
7. Run `scripts/verify all` again immediately → still exit 0, and noticeably faster
   (warm ≈ 2 s vs. cold ≈ 15 s). This proves the split `CARGO_TARGET_DIR` prevents the
   feature-set cache thrash.

8. *No watchdog leaks on the normal path.* A watchdog subshell forks `sleep` as a child, and
   cancelling it by PID alone strands that `sleep` for the rest of the timeout — measured, and
   it would happen on **every** normally-completed gate, four per `verify all`. After the run
   in B5, for each of `lint`, `test`, `test-core`, `api` assert **both**:
   - `target/verify/logs/<gate>.wpgid` exists and matches `^[0-9]+$`. A missing or malformed
     file fails the check outright — it would otherwise make the next assertion vacuous by
     feeding `kill -0` an empty operand.
   - `kill -0 -"$(cat target/verify/logs/<gate>.wpgid)"` **fails**.

   Scoped to the recorded watchdog group, this covers coordinator *and* `sleep` together,
   because the `sleep` is a member of that group. Deliberately **no** global `pgrep -f
   'sleep 300'` cross-check: it matches any unrelated process on the machine, which is the
   same false-positive defect already removed from the cargo/rustc assertions, and it would
   not even name the right sleeper under a modified timeout.

**C. Test-gate failure — assertion path.** Use a harmless throwaway fixture; never break
project code.

9. Append to `src/error.rs`'s `mod tests`:
   `#[test] fn v1_runner_selfcheck() { assert_eq!(1, 2); }`.
   `scripts/verify test > /tmp/v1_fail.out 2>&1; echo $?` → non-zero (cargo's `101`).
   `/tmp/v1_fail.out` contains `FAIL test`, the name `v1_runner_selfcheck`, and
   `log: target/verify/logs/test.log`, and is **under 20 lines** — the log itself is 280+
   lines and must not be dumped.
10. `scripts/verify all` with the same fixture → stops at the `test` gate; no `PASS api` line.
11. Remove the fixture; `git diff -- src/` empty; `scripts/verify all` → exit 0.

**D. Failure paths with no `failures:` block** — D12 and D13 both exercise diagnostic branch
(b); D14 exercises branch (c), which no project-code fixture can reach.

12. *Lint (branch b).* Temporarily add to `src/error.rs`:
    `#[allow(dead_code)] fn v1_lint_selfcheck() { let v: Vec<u8> = Vec::new(); let _ = v.len() == 0; }`
    (clippy `len_zero` fires). `scripts/verify lint` → non-zero, output contains `FAIL lint`,
    at least one `error:` line naming the lint, and the log path; under 25 lines. Remove it;
    `git diff -- src/` empty.
13. *Compile error (branch b).* Temporarily append
    `#[test] fn v1_compile_selfcheck() { let _: u8 = "x"; }` to `src/error.rs`'s `mod tests`.
    `scripts/verify test` → non-zero, and the output contains a real diagnostic (an
    `error[E0308]`-style line), **not** just `FAIL` and a log path. This is the branch a
    `failures:`-only design would have left blank. Remove it; `git diff -- src/` empty.
14. *Branch (c) — no `failures:` block and no `^error` line.* Project code cannot produce this
    state: every cargo failure path on the current toolchain emits an `^error` line. Prove it
    at the runner level instead, the same way F18 proves the timeout — a temporary edit to the
    script, no project code touched. Temporarily replace the `api` gate's command with
    `sh -c 'echo some output; echo more output; exit 3'` (verified on the target machine to
    exit `3` with a log containing zero `^error` and zero `failures:` matches). Run
    `scripts/verify api` and assert all four:
    - exit status is **`3`** — the child's status is passed through verbatim, not flattened;
    - output contains `FAIL api`;
    - output contains `no diagnostic matched; log tail:` followed by the two output lines;
    - output contains `log: target/verify/logs/api.log` and is under 20 lines.

    Restore the real `api` command and confirm `scripts/verify api` exits 0.

**E. Concurrency lock and interruption**

15. *BUSY leaves the incumbent untouched.* Start `scripts/verify all` in the background; while
    it runs, capture `owner=$(cat target/verify/.lock/owner)` and run `scripts/verify test` in
    the foreground → it prints `BUSY — verification already running: all`, exits `2`, and
    returns **immediately** (it must not block). Then, before the first run finishes, assert
    the rejected invocation did no damage: `target/verify/.lock` still exists, its `owner`
    file still reads `$owner`, and its `scope` file still reads `all`. Let the first run
    finish → it exits 0 with its four `PASS` lines, and a subsequent `scripts/verify test`
    succeeds normally.
16. *Interruption reaps the owned group.* Start `scripts/verify all` from a cold
    `rm -rf target/verify/lua` state. While the `lint` gate runs, capture the owned group:
    `pgid=$(cat target/verify/.lock/pgid)`. Send `SIGINT` to the runner. Then assert **all
    three**:
    - `kill -0 -"$pgid"` fails (exit 1) — the runner's own process group is gone. Scoped to
      the recorded PGID, so it cannot be satisfied or defeated by unrelated cargo/rustc
      processes belonging to a developer or another agent. If it *does* succeed, list the
      survivors with `pgrep -g "$pgid" .` — note the mandatory pattern argument; bare
      `pgrep -g "$pgid"` is a macOS usage error that exits `2` and misreads as "gone".
    - `target/verify/.lock` no longer exists.
    - The group death is observable *before* the lock release, i.e. a `scripts/verify lint`
      started immediately afterwards acquires the lock and exits 0.
17. Usage errors take no lock: `scripts/verify`, `scripts/verify bogus`,
    `scripts/verify lint core`, `scripts/verify api core` → each exits `64`, prints usage to
    stderr, and leaves no `target/verify/.lock` behind. `scripts/verify --help` → exits `0` on
    stdout. `scripts/verify test core` (no filter) → exits `0`.

**F. Timeout.** F20 must run with the **restored** 300 s timeout: the plan's own measurement
puts a cold `lint` at ~3 s, and F18 has just deleted the all-features target directory, so a
2 s timeout would make F20 time out instead of proving cleanup.

18. Temporarily set the gate timeout to `2` seconds in the script. From a cold
    `rm -rf target/verify/lua` state run `scripts/verify test` → prints
    `TIMEOUT test — killed after 2s`, exits `124`, and
    `kill -0 -"$(cat target/verify/logs/test.pgid)"` fails. As in E16 the check is scoped to
    the recorded PGID, so it proves *this run's* descendants were reaped rather than that no
    cargo is running anywhere. `target/verify/.lock` no longer exists.
19. *The fired watchdog leaves nothing behind.* Still from F18's run, and **before** anything
    else runs: `target/verify/logs/test.wpgid` exists and matches `^[0-9]+$`, and
    `kill -0 -"$(cat target/verify/logs/test.wpgid)"` **fails**. That asserts coordinator
    *and* its `sleep` descendant are both gone, because the `sleep` is a member of the
    watchdog group. As in B8, no global `sleep`-name check: besides matching unrelated
    processes, F18 runs under the temporary 2 s timeout, so the fired watchdog's sleeper is
    not a `sleep 300` at all. Acquiring the lock in F20 does **not** prove this either: the
    lock is released on the gate-child's group, and a stranded watchdog `sleep` would not
    block it.
20. **Restore the timeout to 300 first**, then run `scripts/verify lint` → acquires the lock
    and exits 0. This proves the timeout path released the lock, without the restored-cold
    build racing the temporary 2 s bound.
21. `scripts/verify all` → exit 0, four `PASS` lines, and `git diff -- scripts/` empty (the
    F18 timeout edit and the D14 command edit are both reverted).

**G. `docs/tests.md`**

22. Exists, ≤ ~120 lines, contains the entrypoint + usage + exit-status table (including `70`,
    `71`, `124`), the gate table **with a command column populated for every gate and
    variant**, the per-area inventory with boundary + perspective, the new-test rule,
    targeted-run syntax, and the log path.
23. The "a lock was left behind" subsection distinguishes **all four** cases and does not
    prescribe the same remedy for them: dead recorded owner (`rm -rf` is safe), incomplete
    metadata (check for a live runner first — the doc must not call this state stale), exit
    `71` (remediate the reported target by hand, confirm `kill -0` on that identifier fails,
    and only then remove), and exit `70` (no identifier exists; confirm manually that nothing
    is still writing under `target/verify/` before removing).
    The exit-`71` entry names **all three** possible targets and keeps them distinct: gate
    process group (identifier in `<gate>.pgid`, holds `CARGO_TARGET_DIR`), watchdog process
    group (identifier in `<gate>.**w**pgid` — a different file — and explicitly *not* holding
    `CARGO_TARGET_DIR`), and bare child PID. The exit-`70` entry names its two sub-cases
    (gate child, watchdog) and states that descendants are unenumerable.
    Specifically: no line offers a bare `rm -rf target/verify/.lock` as the remedy for exit
    `70` or exit `71`.
24. Names **no individual test function**: no line contains `#[test]`, and no `snake_case`
    token matches a function name in any `mod tests` block.
25. Carries **no test counts**: `grep -E '\b(259|212|47|6)\b' docs/tests.md` returns nothing
    that is a test count (the durations `~6 s`/`~7 s` are fine — check the hits by eye).
26. Contains **no RFC 2119 keyword**: `grep -nE '\b(MUST|MUST NOT|SHOULD|SHOULD NOT|MAY)\b'
    docs/tests.md` → empty. It is contributor guidance, not a permanent normative doc.
27. Every area row's concern reference points to a file that exists in `docs/architecture/`.

**H. Doc Update (Phase II)**

28. `README.md` has a `## Verification` section positioned after the Invariants block and
    before `## License`; `grep -n '^## ' README.md` confirms the order
    `… ## LLM Reference … ## Verification, ## License`. The gate table is **not** duplicated
    there.
29. `docs/backlog.kanban.md` carries the three items from Step 7, in the columns named there,
    written in the file's existing `- [ ]` + blockquote representation. `grep -c '^### '
    docs/backlog.kanban.md` → `0` (the board was not migrated), and the column headings are
    unchanged.
30. `git diff --stat -- CLAUDE.md AGENTS.md docs/definition.md docs/decisions.md
    docs/architecture/ .gitignore` → empty apart from the pre-existing, unrelated `.gitignore`
    modifications noted in Step 2.
31. `scripts/verify all` → exit 0, four `PASS` lines. Final gate before archive.

## Proposed Decisions

None. Nothing here binds future plans beyond what `docs/tests.md` itself states as contributor
guidance, and CLAUDE.md §3 already binds plans to reference the project's verification gates.
No entry is proposed for `decisions.md`.
