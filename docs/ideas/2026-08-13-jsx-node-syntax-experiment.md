# Experiment: first-class tag literals + builtin `Node`

**Status:** **landed on master** (2026-08-13) — compiler + runtime surface
complete; Crema pages rewritten onto tags.  
**Date:** 2026-08-13  
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
| Value | Builtin `Node` (`Ty::Node` → `ThoxaHtml*` in `thoxa_rt`) |
| HTML tag | `<div class="x">…</div>` → `__jsx_el(tag, names, vals, kids)` |
| Component | PascalCase `<Card title="Hi">…</Card>` → `Card(title: "Hi").view(children: […])` (eager) |
| Fragment | `<>…</>` → fragment `Node` (empty `<>` included) |
| `{e}` | `String` → escaped text; `Int64`/`Bool` → stringify then text; `Node` → identity; `Array<Node>` → splice |
| Raw text | Source slice (lexer `Junk` so `'` / `·` survive); whitespace-only gaps dropped |
| Void tags | `br` `hr` `img` `input` `meta` `link` — no close in `.html()` |
| Boolean attrs | `disabled` → `disabled="disabled"` |
| Hover | `Node` is an LSP builtin |

No user-facing `el` / `child` / `html` import. Desugar names are `__jsx_*`
(do not steal `el` / `child`).

**Components are types**, not subtypes of `Node`. They **produce** `Node` via
`view`. No inheritance.

## Non-goals (this landing)

- `El \| Text \| Frag` sum in the type system (runtime is a tagged C struct)  
- Spreads, `className`, hydration / client VDOM  
- Handbook / D-id / freeze  

## Dogfood

[`examples/http`](../../examples/http/) pages are tag trees: `type Page` in
`app/Layout.thx`, views in `app/Pages.thx`, `n.html()` + `<!DOCTYPE html>` in
`htmlResponse`.

## Gaps

- No attr spreads / boolean-without-value beyond `name="name"`  
- Hyphenated attrs parse; `data-*` ok  
- `.html()` method has no dedicated hover (type `Node` does)  
- Tree-sitter island colors tags; fragments/`<>` highlighting is best-effort  
- `{e}` attrs are still String (HTML attrs); Int64/Bool stringify is children only  
- Bare `<tag>` after another expression is `<` (comparison). Bind with `let`,
  or make the tag the whole block value.

Pickup: root `CURRENT.md` (focus) · this file · capability `jsx-node`.
