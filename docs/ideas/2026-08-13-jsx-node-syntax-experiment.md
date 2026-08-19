# Experiment: first-class tag literals + builtin `Node`

**Status:** **landed on master** (2026-08-13; Node sum + `Attr` + spreads
2026-08-15) — compiler + runtime surface complete; Crema pages rewritten
onto tags.  
**Date:** 2026-08-13 (updated 2026-08-15)  
**Not** a freeze, **not** handbook / capabilities “shipped.” No D-id yet.  
**Demo:** [`scratch/jsx.thx`](../../scratch/jsx.thx)  
**Gate:** conformance `jsx`  
**Earlier parking lot:** [webapp SSR / components](2026-07-21-webapp-ssr-components.md)

---

## Goal

HTML in Thoxa should look like markup, not hostage-note strings. Tags are
**expressions** that build a builtin **`Node`** tree; `n.html()` is escaped
markup. No import — same ambient shape as `String` / `Array`.

```thx
type Card(title: String) {
    export fn view(children: Array<Node>) -> Node {
        <section class="card">
            <h1>{title}</h1>
            {children}
        </section>
    }
}

let n = <Card title="Hi"><p>Hello, {name}</p></Card>
logging.info(n.html())
```

## As-built (v0)

| Piece | Behavior |
|-------|----------|
| Value | Builtin `Node` (`Ty::Node` → `ThoxaHtml*` in `thoxa_rt`). Matchable as `Element \| Text \| Fragment` (same tree, not a second sum). |
| HTML tag | `<div class="x">…</div>` → `__jsx_el(tag, attrs, kids)` with `Array<Attr>` |
| Component | PascalCase `<Card title="Hi">…</Card>` → `Card(title: "Hi").view(children: […])` (eager) |
| Fragment | `<>…</>` → fragment `Node` (empty `<>` included) |
| Ctors | Ambient `Element(tag, attrs, kids)`, `Text(s)`, `Fragment(kids)`, `Attr(name, value)` — no import |
| `{e}` | Children: `String` → escaped text; `Int64`/`Bool` → stringify then text; `Node` → identity; `Array<Node>` → splice. Attrs: `String` / `Int64` / `Bool` (stringify; `true`/`false`) |
| Spreads | HTML `{...xs}` is `Array<Attr>` (later attrs win by append order). Component `{...p}` copies ctor fields of a product `p` (later named props win) |
| `<` vs tag | `<name` / `<>` (no space after `<`) is JSX, including after another expr. Comparison is `a < b` (space after `<`). Glued `a<b` / `n<3` is an error. |
| Raw text | Source slice (lexer `Junk` so `'` / `·` survive); whitespace-only gaps dropped. Bare `#` is still a line comment. **`\#` is a literal `#`** (`Ticket \#{n}` → `Ticket #3`). |
| Void tags | `br` `hr` `img` `input` `meta` `link` — no close in `.html()` |
| Boolean attrs | `disabled` → `disabled="disabled"` |
| Hover | `Node`, `Element`, `Text`, `Fragment`, `Attr` are LSP builtins |

No user-facing `el` / `child` / `html` import. Desugar names are `__jsx_*`
(do not steal `el` / `child`).

**Components are types**, not subtypes of `Node`. They **produce** `Node` via
`view`. No inheritance.

## Non-goals (this landing)

- `className`, hydration / client VDOM  
- Handbook / D-id / freeze  

## Dogfood

[`examples/http`](../../examples/http/) pages are tag trees: `type Page` in
`app/ui/page.thx` (`title` + `request`), views under `app/pages/`
(`DrinkCard` in `app/ui/drink-card.thx` — **D135**),
`n.html()` + `<!DOCTYPE html>` in `htmlResponse`. `Node` and user types are
legal constructor-param fields.

## Gaps

- Boolean-without-value beyond `name="name"` (bare `disabled` still serializes `disabled="disabled"`)  
- Hyphenated attrs parse; `data-*` ok  
- `.html()` method has no dedicated hover (type `Node` does)  
- Tree-sitter island colors tags; fragments/`<>` highlighting is best-effort  
- Generic `toString` / `repr` / interpolation of `Node` **parked** — see
  [`2026-08-15-tostring-repr-and-traits.md`](2026-08-15-tostring-repr-and-traits.md)
  (not a traits forcing function; do not delete `.html()`)

Pickup: root `CURRENT.md` (focus) · this file · capability `jsx-node`.
