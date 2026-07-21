# Idea: postfix `having` / `else` for `May` (and maybe `Trial`)

**Status:** sleeping — do not implement yet.  
**Sparked by:** nested `match` on `Bytes.find` → `May<Int64>` in `parseRequest` (`tests/conformance/httplib/Http.thx`).  
**Related:** D85 (`May` / `Has` / `Not` / `?`), D63 (early `return`), D69 (`parseRequest`).

---

## Motivation

Today a successful `find` forces two indent levels before the body:

```thoxa
match raw.find(sep) {
    Has(sepAt) {
        ## body — indent 2
    }
    Not {
        Fail("incomplete request")
    }
}
```

Failure arms are often one-liners (`Fail(…)`); the interesting work lives under `Has`. A postfix form can put the happy path first and cut one indent level, while staying pure sugar over `match`.

This is **not** aiming to flatten sequential finds into a zero-nest pipeline (let-else / guard with parent-scope binds). Nested finds may still nest; each nest is one level cheaper and reads better.

---

## Locked preferences (from discussion)

| Choice | Stance |
|--------|--------|
| Keywords | `having` + existing **`else`** (not `without` / `otherwise`) |
| Bind scope | `having (y)` introduces `y` **only inside the having block** — never the parent |
| No guard/let-else dual (for now) | Reject “bind then continue at column 0” as part of this idea |
| Indent story | Win = 1 level to body vs match’s 2; not “fully flat parseRequest” |
| `?` | Keep as silent same-family unwrap; do **not** overload `?` for May→Trial + custom message (still D85) |

---

## Proposed surface

Scrutinee first (postfix control, same family as `?`):

```thoxa
## 1. both arms — primary form
foo.find(x) having (y) {
    ## y in scope here only
} else {
    ## Not / absence
}

## 2. failure only (unwrap-or-else)
foo.find(x) else {
    …
}

## 3. success only
foo.find(x) having {
    ## payload unused
}
foo.find(x) having (y) {
    ## y local to this block
}
```

`else` alone is optional in the design; if kept, success is the **unwrapped payload** (so a surrounding `let` names it — not `having`):

```thoxa
let sepAt = raw.find(sep) else {
    return Fail("incomplete request")
}
```

That does not violate “having never leaks”: the name comes from `let`.

---

## Semantics (sketch)

**May-only first.** Whole form is an **expression**; arm types join like `match`. Desugar target is ordinary `match` on `Has` / `Not`.

| Form | `Has(v)` | `Not` |
|------|----------|--------|
| `e having (y) { A } else { B }` | `A` with `y = v` | `B` |
| `e having { A } else { B }` | `A` (discard `v`) | `B` |
| `e else { B }` | value is `v` (type `T`) | `B` |
| `e having (y) { A }` | `A` | see completeness |
| `e having { A }` | `A` | see completeness |

**Completeness when `else` is omitted**

- If the form must produce a non-`Void` value → require `else` (or use unwrap-`else` only).
- If used as a statement / `Void`-compatible arm → omitted `else` means `Not { Void }` (side-effect-if-present). Useful for query-strip style:

```thoxa
target.find(Bytes.fromString("?")) having (q) {
    target = target.slice(0, q)
}
```

**Invert** (presence is the error) stays possible without extra syntax: Fail in `having`, real work in `else` — rare enough.

**Trial** — later, maybe `else (err) { … }` for Fail payload. Do not block a May-only ship on Trial.

---

## Example shape (`parseRequest`)

Illustrative only — not a proposed rewrite of the library yet:

```thoxa
export fn parseRequest(raw!: Bytes) -> Trial<Request, String> {
    let sep = Bytes.fromString("\r\n\r\n")
    raw.find(sep) having (sepAt) {
        let headerBlock = raw.slice(0, sepAt)
        let crlf = Bytes.fromString("\r\n")
        let reqLine = headerBlock.find(crlf) having (i) {
            headerBlock.slice(0, i)
        } else {
            headerBlock
        }
        let sp = Bytes.fromString(" ")
        reqLine.find(sp) having (sp1) {
            reqLine.find(sp, sp1 + 1) having (sp2) {
                reqLine.find(sp, sp2 + 1) having (_) {
                    Fail("malformed request line")
                } else {
                    parseRequestBody(reqLine, sp1, sp2, headerBlock, crlf)
                }
            } else {
                Fail("malformed request line")
            }
        } else {
            Fail("malformed request line")
        }
    } else {
        Fail("incomplete request")
    }
}
```

Still a pyramid for sequential must-finds; each step is one indent cheaper than `match` + `Has`.

---

## Alternatives considered (not chosen for this idea)

| Alternative | Why set aside here |
|-------------|--------------------|
| `let Has(x) = e else { return … }` (Rust let-else) | Parent-scope bind; different feature |
| Swift-style `guard` | Same; extra keyword; exit-required else |
| `need(m, msg)?` helper | Works today; library, not language |
| Fatter `?` / May→Trial with message | Fights D85 family purity |
| `without` / `otherwise` | Prefer reuse of `else` |

**Works today without new syntax** (for when someone needs flat code before this ships):

```thoxa
let sepAt = match raw.find(sep) {
    Not { return Fail("incomplete request") }
    Has(i) { i }
}
```

---

## Open questions

1. Ship **all three arities** or only the full `having (y) { } else { }` first?
2. Keep **unwrap-`else`** (`e else { B }` → `T` or `B`)?
3. Soft-reserve `having` only in postfix position, or hard keyword?
4. Bind syntax: `having (y)` vs bare `having y`? (Parens favor `(_)` discard.)
5. Grammar / precedence: same band as `?`? How does `else` attach next to a nearby `if`?
6. Trial + error bind in a follow-on, or never part of this form?
7. Expression-only, or also a statement-mode special case for Void?

---

## Non-goals (for a first version)

- Parent-scope introduction of the success binding
- Replacing `match` or `?`
- Inferred fallible returns / May↔Trial `?` mixing
- Implementation, conformance, or a decision id until this is deliberately promoted

---

## Promotion path (when ready)

1. Re-read this file after sleeping; tighten arities and completeness rules.
2. Optional: short desugar table → MIR/AST only (no new runtime).
3. Lock a D-id in `docs/decisions.md`; small conformance (e.g. rewrite one `find` site or a dedicated `having_else` test).
4. Move or link from here into `docs/specs/` if the write-up grows into a real plan.
