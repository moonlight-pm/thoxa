# Idea: Server-rendered web components (HTML trees + optional tag sugar)

**Status:** sleeping — speculation only; do not implement yet.  
**Sparked by:** `scratch/webapp.thx` musing on component/`view` + JSX-like markup for web apps.  
**Related:** D67–D70 (HTTP server / Request / Response / Router), D53–D55 (Array), D84 (bare `type`), SPEC §4.1 defaults **[M3]**, SPEC §6 sum types **[M2]**, D2 (effects deferred).

---

## Motivation

HTTP ladder already serves **static files** (`Router` + `readFile` → `Response`). The next product-shaped question is **dynamic HTML**: compose pages from reusable pieces and emit bytes that plug into the existing accept → handle → writeResponse path.

The scratch sketch aims at a React-ish surface — components, children, markup that *looks* like HTML — without yet committing to a browser DOM or interactive SPA runtime.

```thoxa
## scratch/webapp.thx (as written — not valid Thoxa today)
type Node

type Component is Node {
    fn view(children: Node | Array<Node> = [])
}

type Root is Component {
    fn view(children) {
        <html>
            <head>
            </head>
            <body>
                {children}
            </body>
        </html>
    }
}

type Main is Component {
    fn view() {
        <main>Main</main>
    }
}

fn compose() {
    <Root>
        <Main />
    </Root>
}
```

---

## What the sketch is really proposing

Three layers tangled together:

| Layer | Surface in the sketch | Role |
|-------|----------------------|------|
| **Tree model** | `Node`, `children` | Values that *are* the page (or a virtual tree) |
| **Subtyping** | `type A is B` | “Anything component-shaped can nest” without a big match |
| **View sugar** | `<html>…`, `<Main />`, `{children}` | HTML + component trees that look like markup |

The design question is not “should Thoxa do web?” (static HTTP already does). It is **how much of the UI tree becomes language** vs **library + a little sugar**, and whether the first product is **SSR only**.

---

## Fit with current / oracle Thoxa

### Already in the story (SPEC; not all shipped native)

| Piece | Notes |
|-------|--------|
| Default params (`= []`) | SPEC §4.1 **[M3]** — natural for `children` / props |
| Union types (`Node \| Array<Node>`) | SPEC §6 **[M2]** style; native has monomorphic `May`/`Trial` only |
| Bare `type Node` | Legal today (D84) as a placeholder; better later as a **union alias** |
| User types + methods | Shipped; `fn view() -> Node` can live on types without inheritance |
| HTTP `Request`/`Response`/`Router` | Shipped (D69/D70); natural sink for `render(node) -> Bytes` |

### Not in the language

| Piece | Notes |
|-------|--------|
| `type A is B` | No inheritance / interfaces; oracle aggregates are flat products; polymorphism is **unions + match** |
| Tag / JSX literals | No SPEC surface; heavy lexer/parser + tree-sitter/Zed cost |
| Abstract methods | `Component.view` with no body has no home today |

### Tension with the shipped HTTP stack

Product path today:

`accept → parseRequest → Router.handle → Response → writeResponse → send`

A component `view()` fits as **another way to build a response body**, not a second runtime. No browser DOM is required for useful **server-rendered HTML**. Interactive SPA / hydration is a different product (effects, event loop, possibly WASM/JS).

---

## Locked preferences (from discussion)

| Choice | Stance |
|--------|--------|
| First product | **SSR only** — `Node` → escaped HTML `Bytes`/`String` → `ok(…, contentType: "text/html")` |
| Not first | Interactive UI, VDOM reconcilers, client hydration |
| Polymorphism | **Sum `Node`**, not `is Component` / inheritance lattice |
| Children API | Prefer **`Array<Node>` only**; tag sugar (if any) normalizes one-or-many → array |
| Components | Functions or types with `fn view(…) -> Node` by **convention**, not abstract base types |
| Tag sugar | Optional later; **must desugar** to plain constructors / free fns |
| XSS | Default **escape** text; explicit `rawHtml` (or equivalent) escape hatch |
| Server model | Reuse Net + Http + Router; no new accept-loop shape for “webapps” |

---

## Design forks

### 1. Subtyping: inheritance vs sum vs protocol

**`is Component` reads like OO inheritance** and fights the oracle “constructors + union alias” culture.

**Preferred (first cut):** closed sum, oracle-shaped:

```thoxa
type Element(tag: String, attrs: Array<Attr>, kids: Array<Node>)
type Text(s: String)
type Fragment(kids: Array<Node>)
type Node = Element | Text | Fragment   ## needs general unions [M2]
```

Until general unions land, a practical interim is a single product type plus tag discrimination, or monomorphic helpers only — but the **target shape** is the sum.

**Avoid for this idea:** inventing `is` / interfaces solely to make HTML nest.

**Components** are not subtypes of `Node`; they **produce** `Node`:

```thoxa
type Root {
    export fn view(children: Array<Node>) -> Node { … }
}
fn mainPage() -> Node { … }
```

### 2. Tags: HTML elements vs components

If tag sugar lands, reuse Thoxa’s casing rule:

| Form | Meaning |
|------|---------|
| lowercase / known HTML name | `Element(tag: "html", …)` |
| PascalCase | Component type / `view` (or dedicated builder) |

Open attribute / grammar questions (do not invent casually):

- Attributes as named args, map, or `Array<Attr>`?
- Void elements: `<br />` vs `<br>`?
- `data-*` / `aria-*` / namespaced names?
- Text: bare string children vs only `Text(...)`?

### 3. Props vs children

Classic split to lock before sugar:

```thoxa
<Card title="Hi">   ## props
  <Main />          ## children
</Card>
```

Desugar candidates (pick one evaluation model):

- `Card(title: "Hi").view(children: [Main().view()])`
- `Card.view(title: "Hi", children: […])`
- Free `card(title: "Hi", children: […]) -> Node`

Also: tags as **eager `Node` values** vs **lazy** until top-level `render` (lazy is nicer for conditionals; eager is simpler for C emit).

### 4. `{…}` inside markup vs string interpolation

Markup embeds are **not** the same as `"…"`. Inside tags, `{e}` should mean “splice as child node(s)”, with a small allowed set (e.g. `Node`, `Array<Node>`, `String` → text). Do not treat arbitrary `Response`-producing expressions as children.

### 5. `Node | Array<Node>`

React-style one-or-many unions are convenient at call sites and painful in types. Prefer:

```thoxa
fn view(children: Array<Node> = [])
```

and let sugar always build arrays (single child → singleton).

---

## Syntax tiers (how much sugar is worth it)

### Tier 0 — library only (no new syntax)

```thoxa
fn el(tag!: String, attrs!: Array<Attr>, kids!: Array<Node>) -> Node { … }
fn text(s!: String) -> Node { … }

fn root(children: Array<Node>) -> Node {
    el("html", [], [
        el("head", [], []),
        el("body", [], children),
    ])
}

fn compose() -> Node {
    root([el("main", [], [text("Main")])])
}
```

Validates tree + HTML escape + `Response` wire-up on **today’s** language (Array + user types). Preferred first rung if this idea ever wakes.

### Tier 1 — builder DSL (still library-shaped)

```thoxa
html([head([]), body([main([text("Main")])])])
```

Zero lexer drama; slightly nicer call sites.

### Tier 2 — tag literals (scratch direction)

Worth it only if HTML is a **first-class product surface**. Cost:

- Lexer must not break `Array<T>`, comparisons, or `>>` generic splits
- Nested tags + `{ if … }` expression grammar inside embeds
- Self-closing, attributes, possible spreads
- Diagnostics + tree-sitter/Zed grammar in `extras/`

**Rule:** sugar desugars to Tier 0 constructors — not a second IR.

### Tier 3 — full component system

Defaults, slots, prop types, visibility of `view`, keyed lists, etc. Only after Tier 0–1 prove SSR is useful on real routes.

---

## Natural integration with Router

Illustrative end state (not a plan):

```thoxa
type AppRouter {
    export fn handle(req: Request) -> Response {
        if req.path == "/" {
            return ok(render(compose()), /* content-type text/html */)
        }
        notFound()
    }
}
```

Dynamic HTML routes sit **beside** static file routes; same Net + Http stack (D67–D70).

---

## Where this sits vs deferred megawork

| Dependency | Need for this idea |
|------------|-------------------|
| General sum types / unions [M2] | Clean `Node = Element \| Text \| …`; interim encodings possible |
| Default params [M3] | Nice for `children = []`; can pass `[]` explicitly until then |
| Effects [D2] | **Not** required for SSR; required for interactive UI |
| Perceus [D8] | Nice for tree RC; simple RC already enough for SSR trees |
| Named `std/` roots [D30–D32] | Package can live as `~/htmllib` / conformance-style dirs first |

---

## Open questions

1. Minimal `Attr` model (name/value strings only vs typed enums for common attrs)?
2. Fragment syntax without sugar (`Fragment(kids)` only)?
3. Should `render` return `String`, `Bytes`, or both?
4. Streaming HTML (chunked body) vs single buffer — deferred with better I/O?
5. CSS / assets: still static Router, or co-located component bundles later?
6. If tag sugar lands, exact conflict rules with comparison `<` and generics?
7. Promote path: idea → small `htmllib` design doc + conformance golden HTML, **before** any parser work?

---

## Non-goals (for this parking-lot idea)

- Browser DOM bindings or “Thoxa runs in the page”
- Full React compatibility or JSX spec compliance
- Inheritance / `is` as a general language feature justified only by components
- Infinite product accept-loop (still separate from D67 bounded conformance)

---

## Suggested wake order (if ever)

1. Tier 0 library + `render` + escape + one golden HTML conformance via existing HTTP types  
2. A few real routes next to static files  
3. Only then consider Tier 1 builders or Tier 2 tag desugaring  
4. Interactive / effects only after D2 stops being deferred  

Do **not** start with `is` + JSX.
