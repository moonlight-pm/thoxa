# Ideas from Idris 2: a survey

**Status:** sleeping. Exploration only: nothing here is decided or scheduled,
and no code or freeze changes with it.
**Date:** 2026-11-16.
**Sparked by:** Joshua's request to survey
[A Crash Course in Idris 2][i-intro] for concepts that might improve Thoxa.
**Related:** [closures and ORC](2026-08-11-first-class-functions-and-orc.md),
[having / else](2026-07-20-having-else-may.md),
[generated codecs for generic types](2026-10-19-generic-codecs.md),
[agent DX](2026-08-09-agent-dx-from-zerolang.md); decisions D2 (effects
deferred), D8 (Perceus later), D131 (multi-clause functions), D168 (resource
constructors), D232 (generic bodies checked once), D244 (operators stay built
in), D247 (names come from imports).

## How to read this

Idris 2 is a pure functional language with dependent types (a type may
mention a value), a quantity on every variable (erased, used exactly once, or
unrestricted), a totality checker, and editing commands driven by types.
Thoxa is an imperative language with value semantics that compiles to C. Few
Idris features transfer as written. What transfers is the question each one
answers, so this survey goes through the tutorial chapter by chapter, takes
each concept, and asks what form of it would suit Thoxa.

Each idea has four parts. **Idris** says what the tutorial shows, with a link
to the section. **As built** says what Thoxa does today; every such statement
was checked against the handbook or against `thoxa check` at `c729c71`.
**Idea** is a sketch. Code under it is not valid Thoxa today, and its
spelling is a placeholder, not a proposed surface. **Improves** says what
Thoxa gains in correctness or in what it can express, and what the idea costs.

Verdicts:

| Verdict | Meaning |
|---------|---------|
| **fits** | Compatible with the language as it stands; could become a freeze on its own |
| **reshapes** | Fits the direction, and reopens a freeze, a parked idea, or an entry under "Not in the language" |
| **fundamental** | Changes what the type system or the memory model is; the section says how |
| **no** | Surveyed and not proposed; the reason is recorded so the question is not asked again |

An idea promoted from this file gets its own freeze and D-id, as the ideas
README says. This file stays the survey.

## Summary

Gain: **C** is correctness, **E** is what the language can express.

| # | Idea | Idris concept | Verdict | Gain | Size |
|---|------|---------------|---------|------|------|
| 1 | [Typed holes](#1-typed-holes) | Holes | fits | C, E | small |
| 2 | [Editor actions from types](#2-editor-actions-from-types) | Interactive editing | fits | C | small, after 1 |
| 3 | [Private constructors for exported types](#3-private-constructors-for-exported-types) | Export modifiers | fits | C | small |
| 4 | [`Never` as a written type](#4-never-as-a-written-type) | The empty type | fits | C, E | small |
| 5 | [Results that must be used](#5-results-that-must-be-used) | Linearity, lightest form | fits | C | small |
| 6 | [Record update by spread](#6-record-update-by-spread) | Record update | fits | E, C | small |
| 7 | [Static trait members](#7-static-trait-members) | Interface members without a value | fits | E | medium |
| 8 | [Lazy parameters](#8-lazy-parameters) | `Lazy` | fits | E, C | medium |
| 9 | [`for` as a collecting expression](#9-for-as-a-collecting-expression) | Comprehensions | fits | E | small |
| 10 | [Scoped `?`](#10-scoped-) | `do` blocks | fits | E | small |
| 11 | [Checked handbook samples](#11-checked-handbook-samples) | Literate programming | fits | C | small |
| 12 | [Nested patterns, patterns in `let`](#12-nested-patterns-patterns-in-let-several-scrutinees) | Pattern matching, pattern-matching bind, `with` | reshapes | C, E | medium |
| 13 | [Clause-precise return types](#13-clause-precise-return-types) | First-class types | reshapes | E, C | small |
| 14 | [Recursive types](#14-recursive-types) | Data types | reshapes | E, C | medium |
| 15 | [Lambdas that capture values](#15-lambdas-that-capture-values) | Anonymous functions, `where` | reshapes | E | large |
| 16 | [Anonymous labelled products](#16-anonymous-labelled-products) | Tuples | reshapes | E | medium |
| 17 | [Flow typing for refinements](#17-flow-typing-for-refinements) | Auto implicits, `Dec` | reshapes | C, E | medium |
| 18 | [Total functions](#18-total-functions) | Totality | reshapes | C | large, after 17 |
| 19 | [Pure functions as predicates](#19-pure-functions-as-predicates-evaluated-at-compile-time) | Total functions in types, `Refl` | reshapes | C, E | large |
| 20 | [Refinements that mention other values](#20-refinements-that-mention-other-values) | `Fin`, `Vect`, dependent records | fundamental | C, E | large, after 17 |
| 21 | [Numbers as type parameters](#21-numbers-as-type-parameters) | Indexed types, erasure | fundamental | C | large |
| 22 | [Conditional union members](#22-conditional-union-members) | Indexed families (the well-typed interpreter) | fundamental | C, E | large, after 14 |
| 23 | [Single-owner values](#23-single-owner-values) | Multiplicities, resource protocols | fundamental | C | very large |
| 24 | [Effects in signatures](#24-effects-in-signatures) | `IO` | fundamental | C | very large |

[Not proposed](#surveyed-and-not-proposed) lists the rest of the tutorial.

## What Thoxa already shares with Idris

So that the list below is not read as a list of everything Idris has:

- A `match` on a union covers every member or says `else`. That is Idris's
  *covering* requirement ([Totality and Covering][i-covering]).
- Failure is a value: `May` and `Trial` are Idris's `Maybe` and `Either`.
  The postfix `?` does what [`!`-notation][i-bang] does, and Thoxa's marks
  (`!` and `?` on union members) open it to any user union, which is the
  useful part of [`do`-notation][i-monads] without monads.
- Traits are [interfaces][i-interfaces]: required and provided members
  ([default definitions][i-defaults]), parents ([extending
  interfaces][i-extending]), conformances with bounds (`Show a => Show (Vect
  n a)`). Associated types do the work of [determining
  parameters][i-determining].
- Literal types and `where` refinements are the part of dependent typing that
  needs no proofs: a type that says which values it holds. `x as T` is
  Idris's [`cast`][i-cast] with the failure in the type.
- Explicit type arguments (`none<String>()`) are Idris's `{a = String}`;
  parameter defaults are its [default implicit arguments][i-default-implicit].
- Value semantics gives the main thing purity gives Idris: no alias can
  change a value someone else holds. Several ideas below (14, 15, 17, 20, 23)
  are cheap in Thoxa for that reason and expensive in languages with shared
  references.

---

## Fits

### 1. Typed holes

**Idris.** `?name` stands for a part of the program that is not written yet.
The program still type checks, and asking for the hole's type shows the type
expected there and every variable in scope with its type
([Holes][i-holes]). The tutorial writes most of its functions this way: leave
a hole, read what is needed, fill it.

**As built.** A function with an unwritten part does not check, so the part
gets a stand-in of the right type (`0`, `""`, `Not()`). The stand-in checks
and runs, and nothing marks it as unfinished. `thoxa check --json` reports
errors (D124); there is no way to ask what a position expects.

**Idea.** `?name` is an expression that fits any expected type, as Never
does. `thoxa check` reports each hole as its own kind of diagnostic: the
expected type, and the bindings in scope with their types and whether they
are `mut`. `--json` carries the same as data, and the editor shows it on
hover. `thoxa build` refuses a program that has holes (or, as an option,
compiles each to a panic so the finished paths can be run).

```thx
fn area(shape!: Shape) -> Float64 {
    match shape {
        Circle(radius) => 3.14159 * radius * radius
        Rect(width, height) => ?rectArea
    }
}
## hole `rectArea`: expected Float64
##   width: Float64, height: Float64, shape: Shape
```

**Improves.** Correctness: the rest of the program is checked while a part is
missing, and an unfinished part cannot ship as a plausible value. What it
expresses: the question "what goes here, and what do I have", which is how
Idris programs are written and what an agent needs between edits (see the
agent DX note).

**Open.** The handbook already calls `{expr}` in a string a hole; this needs
another word. Spelling: `count ?rest` parses today as `count?` followed by
`rest`, so the form must be one the postfix `?` cannot reach.

### 2. Editor actions from types

**Idris.** The REPL and editors generate code from types: a clause for a
declared function ([`:addclause`][i-addclause]), one clause per constructor
of a variable ([`:casesplit`][i-casesplit]), the clauses a function lacks
([`:addmissing`][i-addmissing]), and a value for a hole found by search
([`:proofsearch`][i-proofsearch]).

**As built.** The LSP has hover, definition, references, rename, and outline.
The compiler already computes what these commands need: the exhaustiveness
error names a match's missing members, and a conformance that lacks a
required member is already an error.

**Idea.** Code actions in `thoxa lsp`, and the same edits as data in `check
--json` (the agent DX note's "fix plans"): add the missing arms of a
`match`; turn a binding of a union type into a `match` with one arm per
member, each a hole; add the members a `conform` body lacks; offer the
bindings in scope that fit a hole.

**Improves.** Correctness, indirectly: generated arms are complete, so nobody
writes `else` to save typing, and an `else` hides every member added later.
Needs idea 1 for the bodies it generates.

### 3. Private constructors for exported types

**Idris.** A type has three visibilities. `private` is the default. `export`
exports the type's name and not its constructors, so other modules can hold
and pass values and can make one only through the module's functions. `public
export` exports the constructors too ([Export modifiers][i-export],
[Meaning for data types][i-export-data]). A definition may not mention a name
less visible than itself, so a private type cannot leak through an exported
signature.

**As built.** `export type` exports the name, the fields for reading, and the
constructor together. Only a `Resource`'s constructor is private to its
module (D168). So the handbook's own example,

```thx
type Email(address: String) {
    export from(text: String) -> Trial<Email, String> { … }   ## checks for "@"
}
```

protects nothing: another module writes `Email(address: "nonsense")` and it
checks. A `where` refinement does guard its predicate, and covers only
comparisons, ranges, and literals. Separately, an exported function may return
a type that is not exported, and the importer reads its fields.

**Idea.** A way to export a type and keep its constructor in the module, as
D168 does for resources. The fields stay readable. Every value outside the
module then came through a `from` clause or an exported function, so what
those check holds for every value in the program.

**Improves.** Correctness: an invariant written once is true everywhere.
Today it is true by convention. This is "parse, don't validate" made
enforceable, for invariants a `where` clause cannot state. Of everything in
this survey it has the best ratio of gain to cost.

**Open.** The generated `Decodable` reader constructs the type; such a type
would need its own `from(json: Json)` or a rule that the generated reader
goes through a declared clause. Idea 6's spread is construction and follows
the same rule. Whether an exported signature naming an unexported type is an
error or a warning.

### 4. `Never` as a written type

**Idris.** `Void` is the type with no values. A function that returns it
cannot return, and from a value of it anything follows ([The Empty
Type][i-empty]). (The names cross: Idris's `Void` is Thoxa's Never, and
Thoxa's `Void` is Idris's unit `()`.)

**As built.** Never exists in the checker: it is the type of `return`, of a
`loop` with no `break`, and of a `match` whose arms all diverge (D128, D164).
It cannot be written. `fn stop(message!: String) -> Never` is `unknown type
Never`. A user function that always exits, panics, or serves forever cannot
say so, and a call to it does not fit as one branch beside a value.

**Idea.** `Never` is a type a program can write. As a return type it is
checked: the body must diverge. As a type argument it removes a member:
`Trial<Config, Never>` cannot be a `Fail`, so a `match` on it needs no `Fail`
arm, and a `May<Never>` is always `Not`.

**Improves.** Correctness: exhaustiveness is exact for an instance whose
member cannot exist, and divergence shows in the signature. What it
expresses: diverging helpers (`fn fatal(message!: String) -> Never`), and
generic code used where nothing can fail, such as a trait whose member
returns `Trial<Item, Failure>` with `type Failure = Never` for the types that
always succeed.

### 5. Results that must be used

**Idris.** A variable with quantity 1 must be used exactly once; dropping it
is an error, `There are 0 uses of linear name` ([Linearity][i-linearity]).
Idea 23 is the whole of that. This is its smallest slice.

**As built.** `half(3)` as a statement, where `half` returns a `Trial`,
checks without a diagnostic. D3 says failure must be dealt with, which holds
once the value is looked at, and not when it is dropped. There is no
unused-local lint either.

**Idea.** A statement whose value is a marked union (one with a `?` member)
is an error. `let _ = half(3)` discards on purpose.

**Improves.** Correctness: it closes the one path by which a failure
disappears without a word in the source.

**Open.** Whether it extends to every non-`Void` statement as a warning.

### 6. Record update by spread

**Idris.** `{ firstName := "Jim", age $= (+ 1) } fred` is a copy of `fred`
with those fields changed, and the path may be nested: `{ a.b.c := val } x`
([Records][i-records], [Nested record update][i-record-update]).

**As built.** From outside its type a field is read-only, so a changed copy
is the constructor called again with every field: `Point(x: point.x, y: 5)`.
A field added later with a default is silently reset to the default at every
such site. Spread exists in two places: a `Json` object literal
(`{ ...ticket, ok: true }`) and a component's attributes.

**Idea.** The same spread in a constructor call: `Point(...point, y: 5)`
takes the fields it does not name from a value of the same type. It is
construction, so setters and refinement checks run as they do today.

**Improves.** What it expresses: a copy with a change is the ordinary way to
alter a `let` value under value semantics, and today it costs one argument
per field. Correctness: fields the call does not name are carried, not reset.
Under Perceus a spread of a value held once is a write in place.

**Open.** Nested paths (`Order(...order, customer.city: "Paris")`).

### 7. Static trait members

**Idris.** An interface's members need not take a value of the type:
`neutral : ty` in `Monoid` ([Named implementations][i-named] shows it),
`pure : a -> f a` in `Applicative` ([Functors and
Applicatives][i-functors]). Generic code reaches them through the constraint.

**As built.** A trait's members are methods and properties of a value. No
member is called on the type. `T.read(text)` exists and is supplied by the
compiler for `Decodable`; a trait cannot declare such a thing. The generic
codecs note is parked on exactly this ("Static requirements in traits").

**Idea.** A trait may require members of the type itself:

```thx
trait Summable {
    static zero: Self
    fn plus(other!: Self) -> Self
}

fn sum<T: Summable>(items!: Array<T>) -> T {
    mut total = T.zero
    for item in items {
        total = total.plus(item)
    }
    total
}
```

`Decodable` could then declare `static fn from(json: Json) -> Trial<Self,
Array<Issue>>`, which gives generic code `json as T` and wakes the generic
codecs idea.

**Improves.** What it expresses: generic code can make a `T` (an identity, a
default, a parsed value). `sum` over an empty array cannot be written
generically today. Correctness: `Decodable` becomes an ordinary trait where
it is now a case the compiler knows by name.

**Open.** A static member has no value to dispatch on, so it is not reachable
through `mixed`.

### 8. Lazy parameters

**Idris.** A parameter of type `Lazy a` is not evaluated until the function
uses it; the compiler inserts the delay and the force. The tutorial's
examples are a user-defined `ifThenElse` and `maybe : Lazy b -> Lazy (a -> b)
-> Maybe a -> b`, whose default is computed only when needed
([Laziness][i-lazy]).

**As built.** Only `and`, `or`, `if`, and `match` leave an operand
unevaluated. There are no closures, so a function cannot take "an expression
to evaluate if needed". `debug("state {expensive()}")` builds its string and
then `std/logging` drops it when the level is off. `std/fallible` has no
"value or default" helper, and one written today would compute its default
every time.

**Idea.** A parameter marked lazy. The call site writes an ordinary
expression; the callee evaluates it where it reads the parameter, at most
once. The parameter cannot be stored, returned, or passed on except as lazy,
so it needs no heap environment and can make no cycle. That keeps it clear of
what parked closures (idea 15): it can be compiled as code plus a pointer to
the caller's frame, or by inlining.

```thx
fn orElse<T>(value!: May<T>, fallback!: lazy T) -> T {
    match value {
        Has(found) => found
        Not => fallback
    }
}

let port = orElse(configured, readPortFromDisk())   ## reads only when missing
```

**Improves.** What it expresses: user-defined operations that short-circuit,
which today only the compiler can write: defaults, assertions with built
messages, logging that costs nothing when off. Correctness: an argument's
effects and cost happen only when the value is used, where today they always
happen and nothing at the call says so.

**Open.** Whether the call site marks it. Thoxa marks `mut` at the call, and
D247's "avoid surprises" argues the same here; Idris leaves it invisible.

### 9. `for` as a collecting expression

**Idris.** `[ expression | x <- xs, condition ]` builds a list from
generators and guards ([List comprehensions][i-comprehensions]).

**As built.** `{for x in xs { child }}` collects an `Array<Node>`, and only in
the child position of a tag literal. Everywhere else the choice is a `mut`
accumulator with `append` in a loop, or `.map(transform)` with a named
top-level function, which cannot see the caller's locals.

**Idea.** Make the tag literal's form the general one:

```thx
let totals = [for line in lines if line.quantity > 0 { line.total() }]
```

an `Array` of the block's values. The body is ordinary code in the enclosing
function, so it sees locals. That is most of what a closure passed to `map`
or `filter` would be for.

**Improves.** What it expresses: map and filter over locals without closures.
Correctness, in a small way: the result is a `let`, with no `mut` accumulator
left in scope.

**Open.** A map-building form. Ranges are not expressions today, so
`[for index in 0..count { … }]` works and a bare `0..count` value does not.

### 10. Scoped `?`

**Idris.** A `do` block is an expression. A failed bind ends the block, not
the function, and the block's value is the `Maybe` ([Monads and
`do`-notation][i-monads]). [Idiom brackets][i-idiom] do the same for one
application: `[| x + y |]`.

**As built.** `?` returns from the enclosing function. Using it for part of a
function means moving that part to a helper.

**Idea.** A block whose `?` leaves the block: `let sum = try { Has(left? +
right?) }` is a `May<Int64>`.

**Improves.** What it expresses, modestly. Recorded because it completes the
correspondence with `do`; nothing has asked for it yet.

### 11. Checked handbook samples

**Idris.** A `.lidr` file is prose in which lines starting `>` are the
program; the compiler checks it ([Literate programming][i-literate]).

**As built.** The handbook says its programs compile and run as shown. The
site renders the handbook; no crate reads it, so nothing checks that claim.
The nine samples on the home page are run by `cargo make test`.

**Idea.** `thoxa check` over a Markdown file's `thx` blocks, or a `cargo make`
step that does it for `docs/handbook/`, with a way to mark a block as a
fragment or as an expected error.

**Improves.** Correctness of the product docs: the handbook is "shipped truth
only", and this makes the compiler hold it to that.

---

## Reshapes

### 12. Nested patterns, patterns in `let`, several scrutinees

**Idris.** Patterns nest in clauses and in `case`. A `let` may match a
pattern: `let MkPerson name age = p` ([`let` bindings][i-let]). A bind in a
`do` block may match a pattern and give the other cases beside it: `Just x_ok
<- readNumber | Nothing => pure Nothing` ([Pattern matching
bind][i-pattern-bind]). `with (n + 1) | (m + 1)` matches several values at
once ([the `with` rule][i-with]).

**As built.** Patterns do not nest (it is on the handbook's "Not in the
language" list): match the outer value, then match what it bound. `let` binds
a name. A `match` has one scrutinee.

**Idea.** One mechanism, four uses:

```thx
match found {
    Has(Circle(radius)) => radius          ## nested
    Has(Rect(width, _)) => width
    Not => 0.0
}

let Point(x, y) = point                    ## a product has one shape: cannot fail

let Has(user) = lookup(id) else {          ## can fail: `else` must diverge
    return notFound()
}

match left, right {                        ## several scrutinees
    Has(a), Has(b) => a + b
    else => 0
}
```

The third is Idris's pattern-matching bind, and it is what the parked `having
/ else` note wanted. That note declined a keyword for it. Here it costs none:
it falls out of patterns in `let`.

**Improves.** Correctness: a two-level match is written today as a `match`
inside an arm, and the inner one tends to end in `else`, which stops the
checker from seeing the combinations. A nested pattern is checked whole. What
it expresses is plain from the examples.

**Cost.** A real exhaustiveness algorithm over pattern matrices, and lowering
to MIR. `?` already covers the marked-union case; `let … else` covers unions
without marks and exits other than returning the residual.

### 13. Clause-precise return types

**Idris.** A function can compute a type from a value, and another function's
return type can call it: `isSingleton : Bool -> Type`, then `mkSingle : (x :
Bool) -> isSingleton x` returns a `Nat` for `True` and a `List Nat` for
`False` ([First-class types][i-fctypes]).

**As built.** A multi-clause function dispatches on its parameters' types,
literal types included, and has one return type, the join of its clauses
(multi-clause freeze §8). With

```thx
fn setting(name!: "port") -> Int64 { 8080 }
fn setting(name!: "host") -> String { "localhost" }
```

`setting("port")` is an `Int64 | String`, and the caller matches on a result
whose member it already knows.

**Idea.** When the argument types at a call select exactly one clause, the
call has that clause's return type. A call with a wider argument (a variable
of type `"port" | "host"`) keeps the join.

**Improves.** What it expresses: this is the dependent-types idiom Thoxa can
have almost free, because a literal type already carries a value into the
type: typed configuration keys, column names, event names, route names.
Correctness: no arms for members that cannot occur. The checker already
selects the clause to compile the call.

### 14. Recursive types

**Idris.** `data Nat = Z | S Nat`, `data List a = Nil | (::) a (List a)`, and
the expression tree of the interpreter chapter: data that contains its own
type ([Data types][i-data], [the well-typed interpreter][i-interp]).

**As built.** `type Add(left: Expr, right: Expr)` with `type Expr = Number |
Add` is an error: "constructor-param type cannot contain itself through
fields (Expr → Add → Expr); use Array or Map for a recursive shape". A tree
holds its children in an `Array`, so a binary node is an array that holds two
by convention, and each read of a child is an index that can panic. `Json` in
std is built this way. The handbook's Memory section rests the absence of
reference cycles on this rule.

**Idea.** Allow a product to contain its own type through a union or a `May`,
with the recursive field stored as a reference-counted box. The absence of
cycles does not depend on the ban. A value is built from values that already
exist, and a write through a `mut` place copies when the storage is shared,
so no value can come to contain itself. A `Resource` is the exception (its
methods write a shared box in place) and keeps the ban.

**Improves.** What it expresses: syntax trees for a self-hosted compiler,
linked structures, any grammar-shaped data. Correctness: the arity is in the
type (`Add(left:, right:)`), children are read without an index, and a match
on tree shape is exhaustive, more so with idea 12.

**Cost.** Layout for the boxed back edge, and releasing a long chain without
recursing once per link. `==`, `hash`, and `show` already walk to any depth.
Idea 22 needs this.

### 15. Lambdas that capture values

**Idris.** Anonymous functions (`\x => x * 2`), operator sections (`(* 2)`),
and local functions in `where` blocks that see the enclosing function's names
([Anonymous functions and operator sections][i-lambda], [`where`
clauses][i-where]).

**As built.** No closures and no nested functions. A function value is a
top-level function or a bound method. The closures note parked the feature
because the capture it chose, shared bindings as in JavaScript, makes cycles
easy and so needs a cycle collector.

**Idea.** The other answer to that note's first open question: a lambda
captures values, not bindings. For a `let` nobody can tell the difference,
since nobody can write it. A `mut` binding is captured as its value at that
moment, or is not capturable (Java's rule; Swift's capture lists; C++'s
`[=]`). An environment made of values that existed before the lambda cannot
contain the lambda, so no cycle forms and plain reference counting is enough.
No collector.

```thx
let cheap = lines.filter((line) => line.price < limit)   ## `limit` is copied in
```

**Improves.** What it expresses: this is the largest single gain in the
survey: `map`, `filter`, sorting by a key, handlers that carry configuration.
Correctness: a closure is a value like any other, with no action at a
distance through a captured variable.

**Cost.** Large: function values that carry an environment, function types in
fields and arrays (barred today), inference of a lambda's parameter types,
`-->` with a lambda. Ideas 8 and 9 give part of the benefit without any of
this, and stay useful if this ships. A local named function is the same
feature with a name.

### 16. Anonymous labelled products

**Idris.** `(a, b)` is a pair and longer tuples are nested pairs, read with
`fst` and `snd` or by pattern ([Tuples][i-tuples]).

**As built.** No tuples (on the "Not in the language" list). Two results need
a declared type: `Split(before:, after:)`, `Entry<K, V>`.

**Idea.** If Thoxa had them they would be labelled, as its calls are: a type
`(before: String, after: String)`, written the way a parameter list already
is, with values `(before: head, after: tail)` and destructuring through idea
12.

**Improves.** What it expresses, for one-off groupings: two results, several
scrutinees. Correctness: nothing, and a named type documents more. A weak
fit: a product costs one line to declare. Recorded so the question has an
answer; not pressed.

### 17. Flow typing for refinements

**Idris.** `head : (xs : List a) -> {auto p : isCons xs = True} -> a` takes a
proof that the list is not empty, and the compiler finds it "either because
its value is known or because a proof already exists in the context" ([Auto
implicit arguments][i-auto]). A check that returns evidence instead of a
`Bool` (`checkEqNat`, `Dec`) is how such a proof comes to exist ([Theorems in
practice][i-practice]).

**As built.** Thoxa has the evidence-returning check: `count as Quantity`
gives `Trial<Quantity, Array<Issue>>`, and a literal is checked at compile
time. It lacks the proof already in the context:

```thx
type Quantity = Int64 where >= 1
fn reserve(quantity!: Quantity) { … }

if count >= 1 {
    reserve(count)        ## error today: expected Quantity, found Int64
}
```

The program must write `count as Quantity` and handle a `Fail` that cannot
happen.

**Idea.** A condition narrows what it tests. Inside `if count >= 1 { … }`,
`count` is an `Int64 where >= 1` and fits `Quantity`. The same for the `else`
branch (the negation), `and` chains, `while` conditions, literal arms of a
`match`, and `.count` (`if items.count > 0` makes `items` an `Array<T> where
1..`). It applies to `let` bindings and parameters, and to a `mut` binding up
to its next write. Value semantics makes this sound without further analysis:
nothing else can change the value.

**Improves.** Correctness: no arms for failures that cannot occur, which is
where careless code collects. It also makes a refined parameter type free to
satisfy, so functions can ask for what they need. What it expresses:
refinements become usable in ordinary code, not only where data enters.

**Cost.** The checker carries facts per branch. The predicate language is
small and decidable (comparisons, ranges, `&`, `|`), so "does this fact imply
that predicate" is interval arithmetic. Ideas 18 and 20 build on this.

### 18. Total functions

**Idris.** A function is *total* when it covers every input, its recursion
decreases, and it calls only total functions. `total` makes that a checked
claim, `partial` opts out, `%default total` sets a file's default, and
`assert_total` / `assert_smaller` are visible escape hatches ([Totality and
Covering][i-covering], [Totality][i-totality], [Totality
checking][i-totality-checking], [Hints for totality][i-totality-hints]).
"If it's total, it will return a value of type `String` in finite time."

**As built.** Matches are covering. The partial functions are the operations
that panic: `a[i]`, `/` and `%`, a negative shift, implicit narrowing,
`dropLast`, `insert` and `removeAt`, `Bytes` indexing, `main`'s status.
Nothing in a signature says whether a function can panic. An `Array` has no
read that cannot panic (`m[key]` on a `Map` is a `May<V>`; `a[i]` panics, and
there is no `items.get(index)`).

**Idea.** In two layers.

1. Every partial primitive gets a total form: a read that returns `May<T>`,
   and operations whose precondition is a refined type, so `a / b` needs no
   check when `b` is an `Int64 where != 0`.
2. An opt-in marker on a function (or a module default, as `%default total`)
   under which an operation that can panic, or a call to a function not known
   to be total, is a compile error that names it. The escape hatch is an
   explicit assertion, visible in the source.

Idris also checks termination. For an imperative language with `while` and
`loop` that is a poor fit and is not proposed; "cannot panic" is the half
that matters to a server.

**Improves.** Correctness: for a request handler, a `drop`, or a task (whose
panic leaks its memory), "cannot panic" is a guarantee nobody can get today
without reading every callee.

**Cost.** It is only bearable with idea 17, and index bounds need idea 20.
The property crosses modules, so it becomes part of a function's exported
contract. Out of memory and stack overflow stay outside it, as resource
exhaustion stays outside Idris's.

### 19. Pure functions as predicates, evaluated at compile time

**Idris.** The type checker evaluates total functions, and only those, so a
type may call one: `Vect (n + m) a`. `public export` is what makes a
definition usable this way from another module ([Meaning for
functions][i-export-fn]). `twoPlusTwo : 2 + 2 = 4` with the proof `Refl` is
an equality the compiler checks by evaluating both sides
([Equality][i-equality]).

**As built.** A `where` predicate is drawn from a fixed language:
comparisons, ranges, literals. "There are no predicates across fields of a
product and no regular expressions." An email address cannot be a refinement.
Purity is checked in one place: a getter may not write.

**Idea.** A function that is pure (it reads only its arguments: no module
`mut`, no `extern`, no task) and total (idea 18) can be used in three places:

```thx
type Email = String where isEmail            ## a predicate; `as` checks it at run time
let admin: Email = "ada@example.com"         ## a literal: evaluated by the compiler
check slug("Hello World") == "hello-world"   ## a top-level fact, checked at compile time
```

The last is Idris's `Refl` used as a test: an example that cannot go stale.

**Improves.** Correctness: invariants beyond ranges become types, by a second
route beside idea 3. What it expresses: any decidable property as a `where`.

**Cost.** Large: the compiler needs to run Thoxa. The phase A rule against a
tree-walk interpreter is about running programs; an evaluator for pure total
functions over MIR is a different thing and still real work (the compiler
could also build and run the predicate, as it already drives `cc`). A checked
`pure` marker alone is small; see idea 24.

---

## Fundamental

### 20. Refinements that mention other values

**Idris.** `Fin n` is the type of numbers below `n`, and `index : Fin n ->
Vect n a -> a` reads a vector with no bounds check and no failure case,
because the index's type mentions the vector's length ([The finite
sets][i-fin]). `(++) : Vect n a -> Vect m a -> Vect (n + m) a` says what
appending does to lengths ([Vectors][i-vect]). A record's field may mention
another field ([Dependent records][i-dependent-records]).

**As built.** A predicate mentions constants only: `Int64 where 0..24`.

**Idea.** Let a predicate name a binding in scope:

```thx
fn at<T>(items!: Array<T>, index!: Int64 where 0..items.count) -> T {
    items[index]                             ## cannot panic; compiled without a check
}

type Window(low: Int64, high: Int64 where >= low)
```

A caller meets the predicate with a literal, with a fact from idea 17 (`if
index < items.count`, or `for index in 0..items.count`, which states it
outright), or with `as`, which checks at run time and gives a `Trial`.

This is the design point that makes dependent typing plausible for Thoxa:
where Idris demands a proof, Thoxa accepts a checked conversion. The compiler
gives the static guarantee where it can see one; the existing run-time check
covers the rest; the types are the same either way.

**Impact on the language.** A type is no longer a closed term. It can mention
a binding, so whether one type fits another can depend on facts about values,
and the checker must decide implications between predicates with variables.
Linear integer arithmetic is the reasonable limit; past it the answer is
"write `as`". A fact about a `mut` binding ends at its next write, and a fact
that names `items.count` ends when `items` is written. No aliasing is what
keeps that rule short; it is why refinement types are hard in languages with
references and tractable here. Code generation does not change: a refined
value is its base value at run time, as now. The stopping point matters:
predicates on parameters and fields (preconditions, checked at the call or
the construction) and not on results. A result predicate (`-> Array<T> where
.count == left.count + right.count`) turns the checker into a verifier of
function bodies, which is Idris's side of the line.

**Improves.** Correctness: an index out of range, lengths that differ, a
range written backwards become type errors or an explicit `Trial` where the
data enters, instead of a panic deep inside. What it expresses: signatures
state preconditions that live in `##` comments today.

### 21. Numbers as type parameters

**Idris.** `Vect : Nat -> Type -> Type` is indexed by a number, and types do
arithmetic on it. Whether the number exists at run time is a quantity: erased
by default, kept when written `{n : Nat}` ([Vectors][i-vect],
[Erasure][i-erasure]).

**As built.** `Array<T, N>` with a literal `N` is the one place a number sits
in a type. A declared type cannot take one: `Grid<4>` for `type Grid<N>(cells:
Array<Int64, N>)` fails inside the compiler ("literal type `4` was not
materialized").

**Idea.** Type parameters that stand for integers:

```thx
type Matrix<Rows: Int64, Columns: Int64>(cells: Array<Float64, Rows * Columns>)

fn multiply<R, K, C>(left!: Matrix<R, K>, right!: Matrix<K, C>) -> Matrix<R, C>
```

Each instance is compiled separately, as for type parameters, so `Rows` is a
constant in the body and usable as a value. Idris needs the erasure rule to
say when `n` exists at run time; monomorphization answers it without a rule.

**Impact on the language.** Type equality now includes arithmetic (`N + M`
against `M + N`), which Idris settles with proofs and `rewrite` ([Theorems in
practice][i-practice]); Thoxa would normalize linear expressions and refuse
the rest. One copy of the code per distinct number. Only sizes known at
compile time qualify: a length read from input cannot be a type argument.
That case is idea 20's, and Idris's answer to it, the [dependent
pair][i-dpair] `(n ** Vect n a)`, is in Thoxa simply `Array<T>`. So ideas 20
and 21 are the two halves of `Vect n a`: 21 for sizes fixed in the source (C
buffers, matrices, protocol frames), 20 for sizes found at run time.

**Improves.** Correctness for fixed-layout code. For the servers and tools
Thoxa is written for, idea 20 is worth more.

### 22. Conditional union members

**Idris.** The interpreter chapter's `Expr` is a family of types indexed by
the type of value an expression produces. Each constructor fixes the index,
so an ill-typed expression cannot be built, and `interp` returns the right
type for each with no tags and no run-time type errors ([Representing
languages][i-interp-lang], [Writing the interpreter][i-interp-write]).

**As built.** A generic union has the same members in every instance. The
neighbouring features exist: a conformance that holds for some instances
(`conform Box<T: Equatable> to Equatable`) and same-type constraints on
functions (`where S.Item == Int64`).

**Idea.** A member guarded by a constraint on the union's parameters
(spelling a placeholder):

```thx
type Expr<T> =
    Number  when T == Int64       ## Number(value: Int64)
  | Compare when T == Bool        ## Compare(left: Expr<Int64>, right: Expr<Int64>)
  | If<T>                         ## If(test: Expr<Bool>, then: Expr<T>, else: Expr<T>)

fn evaluate(expression!: Expr<Int64>) -> Int64 { … }   ## no Compare arm: it cannot occur
fn evaluate(expression!: Expr<Bool>) -> Bool { … }
```

Under monomorphization each instance is an ordinary union with fewer members,
so representation and matching need nothing new.

**Impact on the language.** A union becomes a family: its members, and so its
exhaustiveness, depend on the instance. The hard part is a function generic
in `T` that matches an `Expr<T>` and must learn `T == Int64` inside the
`Number` arm. That is type equality local to an arm, and it sits badly with
D232 (a generic body is checked once, against its bounds). Multi-clause
functions over concrete instances, as above, avoid it and may be all that is
needed. Requires idea 14.

**Improves.** Correctness: trees that cannot be ill-typed, such as a
compiler's IR after checking, a typed query, a typed route table. What it
expresses: embedded languages whose types Thoxa checks.

### 23. Single-owner values

**Idris.** Every variable has a quantity. With quantity 1, `openDoor : (1 d :
Door Closed) -> Door Open` consumes its argument: after the call the old door
cannot be used, and a program that does not finish the protocol does not
compile. The type's index holds the state and linearity keeps stale states
out of reach ([Linearity][i-linearity], [Resource protocols][i-protocols]).
The chapter ends by asking whether linearity can inform memory management.

**As built.** A `Resource` is a shared, counted box. An explicit `close`
leaves every other copy holding a closed resource, so use after close is a
state each std function tests for at run time (`fd` is -1). A socket's
ownership by a task is recorded at run time (`net.ownByTask`). The state half
of the door example works today: with `type Door<State>(id: Int64)` and
marker types, `fn open(door!: Door<Closed>) -> Door<Open>` checks. The
linear half is missing: calling `open(closed)` twice checks too.

**Idea.** A kind of type whose values have one owner. Passing, returning,
storing, or assigning such a value moves it, and using the old binding
afterwards is a compile error. A `mut` parameter lends it for the call, which
is already exclusive access in place. A value still owned at the end of its
scope is dropped ("at most once", affine); a stricter marker could demand an
explicit end ("exactly once", linear: a transaction is committed or rolled
back, never forgotten).

```thx
let closed = install()            ## Door<Closed>, single-owner
let opened = open(closed)
open(closed)                      ## error under this idea: `closed` was moved
```

What follows from it: `close` consumes, so use after close and double close
do not compile; protocols such as listen, accept, read, close are checked in
order; giving a socket to a `-->` task is a move, checked, in place of
`ownByTask`; and a single-owner value needs no reference count, with update
in place guaranteed where Perceus would only discover it at run time.

**Impact on the language.** The largest in this survey. Today every value can
be copied anywhere, and much of Thoxa's simplicity is that sentence. With
single-owner types:

- generic code must say whether it copies. Every type parameter is either
  known copyable or not (a bound with an opt-out, as Swift's `~Copyable`, or
  a quantity on the binder, as Idris);
- `Array<T>` and `Map<K, V>` of such values need operations that move out;
- `for` (which reads a copy), match bindings, `self` (alone, a copy of the
  whole value), field write-back, and `mixed` each need a moving form;
- every early exit (`return`, `?`) drops what is still owned, and under the
  strict marker each exit must consume it or fail to compile.

Confining it to resources that cannot be type arguments would be a first cut
that later breaks code, so the generic story has to be designed before any of
it ships. In its favour: this is not a borrow checker. There are no
references, so there are no lifetimes; `mut` parameters are the only loans
and they end with the call. Thoxa's value model makes ownership far cheaper
than it is in Rust, which is the reason to consider it at all.

**Improves.** Correctness: resource protocols checked at compile time, and no
closed state to observe. Performance: resources and buffers without counts.
Idea 5 is the slice of this that costs almost nothing.

### 24. Effects in signatures

**Idris.** `IO a` is the type of a computation that interacts with the world,
so a function's type says whether it does: `main : IO ()`, `getLine : IO
String`, and a function without `IO` cannot print ([I/O][i-io]).

**As built.** Any function may do anything. D2 defers algebraic effects.

**Idea.** Not monadic `IO`. The part that transfers is the distinction,
marked from the other side: `pure` on functions that have no effects (idea
19), not `IO` on those that do. Past that lies a general account of what a
function may do: touch files, the network, module state; panic (idea 18);
start tasks. Ideas 18 and 19 are two instances of it. If both are promoted
they should be one mechanism with two properties, not two unrelated markers.

**Impact on the language.** In the general form every signature, function
type, and trait member carries an effect, and generic code must be able to
abstract over it. That is D2's deferred work under another name. The
proposal here is only the ordering: decide whether "total" and "pure" are the
first two rows of an effect system before either gets its own keyword.

---

## Surveyed and not proposed

| Idris concept | Why not |
|---------------|---------|
| User-defined operators and fixity declarations ([Data types][i-data]) | D244: operators stay built in |
| Arithmetic and literals through interfaces (`Num`, `fromInteger`) | D244 for arithmetic. Literals already take the expected type (refined, `Float64`, `UInt64`) |
| A type declaration on every function | Return types are inferred (D129); `->` is the contract when written |
| [`mutual` blocks][i-mutual], definition before use | Declarations are order-free already |
| `Functor`, `Applicative`, `Monad`; [`do`][i-monads], [`!`][i-bang], [idiom brackets][i-idiom], [monad comprehensions][i-monad-comprehensions] | These need type parameters that are themselves generic, and closures. `?` with marks covers early exit, the case that matters, and tasks need no `await`. Ideas 9 and 10 take the two useful notations without the machinery |
| [Named implementations][i-named] | One conformance per type keeps a `Map<K, V>`'s hashing and an `Array<T>`'s order unambiguous. A second ordering is a wrapper type, or a function passed to a sort |
| [Interface constructors][i-interface-constructors], [quantities for parameters][i-interface-quantities] | Artifacts of passing implementations as values; Thoxa compiles each instance |
| [Determining parameters][i-determining] | Associated types |
| [Pattern matching on types][i-pmtypes] | A generic body sees its parameter only through bounds (D232), so a generic function cannot behave differently per type. `match` on a `mixed` value is the run-time form, where asked for |
| [Erasure][i-erasure] as something the user writes | Monomorphization erases types; see idea 21 for numbers |
| Equality proofs, `rewrite`, `Dec`, universes ([Theorem proving][i-theorems], [Cumulativity][i-cumulativity]) | Proof terms. Thoxa's answer is a checked conversion (`as`) plus facts from conditions (idea 17) |
| [Views and the `with` rule][i-with] | `match parity(count) { Even(half) => … Odd(half) => … }` over a function that returns a union is the same program without the index refinement, and a `from` clause makes `count as Parity` the view. Expressible today |
| [Infinite data][i-inf], `Stream` | `Iterator` already gives lazy and unbounded sequences |
| List syntax for any type with `Nil` and `::` ([`List` and `Vect`][i-list]) | `from(items: Array<T>)` with `as` is the explicit form; implicit conversion is kept for `Json` (D191) |
| [`parameters` blocks][i-parameters] | A type with methods: its fields are the shared parameters |
| [Explicit namespaces][i-namespaces] for overloading | Multi-clause functions |
| [`import public`][i-reexport] (re-export) | There is no re-export form (D139 declined one for `std/http`), and D247 wants a name's origin visible in the importer |
| [Default][i-default-implicit] and [implicit][i-implicit] arguments | Parameter defaults; inferred type arguments |
| [Packages][i-packages], the REPL | No package manager yet; the shell and session were removed (D227) |

## If something is promoted

The ideas depend on each other as follows.

- Idea 1, then 2.
- Ideas 3, 4, 5, 6, 9, 11, 13 stand alone.
- Idea 7, then the parked generic codecs.
- Idea 14, then 12 pays off most, then 22.
- Idea 17, then 18 and 20; idea 19 needs 18.
- Ideas 8 and 9 lower the pressure for 15 and do not replace it.
- Idea 5, then 23, which should not start before its generic story is
  written.

By gain against cost, the first four would be 3 (private constructors), 5
(results that must be used), 1 (typed holes), and 17 (flow typing), with 14
(recursive types) next because self-hosting will need it. Of the fundamental
ideas, 20 is the one that fits what Thoxa already is: refinements are in the
language, value semantics makes the facts stable, and `as` supplies the
run-time check where a proof would otherwise be required.

## Sources

The tutorial's chapters, all under
`https://idris2.readthedocs.io/en/latest/tutorial/`:
[Introduction][i-intro], [Types and Functions][i-typesfuns],
[Interfaces][i-interfaces], [Modules and Namespaces][i-modules],
[Multiplicities][i-multiplicities], [Packages][i-packages], [The Well-Typed
Interpreter][i-interp], [Views and the `with` rule][i-with], [Theorem
Proving][i-theorems], [Interactive Editing][i-interactive],
[Miscellany][i-miscellany]. The chapter on multiplicities points to [Idris 2:
Quantitative Type Theory in
Action](https://www.type-driven.org.uk/edwinb/idris-2-quantitative-type-theory-in-action.html)
for the theory behind idea 23.

[i-intro]: https://idris2.readthedocs.io/en/latest/tutorial/introduction.html
[i-typesfuns]: https://idris2.readthedocs.io/en/latest/tutorial/typesfuns.html
[i-data]: https://idris2.readthedocs.io/en/latest/tutorial/typesfuns.html#data-types
[i-where]: https://idris2.readthedocs.io/en/latest/tutorial/typesfuns.html#where-clauses
[i-covering]: https://idris2.readthedocs.io/en/latest/tutorial/typesfuns.html#totality-and-covering
[i-holes]: https://idris2.readthedocs.io/en/latest/tutorial/typesfuns.html#holes
[i-fctypes]: https://idris2.readthedocs.io/en/latest/tutorial/typesfuns.html#first-class-types
[i-vect]: https://idris2.readthedocs.io/en/latest/tutorial/typesfuns.html#vectors
[i-fin]: https://idris2.readthedocs.io/en/latest/tutorial/typesfuns.html#the-finite-sets
[i-implicit]: https://idris2.readthedocs.io/en/latest/tutorial/typesfuns.html#implicit-arguments
[i-mutual]: https://idris2.readthedocs.io/en/latest/tutorial/typesfuns.html#note-declaration-order-and-mutual-blocks
[i-io]: https://idris2.readthedocs.io/en/latest/tutorial/typesfuns.html#i-o
[i-lazy]: https://idris2.readthedocs.io/en/latest/tutorial/typesfuns.html#laziness
[i-inf]: https://idris2.readthedocs.io/en/latest/tutorial/typesfuns.html#infinite-data-types
[i-list]: https://idris2.readthedocs.io/en/latest/tutorial/typesfuns.html#list-and-vect
[i-lambda]: https://idris2.readthedocs.io/en/latest/tutorial/typesfuns.html#aside-anonymous-functions-and-operator-sections
[i-tuples]: https://idris2.readthedocs.io/en/latest/tutorial/typesfuns.html#tuples
[i-dpair]: https://idris2.readthedocs.io/en/latest/tutorial/typesfuns.html#dependent-pairs
[i-records]: https://idris2.readthedocs.io/en/latest/tutorial/typesfuns.html#records
[i-record-update]: https://idris2.readthedocs.io/en/latest/tutorial/typesfuns.html#nested-record-update
[i-dependent-records]: https://idris2.readthedocs.io/en/latest/tutorial/typesfuns.html#dependent-records
[i-let]: https://idris2.readthedocs.io/en/latest/tutorial/typesfuns.html#let-bindings
[i-comprehensions]: https://idris2.readthedocs.io/en/latest/tutorial/typesfuns.html#list-comprehensions
[i-totality]: https://idris2.readthedocs.io/en/latest/tutorial/typesfuns.html#totality
[i-interfaces]: https://idris2.readthedocs.io/en/latest/tutorial/interfaces.html
[i-defaults]: https://idris2.readthedocs.io/en/latest/tutorial/interfaces.html#default-definitions
[i-extending]: https://idris2.readthedocs.io/en/latest/tutorial/interfaces.html#extending-interfaces
[i-interface-quantities]: https://idris2.readthedocs.io/en/latest/tutorial/interfaces.html#quantities-for-parameters
[i-functors]: https://idris2.readthedocs.io/en/latest/tutorial/interfaces.html#functors-and-applicatives
[i-monads]: https://idris2.readthedocs.io/en/latest/tutorial/interfaces.html#monads-and-do-notation
[i-pattern-bind]: https://idris2.readthedocs.io/en/latest/tutorial/interfaces.html#pattern-matching-bind
[i-bang]: https://idris2.readthedocs.io/en/latest/tutorial/interfaces.html#notation
[i-monad-comprehensions]: https://idris2.readthedocs.io/en/latest/tutorial/interfaces.html#monad-comprehensions
[i-idiom]: https://idris2.readthedocs.io/en/latest/tutorial/interfaces.html#idiom-brackets
[i-named]: https://idris2.readthedocs.io/en/latest/tutorial/interfaces.html#named-implementations
[i-interface-constructors]: https://idris2.readthedocs.io/en/latest/tutorial/interfaces.html#interface-constructors
[i-determining]: https://idris2.readthedocs.io/en/latest/tutorial/interfaces.html#determining-parameters
[i-modules]: https://idris2.readthedocs.io/en/latest/tutorial/modules.html
[i-export]: https://idris2.readthedocs.io/en/latest/tutorial/modules.html#export-modifiers
[i-export-fn]: https://idris2.readthedocs.io/en/latest/tutorial/modules.html#meaning-for-functions
[i-export-data]: https://idris2.readthedocs.io/en/latest/tutorial/modules.html#meaning-for-data-types
[i-reexport]: https://idris2.readthedocs.io/en/latest/tutorial/modules.html#propagating-inner-module-api-s
[i-namespaces]: https://idris2.readthedocs.io/en/latest/tutorial/modules.html#explicit-namespaces
[i-parameters]: https://idris2.readthedocs.io/en/latest/tutorial/modules.html#parameterised-blocks-parameters-blocks
[i-multiplicities]: https://idris2.readthedocs.io/en/latest/tutorial/multiplicities.html
[i-linearity]: https://idris2.readthedocs.io/en/latest/tutorial/multiplicities.html#linearity
[i-protocols]: https://idris2.readthedocs.io/en/latest/tutorial/multiplicities.html#resource-protocols
[i-erasure]: https://idris2.readthedocs.io/en/latest/tutorial/multiplicities.html#erasure
[i-pmtypes]: https://idris2.readthedocs.io/en/latest/tutorial/multiplicities.html#pattern-matching-on-types
[i-packages]: https://idris2.readthedocs.io/en/latest/tutorial/packages.html
[i-interp]: https://idris2.readthedocs.io/en/latest/tutorial/interp.html
[i-interp-lang]: https://idris2.readthedocs.io/en/latest/tutorial/interp.html#representing-languages
[i-interp-write]: https://idris2.readthedocs.io/en/latest/tutorial/interp.html#writing-the-interpreter
[i-cast]: https://idris2.readthedocs.io/en/latest/tutorial/interp.html#aside-cast
[i-with]: https://idris2.readthedocs.io/en/latest/tutorial/views.html#the-with-rule-matching-intermediate-values
[i-theorems]: https://idris2.readthedocs.io/en/latest/tutorial/theorems.html
[i-equality]: https://idris2.readthedocs.io/en/latest/tutorial/theorems.html#equality
[i-empty]: https://idris2.readthedocs.io/en/latest/tutorial/theorems.html#the-empty-type
[i-practice]: https://idris2.readthedocs.io/en/latest/tutorial/theorems.html#theorems-in-practice
[i-totality-checking]: https://idris2.readthedocs.io/en/latest/tutorial/theorems.html#totality-checking
[i-totality-hints]: https://idris2.readthedocs.io/en/latest/tutorial/theorems.html#hints-for-totality
[i-interactive]: https://idris2.readthedocs.io/en/latest/tutorial/interactive.html
[i-addclause]: https://idris2.readthedocs.io/en/latest/tutorial/interactive.html#addclause
[i-casesplit]: https://idris2.readthedocs.io/en/latest/tutorial/interactive.html#casesplit
[i-addmissing]: https://idris2.readthedocs.io/en/latest/tutorial/interactive.html#addmissing
[i-proofsearch]: https://idris2.readthedocs.io/en/latest/tutorial/interactive.html#proofsearch
[i-miscellany]: https://idris2.readthedocs.io/en/latest/tutorial/miscellany.html
[i-auto]: https://idris2.readthedocs.io/en/latest/tutorial/miscellany.html#auto-implicit-arguments
[i-default-implicit]: https://idris2.readthedocs.io/en/latest/tutorial/miscellany.html#default-implicit-arguments
[i-literate]: https://idris2.readthedocs.io/en/latest/tutorial/miscellany.html#literate-programming
[i-cumulativity]: https://idris2.readthedocs.io/en/latest/tutorial/miscellany.html#cumulativity
