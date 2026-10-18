# Freeze: String methods and Unicode text (D195)

**Date:** 2026-10-17
**Status:** target (freeze)
**Implementation:** shipped (§1–§3 slice 1, §4 slice 2); since D196 the methods are Thoxa in `std/string.thx`, not C
**Decision:** D195 (needs D194, positional-then-named calls)
**Gates:** `string_methods`, `string_scalars`, `string_escapes`, `stdlib`, `call_mixed_args`, `fail/call_positional_after_named`, `fail/call_named_twice`
**Gaps:** final sigma (`Σ` lowercases to `σ` everywhere); no normalization; no `\u{…}` escape; a C string that is not UTF-8 (a Latin-1 file name) is repaired, so it cannot round-trip; positions in a long non-ASCII string are O(n)
**As-built:** [`../capabilities.md`](../capabilities.md) · [`../architecture.md`](../architecture.md)
**Pickup:** root [`CURRENT.md`](../../CURRENT.md) · this freeze · D195 in
[`decisions.md`](../decisions.md)

---

## Intent

A String is UTF-8, end to end. Until now the string toolkit was free
functions in `std/str`, written in Thoxa as loops over `Bytes`, and anything
involving case worked on ASCII only (`containsIgnoreCaseAscii`). The runtime
had no Unicode data, and nothing in it decoded UTF-8. Measured before this
freeze:

| Call | Result |
|------|--------|
| `containsIgnoreCaseAscii("CAFÉ AU LAIT", "café")` | `false` |
| `containsIgnoreCaseAscii("Straße", "STRASSE")` | `false` |
| `truncate("café", 4)` | `caf\xC3`, not UTF-8 |
| `reverse("café")` | `\xA9\xC3fac`, not UTF-8 |
| `padLeft("é", 3)` | one space, not two |

This freeze moves the toolkit onto String as methods implemented in the C
runtime, and makes case and white space Unicode. It does it in two slices:
slice 1 takes everything that does not count or index (§1–§3); slice 2 settles
what a String index counts and moves the rest (§4).

```thx
fn drinkMatches(drink!: Drink, query!: String) -> Bool {
    drink.name.contains(query, ignoreCase: true) or
        drink.notes.contains(query, ignoreCase: true) or
        drink.slug.contains(query, ignoreCase: true)
}

"Straße".equals("STRASSE", ignoreCase: true)   ## true
"Straße".toUpper()                             ## "STRASSE"
"\tcafé\n".trim()                              ## "café"
["été", "hiver"].join(" · ")                   ## "été · hiver"
```

## §1 Methods on String (slice 1)

As first shipped, each method was a call into `thoxa_rt` behind a table in
the compiler. Since **D196** they are Thoxa, in `std/string.thx`, attached
with `extend String as text { … }` and built on three runtime byte
primitives ([`2026-10-17-std-extend.md`](2026-10-17-std-extend.md)).

| Method | Result |
|--------|--------|
| `contains(needle, ignoreCase: Bool = false)` | `Bool` |
| `startsWith(prefix, ignoreCase: Bool = false)` | `Bool` |
| `endsWith(suffix, ignoreCase: Bool = false)` | `Bool` |
| `equals(other, ignoreCase: Bool = false)` | `Bool`; without `ignoreCase` it is `==` |
| `isEmpty()`, `isAscii()` | `Bool` |
| `toLower()`, `toUpper()` | `String` |
| `trim()`, `trimStart()`, `trimEnd()` | `String` |
| `removePrefix(prefix)`, `removeSuffix(suffix)` | `String`, unchanged when absent |
| `replace(old, new)`, `replaceFirst(old, new)` | `String`; every / the first non-overlapping `old`; an empty `old` changes nothing |
| `repeat(count)` | `String`; `count <= 0` is `""` |
| `split(separator)` | `Array<String>`; keeps empty pieces; an empty separator gives `[s]` |
| `lines()` | `Array<String>`; splits on `\n` and drops one trailing `\r` per line |

On `Array<String>`: `join(separator) -> String`.

A refined String (`type Name = String where >= 1`) takes the same methods,
unless it declares a method of the same name. `ignoreCase` is a labelled
parameter, so the call is `s.contains(x, ignoreCase: true)`: positional
arguments first, then named (D194, the oracle SPEC's call rule).

## §2 Unicode semantics (slice 1)

- **Exact operations compare bytes.** `contains`, `startsWith`, `endsWith`,
  `split`, `replace`, and `removePrefix` / `removeSuffix` are exact on UTF-8:
  a match of valid UTF-8 cannot begin or end inside a character.
- **`ignoreCase: true` compares full case folds** (Unicode `CaseFolding.txt`,
  statuses C and F): `Straße` = `STRASSE`, `CAFÉ` contains `café`, `ΣΊΣΥΦΟΣ`
  contains `σίσυφος`. Both sides are folded and then compared as bytes, so a
  prefix may match part of an expansion (`"ß".startsWith("s", ignoreCase:
  true)` is true).
- **`toLower` / `toUpper` use the full default mappings** (`UnicodeData.txt`
  plus the unconditional entries of `SpecialCasing.txt`): `ß` → `SS`,
  `İ` → `i̇`, `ǅ` → `Ǆ` / `ǆ`. Conditional mappings are left out: final sigma,
  and the Turkish, Azeri, and Lithuanian rules.
- **`trim` removes Unicode White_Space**: ASCII space, tab, and line ends,
  plus U+0085, U+00A0, U+1680, U+2000–U+200A, U+2028, U+2029, U+202F, U+205F,
  U+3000.
- **Every String is valid UTF-8** (§4), so the operations above never meet a
  malformed byte.
- **Not normalization.** Case-insensitive is not normalization-insensitive: a
  precomposed `é` (U+00E9) does not match `e` + U+0301.

## §3 The case tables (slice 1)

`std/unicode.thx` (first `runtime/src/thoxa_unicode.h`, before D196) is
generated by `cargo make unicode <ucd-dir>` (`crates/make/src/unicode.rs`)
from `UnicodeData.txt`, `SpecialCasing.txt`, and `CaseFolding.txt` of one
Unicode version, downloaded from `https://www.unicode.org/Public/<version>/ucd/`;
it names the version (`unicodeVersion`). As shipped: Unicode 17.0.0. Three
tables (lower, upper, fold), each a sorted list of one-to-one ranges
(`first..=last` every `stride` code points moves by `delta`) plus a short
list of multi-code-point expansions, as flat `Array<Int64>`s that
std/string searches by binary search.

Checked when generated: every code point's three mappings agreed with
Python's `str.lower` / `upper` / `casefold` (Unicode 15.1) except four
lowercase letters whose capitals arrived in 16.0 and 17.0.

After slice 1, `std/str` kept only what counts or cuts by bytes; slice 2 (§4)
moved that onto String too, and D196 replaced `std/str` with `std/string`.
The removed functions have no compatibility names; every caller moved.

## §4 What a String counts (slice 2)

**Joshua, 2026-10-17: Unicode scalars.** `.count`, and every method that
takes or returns a position, count Unicode scalar values (code points), as
the oracle SPEC's `Char` is a scalar.

- **`.count` is scalars, `.byteCount` bytes** (`"café"`: 4 and 5), on String
  and on refined Strings. A `where` length test on a String counts scalars at
  run time, as the compile-time check of a literal already did.
- **Position methods** (positions clamp to `0..count`):

  | Method | Result |
  |--------|--------|
  | `indexOf(needle)`, `lastIndexOf(needle)` | `May<Int64>`; an empty needle is at 0 / at `count` |
  | `substring(start, end)` | `String`; `""` when `start >= end` |
  | `truncate(max)` | the first `max` scalars |
  | `padLeft(width, pad: String = " ")`, `padRight(…)` | `pad` repeated, its last copy cut, to `width` scalars; an empty `pad` is a space |
  | `reverse()` | scalars in reverse order (a combining mark moves before its base) |

- **Every String is valid UTF-8.** `thoxa_string_new`, which every String
  goes through (`Bytes.toString()`, C strings from `extern` calls, the
  runtime's own builders), replaces each maximal ill-formed subsequence with
  U+FFFD, as the Unicode Standard (§3.9) and WHATWG decoders do. So
  `Bytes.toString()` still cannot fail; code that must reject bad input checks
  the bytes itself.
- **`splitOnce`** stayed a `std/str` function at first, because a method in C
  could not return the std type `Split`. That is what led to D196: it is now a
  String method in `std/string`, beside `Split` and the function `parseUInt`.
  `length` (now `.count`), `count(s, needle)`, and `byteAt` were removed
  without replacement (`byteAt` is now a String primitive on byte offsets).
- **A built-in `May<Int64>`** (`String.indexOf`, `Bytes.find`) needs
  `std/fallible` in the program, not `May` imported by name.
- **The audit** of every `.count` on a String (each lowered in every program,
  std included) found one that meant bytes, a test of a literal's encoding;
  wire sizes such as Content-Length were already computed on `Bytes`.

**Cost, and the revisit (Joshua, 2026-10-17):** "thoxa should be performant
and that O(n) bothers me a little." As built:

- Every String stores its scalar count beside its byte length, computed once
  while the bytes are validated (an 8-bytes-at-a-time ASCII path), so `.count`
  is O(1) and concatenation adds the two counts.
- In an all-ASCII String (count = bytes) a position is a byte offset: O(1).
- Otherwise a position walks the UTF-8 from the start: O(n). (The C version
  walked from the nearer end; the Thoxa one in D196 does not yet.) Code that
  indexes a long non-ASCII string in a loop should work in bytes
  (`byteAt`, `byteSlice`, `byteIndexOf`).

Still open: an index of positions kept on long non-ASCII strings, or
reopening the unit itself (bytes with boundary checks, as Rust and Go do), if
real programs show the walk.

## Non-goals

- Grapheme clusters (`👍🏽` counts as two scalars), display width.
- Locale-dependent case (Turkish `i`), collation, `<` on strings.
- Normalization (NFC / NFD).
- A general way to add methods to built-in types.
