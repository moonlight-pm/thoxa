# Idea: file-based routing (examples/http)

**Status:** **parked** — 2026-08-15. Not a freeze, not product, not the next
slice.  
**Sparked by:** considering Next/SvelteKit-style routes for Little Crema.  
**Related:** Crema charter
[`2026-08-10-examples-http-crema-experiment.md`](2026-08-10-examples-http-crema-experiment.md)
(lists “module-tree auto-routing” as a non-goal — this note is that parked
fork); route clauses
[`2026-08-12-router-clause-syntax-experiment.md`](2026-08-12-router-clause-syntax-experiment.md);
kebab-case modules **D138** (split out and shipped — see below).

---

## What we wanted

Path-from-the-tree routing for `examples/http`: the filesystem *is* the URL
space (`app/pages/menu.thx` → `GET /menu`), not a central `type Routes`
table of string patterns.

That is a compile-time problem. Packages are **import-closed**; unused `.thx`
files are not part of the program. Real file-based routing needs directory
discovery (compiler or a generated barrel), not a runtime walk.

Crema’s live router stays type-body `GET "/…"` / `POST "/…"` on `type Routes`
plus free `handle` with static fallthrough.

---

## What we learned

### 1. PascalCase files collide with URL space

Native (and the oracle) treated a module file as **describing a type**, so
stems were PascalCase (`Http.thx`, `Pages.thx`). The loader enforced it.
URLs are kebab/lowercase. `Menu.thx` is not `/menu`. `$slug.thx` /
`[slug].thx` are not type names.

A “every folder has `Page.thx`” tree would have satisfied PascalCase and
still failed: module id was the **file stem**, so twenty `Page.thx` files
would all mangle as `Page_*`.

That conflict is why module file naming was reopened — and **split out of
this idea**. Product now: a file is a module (a scope), not a type; library
stems are kebab-case; module ids are path-qualified (**D138**). Types stay
PascalCase inside files. JSX `<Page>` still means type `Page`.

Do **not** kebab-case this idea back into a second kind of module
(“libraries cannot have `let`”). One module kind is enough.

### 2. Do not encode a DSL in inodes

Once the tree is a pattern language, the punctuation multiplies: captures,
then layouts, then index, then error. Two concrete capture spellings were
considered:

- `$userId` — one token, legal filename, not kebab-case. User disliked `$`.
- `+userId` — same shape; one marker in the **filename and** the URL
  pattern (`GET "/users/+userId/profile"`), so today’s `:slug` would go
  away. SvelteKit uses `+` for framework files (`+page`), not params — a
  cultural collision, not a technical one.

Mid-path params are a **directory**, never `.thx` in the middle:

```text
# do not:  users/[userid].thx/profile
users/
  +userId.thx                 → GET "/users/+userId"
  +userId/
    profile.thx               → GET "/users/+userId/profile"
```

`+userId.thx` (file) and `+userId/` (dir) can coexist. The name after `+`
is the formal (`+userId`, not `+user-id`). Capture type stays on the
handler (`String` default, `Int64` → 400), same as route clauses.

Layouts want a **reserved word**, not a second sigil: `layout.thx` in a
folder (wrap the subtree), or no magic and keep today’s explicit `<Page>`
import. If `+` means capture, it only means capture.

### 3. Variable segments do not have to be filenames

The filesystem-as-DSL is optional. Alternatives that keep kebab-case files
ordinary:

1. **Route line in the file** (leaned this way if routing returns). Walk
   `pages/`. Static pages omit the line and use the tree. The moment there
   is a hole, write it:

   ```thx
   route GET "/users/+userId/profile"

   export fn get(userId: String, req: Request) -> Response { … }
   ```

   Mid-path params are trivial. Folders stay human organization (or a
   static prefix check). Same `+name` as clauses; never in the inode.

2. **Formals are the holes, tree is only static segments.** Fine for a
   leaf (`get(slug: String)` → `/menu/+slug`). Cannot place
   `/users/+userId/profile` without an insertion rule or writing the path
   anyway.

3. **Two layers.** File-based static pages; parameterized URLs stay in
   `type Routes`. Two drawers. Only a stepping stone.

v0 would not need optional segments, catch-alls, or regex.

### 4. What this is not

- Not Node `module.exports` / `require` as a value.
- Not a user-facing `type Module`.
- Not Crema domain in `std/`.
- Not a reason to keep “file is an instantiated type.” That story was
  never surfaced (D33: namespace is not a value) and is not a future we
  are banking.

---

## If this is unparked

1. Decide discovery (compiler walk of a reserved tree vs generated
   `routes` barrel). Packages stay import-closed either way.
2. Pick **route-line-in-file** vs **`+name` in the tree**. Do not do both
   as two official spellings.
3. If the tree carries captures, lock `+name` (or reject it) and migrate
   type-body clauses off `:slug` so there is one hole marker.
4. `index.thx` / `layout.thx` as reserved names only if still wanted;
   keep today’s `<Page>` if not.
5. Stay example-only until Crema forces a product helper. No handbook
   until then.

**Pickup:** this file. Priority stays in root [`CURRENT.md`](../../CURRENT.md).
