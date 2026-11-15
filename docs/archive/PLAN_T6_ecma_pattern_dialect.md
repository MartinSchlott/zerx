# PLAN_T6 — ECMA-262 pattern dialect (intersection semantics)

## Context & Goal

JSON Schema `pattern` is an ECMA-262 regular expression. zerx compiles every pattern source directly with the `regex` crate in its default Unicode mode. That applies to sources from `string().regex(src)` / `.pattern(src)`, from an imported `pattern`, and from the static evaluation of `enum`/`const` siblings. The two dialects differ in ways that let zerx accept values the exported or imported schema forbids (`D-import-fail-closed`):

| Construct | ECMA-262 (`u` flag) | `regex` crate default |
|---|---|---|
| `\d` | `[0-9]` | any Unicode decimal digit (`"١٢٣"` matches) |
| `\w` | `[A-Za-z0-9_]` | any Unicode word character (`"é"` matches) |
| `\b`, `\B` | ASCII word boundary | Unicode word boundary |
| `\s` | the WhiteSpace + LineTerminator set | Unicode `White_Space` (adds U+0085, lacks U+FEFF) |
| `.` | any code point except `\n`, `\r`, U+2028, U+2029 | any code point except `\n` |
| `[a&&b]`, `[a~~b]`, literal `[` in a class | literal class members | set operations / nested class |
| `(?i)`, `\pL`, `\A`, `\z`, `(?P<n>…)` | invalid | supported |
| `(?=…)`, `(?<=…)`, `\1` | supported | not supported |

The bug card "`pattern` uses Rust regex semantics, not ECMA-262" tracks this. DevLead set the direction under its bug-sweep mandate: translate to a `regex` crate expression and refuse what cannot be mapped. Exact ECMA semantics would also widen some patterns relative to today (see *Where the dialects disagree*). Under the strictness-first rule, Martin (Product Owner) therefore chose **intersection semantics** from three options, relayed by DevLead. The other two were exact ECMA, which widens, and refusing every differing construct, which refuses common patterns.

**Goal.** Every pattern source zerx compiles is read as ECMA-262 with the `u` flag, restricted to a supported subset. Within that subset, each construct whose meaning differs between the dialects matches only what **both** dialects accept. Valid ECMA constructs outside the subset are refused. The result is a **deliberate conservative subset of ECMA-262, not an exact implementation**:
- it is never wider than today's behaviour;
- it is never wider than the exported `pattern`;
- for some inputs it is narrower than ECMA, and `docs/architecture/json-schema.md` names that as a deliberate narrowing.

Behaviour of other validators is unchanged.

Edition: the current ECMA-262 edition is the reference. Node 22 (`new RegExp(p, "u")`) serves as its proxy, except where Node 22 predates the standard (ES2025 modifier groups, duplicate named groups across alternatives). A construct that is valid in the current edition but cannot be held by the translation is `Unsupported`, never `Malformed`.

Dialect: `u`-flag semantics, so matching is per code point, as the `regex` crate matches a `&str`. The supported subset is the portable token set JSON Schema 2020-12 core §6.4 recommends, plus the class escapes:
- literal characters;
- `.`, `^`, `$`;
- groups `( )`, `(?: )`, `(?<name> )`;
- alternation `|`;
- quantifiers `* + ? {n} {n,} {n,m}`, each optionally lazy;
- character classes `[…]` and `[^…]`, with ranges;
- the escapes `\d \D \w \W \s \S \b \B \t \n \v \f \r`, `\xHH`, `\uHHHH`, `\u{H…}`, and `\` followed by a syntax character (`^ $ \ . * + ? ( ) [ ] { } | /`), plus `\-` inside a class.

## Where the dialects disagree

Exact ECMA would accept these values or patterns, which zerx rejects today. Under this plan every example below is rejected, either by matching or by refusal. Each example was checked against Node 22 with the `u` flag (valid, matching) and against the current zerx behaviour (rejected).

| # | Construct | Exact-ECMA widening | Example | This plan |
|---|---|---|---|---|
| W1 | `\D` | non-ASCII digits match | `^\D$` vs `"١"` | no match |
| W2 | `\W` | non-ASCII word characters match | `^\W$` vs `"é"` | no match |
| W3 | `\s` / `\S` | `\s` gains U+FEFF; `\S` gains U+0085 | `^\s$` vs `"\u{FEFF}"`; `^\S$` vs `"\u{85}"` | no match |
| W4 | `\b` | ASCII boundary between an ASCII word character and a non-ASCII letter | `^a\bé$` vs `"aé"` | no match |
| W5 | `\B` | non-boundary where Unicode sees a boundary | `^é\B!$` vs `"é!"` | no match |
| W6 | negated classes with `\d`, `\w`, `\s`, `\S` | complement of a narrowed member; `[^\S]` equals `\s` | `^[^\w]$` vs `"é"`; `^[^\S]$` vs `"\u{FEFF}"` | no match |
| W6b | positive classes with `\D`, `\W`, `\s`, `\S` | member widens as in W1–W3 | `^[\D]$` vs `"١"`; `^[\s]$` vs `"\u{FEFF}"` | no match |
| W7 | `&&`, `~~`, unescaped `[` in a class | Rust read set operations / a nested class | `^[a&&b]$` vs `"&"`; `^[a~~b]$` vs `"~"` | `Unsupported` |
| W8 | `[^]`, `\cX`, `\0`, `[\b]` | invalid in the `regex` crate today (rejects everything) | `^[^]$` vs `"x"` | `Unsupported` |

`[[:alpha:]]` and `[a--b]` are SyntaxErrors in ECMA `u` mode, so they are `Malformed`.

The plan's narrowings against today are deliberate:
- `\d`, `\w`, `.`, `\s` (U+0085 drops out) and `\S` (U+FEFF drops out);
- `\b`/`\B` at positions where the Unicode and ASCII readings disagree;
- the refusal of Rust-only syntax, W7 and W8.

Its narrowings against ECMA are equally deliberate: `\D`, `\W`, `\S` and `\s` (U+FEFF), `\b`/`\B`, W7 and W8.

## Breaking Changes

Yes. Builder patterns behave differently:
- `\d`, `\w` and `.` take their ECMA meaning. `\d` and `\w` no longer match non-ASCII characters, and `.` no longer matches `\r`, U+2028 or U+2029.
- `\s` no longer matches U+0085, and `\S` no longer matches U+FEFF.
- `\b` and `\B` hold only where both the Unicode and the ASCII reading agree.
- `\D` and `\W` keep their current Unicode meaning.
- Rust-only syntax is no longer accepted: `(?i)`, `\p{…}`, `\A`, `\z`, `(?P<…>)`, POSIX classes and class set operations. Neither are the W7/W8 constructs. Such a schema validates every string as `PATTERN_INVALID`, as a non-compiling pattern does today.
- On import, a source that is invalid ECMA raises `IMPORT_MALFORMED`, and a valid but untranslatable one raises `IMPORT_UNSUPPORTED`. Lookaround therefore moves from `IMPORT_MALFORMED` to `IMPORT_UNSUPPORTED`.

**Version:** a major release, **3.0.0**. The Planner bumps `Cargo.toml` and `Cargo.lock` at finalization, as DevLead instructed. It is published to GitHub only.

**What callers do:**
- A consumer relying on Unicode `\d`/`\w` writes an explicit class, e.g. `[0-9\u0660-\u0669]`.
- Inline flags are rewritten out of the pattern.

There is no migration code (Hard Rule 2). DevLead's read-only scan of the local consumers found no pattern that uses the differing constructs.

## Reference Patterns

- `src/types.rs` — `Pattern`, `PatternState`, `Pattern::new`: the compiled-or-invalid storage consulted by `validate`.
- `src/json_schema.rs` — the other three `regex::Regex::new` call sites for pattern sources:
  - `import_string_validators`, which refuses a non-compiling `pattern`;
  - `validate_static_value`;
  - the static `pattern` evaluation in `static_keyword_holds`.
- `src/delta.rs`, `parse_pointer` — the crate's hand-written parser style.

## Dependencies

None. The translator targets the `regex` crate already in use; the lockfile pins 1.12.3.

## Assumptions & Risks

- Probed on regex 1.12.3:
  - `\b`, `\B`, `(?-u:\b)` and `(?-u:\B)` compile in a Unicode `&str` regex, and an adjacent pair asserts both.
  - Nested classes (`[[^0-9]a]`), `(?<name>…)` with Unicode names, and `\x{…}` inside classes all compile.
  - `[^\x{0}-\x{10FFFF}]` compiles and matches nothing.
- **`\s` set:** the ECMA WhiteSpace + LineTerminator set is fixed by the spec and written out explicitly: U+0009–U+000D, U+0020, U+00A0, U+1680, U+2000–U+200A, U+2028, U+2029, U+202F, U+205F, U+3000, U+FEFF. Rust `White_Space` is that set without U+FEFF and with U+0085. The intersection is therefore the ECMA set without U+FEFF.
- **Over-strict translator:** refusal is the intended fail-closed outcome. The regression row keeps ordinary patterns working.
- **Out of scope:** `\p{…}`/`\P{…}`, case-insensitive matching, lookaround and backreferences. These are refused, not emulated.
- **Not touched:** the `url` structural-check regex, which is internal and not a pattern source.

## Steps

1. **Translator module.** Add the crate-private module `src/ecma_regex.rs` (`mod ecma_regex;` in `src/lib.rs`):
   - `pub(crate) enum EcmaError { Malformed(String), Unsupported(String) }`.
   - `pub(crate) fn translate(src: &str) -> Result<String, EcmaError>`: a recursive-descent parser over `src.chars()` that emits a `regex` crate expression. It counts the nesting of groups and classes, and beyond 250 levels (the `regex` crate's own nest limit, which guards today's code) returns `Unsupported`, so a deeply nested source cannot overflow the stack.
   - `pub(crate) fn compile(src: &str) -> Result<regex::Regex, EcmaError>`: `translate`, then `regex::Regex::new`. A build error on the translated output (for example the size limit) is `Unsupported`.

   **Emission rules.** The parser tracks each escape's position: outside a class, in a positive class, or in a negated class.

   Literals, anchors and `.`:
   - A literal outside a class: alphanumerics and non-ASCII characters are written as themselves, everything else as `\x{HEX}`.
   - A literal inside a class: alphanumerics and non-ASCII characters as themselves, everything else as `\x{HEX}`, including `&`, `-` and `~`.
   - `.` → `[^\n\r\x{2028}\x{2029}]`. `^` → `^`. `$` → `$` (no multiline flag, so the meaning is the same in both dialects).

   Class escapes outside a class or in a positive class (intersection form):
   - `\d` → `[0-9]`; `\w` → `[A-Za-z0-9_]`.
   - `\D` → `\D` and `\W` → `\W`: the Unicode forms, which are subsets of ECMA's.
   - `\s` → `[` ECMA set without U+FEFF `]`.
   - `\S` → `[^` Rust `White_Space` plus U+FEFF `]`.
   - Inside a positive class each is written as a nested class, e.g. `[\Da]` → `[\Da]`, `[\da]` → `[[0-9]a]`.

   Class escapes inside a negated class (union form, so the complement is the intersection):
   - `\d` → `\d` (Unicode); `\w` → `\w` (Unicode).
   - `\s` → `[` Rust `White_Space` plus U+FEFF `]`.
   - `\D` → `[^0-9]`; `\W` → `[^A-Za-z0-9_]`.
   - `\S` → `[^` ECMA set without U+FEFF `]`.

   Assertions:
   - `\b` → `\b(?-u:\b)`; `\B` → `\B(?-u:\B)`.
   - A quantifier directly after an assertion (`\b*`, `\b{2}`, `^*`) is invalid ECMA ("Nothing to repeat") and is `Malformed`. The translation never makes it valid by wrapping.
   - A source-level group keeps its scope: `(?:\b)*` → `(?:\b(?-u:\b))*`.

   Inside a class, `\b` (backspace in ECMA) is a W8 construct → `Unsupported`, and `\B` → `Malformed`.

   Character escapes:
   - `\t \n \v \f \r` → `\x{9} \x{A} \x{B} \x{C} \x{D}`.
   - `\xHH`, `\uHHHH` and `\u{H…}` (at most `10FFFF`) → `\x{H}`.
   - A surrogate pair `\uHHHH\uHHHH` (high U+D800–U+DBFF followed by low U+DC00–U+DFFF) → `\x{…}` of the combined code point (`\uD800\uDC00` → `\x{10000}`).
   - A lone or reversed surrogate, and a braced `\u{D800}`–`\u{DFFF}`, → `Unsupported`: valid in `u` mode, but it matches a lone surrogate, which a Rust `&str` cannot hold.

   Groups:
   - `(` → `(`; `(?:` → `(?:`; `(?<name>` → `(?<name>`.
   - Names follow ECMA's GroupName grammar and pass through unchanged; the crate accepts Unicode names. A name outside that grammar (empty, digit-leading) → `Malformed`.
   - A duplicate name in the same alternative → `Malformed`. In distinct alternatives (ES2025) → `Unsupported`, because the crate rejects duplicate names.

   Quantifiers:
   - Copied with their lazy `?`. `{n,m}` with `n > m` → `Malformed`.
   - Nothing to repeat, a `{` that does not start a valid quantifier, and a lone `}` or `]` → `Malformed`.

   Classes:
   - A range whose ends are class escapes, or whose start is above its end → `Malformed`.
   - `[]` → `[^\x{0}-\x{10FFFF}]` (matches nothing, as today's invalid pattern rejects everything).

   Explanatory comments go at the `\b`/`\B` emission (the pair is the intersection of the Unicode and the ASCII boundary) and at the `\s`/`\S` emission (a fixed ECMA list, intersected with or united with `White_Space` by position).

   **Refusals.**
   - `Unsupported` (valid ECMA that cannot be held):
     - `(?=`, `(?!`, `(?<=`, `(?<!`;
     - a backreference `\1`–`\7` (or more digits) naming an existing group, and `\k<name>` naming an existing group;
     - `\p{…}`/`\P{…}`;
     - the ES2025 modifier groups `(?i:…)`/`(?-i:…)`;
     - the W7 constructs: `&&`, `~~` or an unescaped `[` inside a class;
     - the W8 constructs: `[^]`, `\cX`, `\0`, `[\b]`;
     - nesting deeper than 250.
   - `Malformed` (invalid ECMA):
     - any other `(?` form (for example `(?i)`, `(?P<…>`);
     - `\8` and `\9`, a numbered backreference without a matching group, `\k<…>` without a matching group;
     - an identity escape of a non-syntax character (`\a`, `\e`, `\y`, …);
     - `\-` outside a class;
     - an unbalanced `(` or `)`, and an unterminated class or escape.

2. **Pattern validator.** In `src/types.rs`, `Pattern::new` calls `crate::ecma_regex::compile(&source)` instead of `regex::Regex::new`. Either `EcmaError` becomes `PatternState::Invalid(message)`, so a refused construct validates as `PATTERN_INVALID`; the builder stays infallible, as today. `Pattern::json_schema` still exports the original source.

3. **Import.**
   - `import_string_validators`: replace the `regex::Regex::new(pat).is_err()` check with `ecma_regex::compile(pat)`. `Malformed` → `IMPORT_MALFORMED`; `Unsupported` → `IMPORT_UNSUPPORTED`; both at the node path.
   - `validate_static_value`: the `"pattern"` check accepts a source exactly when `compile` succeeds, and returns `IMPORT_UNSUPPORTED` for `Unsupported` (a small change to that function's error construction for this key).
   - The static `pattern` evaluation compiles with `ecma_regex::compile`, so static and runtime semantics agree.

4. **Tests.** Add `#[cfg(test)] mod tests` in `src/ecma_regex.rs`, import cases in the `src/json_schema.rs` tests, and a validator case in `src/types.rs`. The required cases are listed under Verification.

## Documentation

- `docs/architecture/types.md`:
  - New constraint under *String validators*: pattern sources MUST be read as ECMA-262 with the `u` flag, restricted to the translated subset, and a construct whose meaning differs between ECMA and the `regex` crate MUST match only what both accept. A construct outside the subset MUST make the pattern invalid (`PATTERN_INVALID`). **Basis:** the JSON Schema boundary contract (`pattern` is ECMA-262) and `D-import-fail-closed`, because the runtime must not be wider than the exported `pattern`; plus the Product Owner's explicit choice of intersection semantics.
  - Changed *Architecturally Significant Dependencies* `regex` bullet: patterns are compiled from the ECMA translation. **Basis:** a reference update following the constraint above.
- `docs/architecture/json-schema.md`:
  - Changed: static evaluation compiles `pattern` through the same translation, not the raw `regex` crate. **Basis:** `D-import-fail-closed` (static and runtime semantics must agree).
  - New: an imported `pattern` that is invalid ECMA MUST raise `IMPORT_MALFORMED`, and one that is valid but untranslatable MUST raise `IMPORT_UNSUPPORTED`. **Basis:** `D-import-fail-closed` and the existing grammar rule.
  - Changed: the two standing clauses that make a non-compiling `pattern` raise `IMPORT_MALFORMED` are reworded to this split. They are the import-coverage bullet for string/`uri`/`url` nodes and the static-evaluation bullet under *`enum` / `const` import*. **Basis:** the same.
  - New: a named narrowing. An imported `pattern` MAY accept less than ECMA-262 does, where the dialects disagree (`\D`, `\W`, `\s`, `\S`, `\b`, `\B`). This is a deliberate narrowing under `D-import-fail-closed`. **Basis:** `D-import-fail-closed` permits a narrowing only where this file names it; the Product Owner chose it.
- `docs/tests.md`: a new inventory row `ecma_regex`, with unit boundary, the `types` concern, and the perspectives "ECMA-262 dialect conformance, intersection semantics, refusal classification, nesting bound".
- `docs/bug.kanban.md`: the card "`pattern` uses Rust regex semantics, not ECMA-262" moves to `Done`.
- No register entry is proposed or superseded.

## Verification

Required gate: `scripts/verify all` (fmt, lint, test, test core, api), green on the final state.

Plan-specific checks, all as unit tests in the gate:

- **Translation semantics.** Each case is a `(pattern, value, expected)` triple checked through `compile(...).is_match`:
  - `^\d+$`: `"123"` matches; `"١٢٣"` does not.
  - `^\w+$`: `"a_1"` matches; `"é"` does not.
  - `^.$`: `"a"` matches; `"\r"` and `"\u{2028}"` do not.
  - `^\s$`: `" "` matches; `"\u{FEFF}"` and `"\u{85}"` do not.
  - `^\S$`: `"a"` matches; `"\u{FEFF}"` and `"\u{85}"` do not.
  - `^\D$`: `"a"` matches; `"١"` does not.
  - `^\W$`: `"!"` matches; `"é"` does not.
  - `^a\bé$`: does not match `"aé"`. `^é\B!$`: does not match `"é!"`. `^\bab\b$`: matches `"ab"`.
  - `^[^\w]$`: `"!"` matches; `"é"` does not.
  - `^[^\S]$`: `" "` matches; `"\u{FEFF}"` and `"\u{85}"` do not.
  - `^[^\D]$`: `"1"` matches; `"١"` does not.
  - `^[\D]$`: `"a"` matches; `"١"` does not.
  - `^[\s]$`: `"\u{FEFF}"` does not match.
  - `^[\d-]+$`: `"1-2"` matches. `^[]$`: `"x"` does not.
  - `^\u{1F600}$`: `"😀"` matches. `^\uD800\uDC00$`: `"\u{10000}"` matches.
  - `^(?<y>\d{4})-\d{2}$`: `"2024-01"` matches. `^(?<ünï>\w)$`: `"a"` matches.
  - `^a{2,3}?$`: `"aa"` matches. `^(?:\b)*a$`: `"a"` matches.
- **Refusals.**
  - `translate` returns `Unsupported` for:
    - `(?=a)`, `(?<!a)b`, `(a)\1`, `(?<n>a)\k<n>`, `\p{L}`;
    - `\uD800`, `\uDC00\uD800`, `\u{D800}`;
    - `(?<n>a)|(?<n>b)`, `(?i:a)`;
    - `[a&&b]`, `[a~~b]`, `[[]`, `[^]`, `\cA`, `\0`, `[\b]`;
    - a source of 300 nested groups. A 100 000-deep source returns `Unsupported` without overflowing the stack.
  - It returns `Malformed` for:
    - `\b*`, `\b{2}`;
    - `\8`, `\1` (no group), `\k<n>` (no group);
    - `(?<1a>x)`, `(?<n>a)(?<n>b)`, `(?P<n>a)`, `(?i)a`;
    - `\A`, `a{,2}`, `\e`, `\-` (outside a class), `[\B]`, `[z-a]`, `[a--b]`, `[[:alpha:]]`;
    - `(a`, `a)`, `{`, `}`, `]`.
  - Every classification was checked against Node 22 with the `u` flag: `Malformed` means a SyntaxError there, `Unsupported` means valid there. The ES2025 forms are judged by the current edition instead.
- **Validator.** `string().regex(r"^\d+$")` rejects `"١٢٣"` with `PATTERN_MISMATCH`. `string().regex("(?i)a")` yields `PATTERN_INVALID`.
- **Import.**
  - `{"type":"string","pattern":"^\\d+$"}` rejects `"١٢٣"`.
  - `{"type":"string","pattern":"(?=a)a"}` raises `IMPORT_UNSUPPORTED`; `{"type":"string","pattern":"\\e"}` raises `IMPORT_MALFORMED`.
  - `{"enum":["١٢٣"],"pattern":"^\\d+$"}` raises `IMPORT_UNSUPPORTED` (statically violated); `{"enum":["123"],"pattern":"^\\d+$"}` imports.
- **Regression.** The existing pattern tests in `src/types.rs` and `src/json_schema.rs` stay green unchanged, including the uri/url marker pattern round-trip and the static `enum` + `pattern` cases. Any change needed there is reported as a Rust-only construct.
