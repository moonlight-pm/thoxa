# Thoxa × Kilo — Stage 0: the `Terminal` module

- **Date:** 2026-07-16
- **Status:** Approved design (Stage 0 of the kilo-port roadmap
  `2026-07-16-kilo-port-roadmap.md`). Follows LF-A (bitwise operators) and LF-B
  (typed C-struct FFI), both shipped 2026-07-16.

## 1. Context & goal

Stage 0 delivers `examples/kilo/Terminal.thx` — the first kilo module — wrapping the
Linux `termios`/`ioctl` boundary behind a semantic, `Trial`-returning API, exactly
as `examples/http`'s `Net`/`Io` wrap sockets and files (private raw externs + magic
numbers → a wrapper *type* whose methods lift `-1`/`errno` into `Trial<T, String>`).
It provides three capabilities kilo's `enableRawMode`/`disableRawMode`/`getWindowSize`
need:

- `RawMode.enable()` — switch stdin to raw mode (the kilo flag dance).
- `RawMode.restore()` — put the saved settings back (idempotent).
- `windowSize()` — the terminal size in character cells, via `TIOCGWINSZ`.

## 2. Scope finding — Stage 0 forces no new language feature

LF-A and LF-B already ship every primitive raw mode needs, so Stage 0 is **pure
Thoxa authoring** (the point of the port: the demo is the forcing function, and here
the forcing already happened in LF-A/LF-B). The evidence, checked against the tree:

- **The C structs are ABI structs.** `ffi.abi_struct_specs` classifies a `type` as an
  ABI struct iff it has *no ctor params, no methods, ≥1 field, and every field is a
  fixed-width scalar or `Array<scalar, N>`*. `Termios` and `Winsize` qualify → each is
  backed by a synthesized `ctypes.Structure` with the exact native layout. `RawMode`
  has methods → it is **never** ABI-classified, so it stays an ordinary dict-backed
  wrapper (the `Socket`/`File` shape). No marker, no `extern type` — the markerless
  decision from LF-B carries this for free.
- **The pointer boundary exists.** `tcgetattr`/`tcsetattr`/`ioctl` take
  `Address<Termios>` / `Address<Winsize>`; under implicit addressing the struct is
  passed directly and coerces. `MARSHALABLE_PARAMS` already includes `Address`.
- **The flag edits are LF-A.** `& | ^ ~ << >>` on integer struct fields, with a
  width-truncating store — LF-B made a struct-field write exactly such a boundary.
- **The `mut`-at-boundary rule is deferred.** LF-B *described* "coercing a struct into
  an `Address<T>` parameter requires a `mut` place" but **deferred the enforcement**
  (the named "read/write safety later pass"). There is no call-site `mut` check in the
  checker or interp today. So `RawMode.enable()` passing its own `mut orig` field to
  `tcgetattr` is accepted — no gap to hit.

The only combinations LF-B's own tests did not exercise are benign and expected to
work: a `CStruct` value held in a **field** of a dict-backed type (marshalling reads
the value regardless of where it is bound), a **paramless** wrapper type with body
state constructed via `RawMode()`, and mutation-through-parameter for a `CStruct`
(LF-B adds no value-copy, so `rawFlags(raw)` mutates the caller's struct). If any of
these surfaces a real gap during implementation, that becomes a small language
increment per the port's discipline — but clean authoring is expected.

## 3. Module surface — `examples/kilo/Terminal.thx`

### 3.1 Private FFI layer (hidden behind the API, `Net`/`Io` precedent)

```thoxa
import { errno, strerror } from "ffi"

# termios/ioctl constants (Linux x86-64), named for documentation, kept private.
let echo=0x8   let icanon=0x2   let isig=0x1    let iexten=0x8000
let ixon=0x400 let icrnl=0x100  let brkint=0x2  let inpck=0x10  let istrip=0x20
let opost=0x1  let cs8=0x30
let vmin=6     let vtime=5      let tcsaflush=2 let tiocgwinsz=0x5413
let stdinFileno=0  let stdoutFileno=1
```

`struct termios` (Linux x86-64) — **must be the full 60-byte layout**. The kernel
writes `sizeof(struct termios)` bytes through the pointer, so a short struct would
corrupt memory past the backing block. ctypes computes the 3 tail-pad bytes between
`cc[32]` (offset 17..48) and `ispeed` (offset 52) for free (verified in LF-B:
`struct termios` = 60 bytes, fields at 0/4/8/12/16/17/52/56). Every field is `mut`
(C writes them; `rawFlags` edits the flags; `let`-without-initializer is not needed):

```thoxa
type Termios {
    mut iflag: UInt32     mut oflag: UInt32   mut cflag: UInt32   mut lflag: UInt32
    mut line:  Byte       mut cc: Array<Byte, 32>
    mut ispeed: UInt32    mut ospeed: UInt32
}
```

`struct winsize` (4×`UInt16` = exactly 8 bytes):

```thoxa
type Winsize { mut rows: UInt16  mut cols: UInt16  mut x: UInt16  mut y: UInt16 }
```

The raw syscalls (the FFI boundary, `sys`-prefixed like `Net`/`Io`). `ioctl` is
variadic in C (`int ioctl(int, unsigned long, ...)`); a fixed 3-arg declaration is
correct because ctypes pushes the pointer argument positionally:

```thoxa
extern fn sysTcgetattr(fd!: Int32, p!: Address<Termios>) -> Int32 from "c" "tcgetattr"
extern fn sysTcsetattr(fd!: Int32, act!: Int32, p!: Address<Termios>) -> Int32 from "c" "tcsetattr"
extern fn sysIoctl(fd!: Int32, request!: UInt64, ws!: Address<Winsize>) -> Int32 from "c" "ioctl"

fn termErr(what!: String) -> String { "{what}: {strerror(errno())}" }
```

The raw-mode flag transformation, factored into a named helper (a hand-rolled
`cfmakeraw`) so the deterministic conformance slice can pin the exact bitmasks it
produces. It mutates the passed struct in place — the same struct the caller holds,
since a `CStruct` is not copied on a call:

```thoxa
fn rawFlags(raw!: Termios) -> Void {
    raw.iflag = raw.iflag & ~(brkint | icrnl | inpck | istrip | ixon)
    raw.oflag = raw.oflag & ~opost
    raw.cflag = raw.cflag | cs8
    raw.lflag = raw.lflag & ~(echo | icanon | iexten | isig)
    raw.cc[vmin] = 0
    raw.cc[vtime] = 1
    Void
}
```

### 3.2 Public semantic API

```thoxa
## The terminal window size in character cells. A public *immutable data record* — new to
## Thoxa: a ctor-param field is private and an all-scalar body-field record would be an ABI
## struct (zero-init only), so the ctor params project into `export let` fields. Callers
## read `size.rows`/`size.cols` directly, the value stays immutable, and construction is one
## call. (This resolves the design-time sketch `WindowSize(rows!, cols!)`, whose ctor-param
## fields would not have been externally readable.)
export type WindowSize(r!: Int32, c!: Int32) {
    export let rows = r
    export let cols = c
}

## Owns the terminal's original settings. `enable()` switches stdin to raw mode;
## `restore()` puts the saved settings back. `restore()` is idempotent — a no-op
## before `enable()` or after a prior restore — so every exit path can call it.
export type RawMode() {
    mut orig = Termios()
    mut active = false

    export fn enable() -> Trial<Void, String> {
        if sysTcgetattr(stdinFileno, orig) < 0 { return Fail(termErr("tcgetattr")) }
        mut raw = Termios()
        sysTcgetattr(stdinFileno, raw)          # 2nd read == working copy (no struct-copy primitive yet)
        rawFlags(raw)
        if sysTcsetattr(stdinFileno, tcsaflush, raw) < 0 { return Fail(termErr("tcsetattr")) }
        active = true
        Pass(Void)
    }

    export fn restore() -> Trial<Void, String> {
        if not active { return Pass(Void) }
        if sysTcsetattr(stdinFileno, tcsaflush, orig) < 0 { return Fail(termErr("tcsetattr")) }
        active = false
        Pass(Void)
    }
}

## Query the window size via `ioctl(TIOCGWINSZ)`. Fails cleanly when unavailable
## (e.g. stdout is not a tty, or a terminal that reports 0 columns); the
## cursor-probe fallback is deferred to after Keys (Stage 1).
export fn windowSize() -> Trial<WindowSize, String> {
    mut ws = Winsize()
    if sysIoctl(stdoutFileno, tiocgwinsz, ws) < 0 {
        Fail(termErr("ioctl TIOCGWINSZ"))
    } else if ws.cols == 0 {
        Fail("ioctl TIOCGWINSZ: zero columns")
    } else {
        Pass(WindowSize(ws.rows, ws.cols))
    }
}
```

## 4. Design decisions

1. **API shape — a `RawMode` type** (owns `orig`), not free functions. This follows
   the established Thoxa wrapper pattern (`Socket`/`File`): private state reached only
   through methods, TypeScript-simple to call (`RawMode()` then `.enable()`),
   Rust-robust in that a caller cannot mishandle the saved settings. (The LF-B design
   §7 sketched free functions returning `orig`; the established pattern supersedes the
   sketch.)
2. **Window size — a real `Winsize` C struct** (LF-B), not the `Bytes(8)` hand-unpack.
   A clean second use of LF-B: type-safe `ws.rows`/`ws.cols`, no manual little-endian
   math, and it exercises a 4×`UInt16` ABI struct (widths LF-B's conformance did not
   otherwise cover).
3. **Cursor-probe fallback — deferred.** When `ioctl` fails, return a `Fail`. kilo's
   fallback writes cursor-move escapes and parses the position report back, which
   needs byte-by-byte reads + escape parsing — that machinery is Stage 1 (Keys).
4. **Verification — deterministic conformance slice + manual smoke** (see §7).
5. **Field names drop C's `c_` prefix** (`iflag`/`oflag`/`cflag`/`lflag`/`cc`/`line`/
   `ispeed`/`ospeed`) for readability; the struct is private so the names are local.
6. **`Main.thx` is the smoke driver** (not a throwaway `*_smoke.thx`), seeding the
   editor loop that later stages grow — the `examples/http` `Main.thx` precedent.

## 5. `examples/kilo/Main.thx` — a minimal driver

Seeds the editor entry point. Stage 0 behaviour: print the window size; enable raw
mode; read keys in raw mode until `q`, echoing each byte's decimal code; restore on
every exit path. The echoed codes are the interactive proof that raw mode took —
ECHO off (keys do not appear unless echoed), ICANON off (bytes arrive without Enter),
ISIG off (Ctrl-C arrives as code `3` rather than killing the process).

The one-byte read uses a **clearly-marked throwaway** `read` extern; Stage 1's
`Keys.readKey()` replaces it with real VT100 decoding. Output produced while in raw
mode is written with an explicit `\r\n` since `OPOST` is off. The pty smoke test (§7)
caught a real defect here: with `VMIN=0`/`VTIME=1` a read returns `0` on a 100ms idle,
which is a **timeout, not EOF**, so `readByte` must retry (as kilo does) rather than
quit on a pause — the demo-as-forcing-function surfacing a bug before it shipped.

## 6. Layout & ABI notes

- **`Termios` = 60 bytes, exact native layout.** Non-negotiable: `tcgetattr`/
  `tcsetattr` read/write the full struct through the pointer. All eight fields plus
  the ctypes-computed pad are required; omitting `ispeed`/`ospeed` would let the
  kernel write past the block.
- **`Winsize` = 8 bytes.** 4×`UInt16`, matching `struct winsize` (`ws_row`, `ws_col`,
  `ws_xpixel`, `ws_ypixel`).
- **Native endianness** (ctypes host order) — correct for termios/ioctl.
- **Width-truncating field stores.** `raw.lflag & ~mask` stores its low 32 bits; the
  masks only ever clear bits, so every result is a small positive value that stores
  exactly. Array-element writes (`cc[vmin]`) are single-`Byte`, bounds-checked to `N`.

## 7. Verification

Joshua's chosen gate: a deterministic conformance slice plus a manual live smoke run.

1. **`spec/conformance/kilo_terminal.thx` + `.out`** — self-contained (the conformance
   harness runs each file standalone and cannot import an example module, so it
   re-declares `Termios`/`Winsize` + the flag constants + `rawFlags`, mirroring the
   module). It pins: struct field write/read for `UInt32`/`UInt16`/`Byte`;
   `Array<Byte, 32>` element writes at the `VMIN=6`/`VTIME=5` indices; the exact
   results of all four raw bitmask transformations (proving the masks clear/set
   exactly the intended bits and preserve the rest); and a `Winsize` field round-trip
   incl. a `UInt16` width-truncating store. `.out` is generated from a verified run,
   not hand-computed. This rides the existing `test_conformance` (stdout==`.out`),
   `test_checker` (type-checks clean), and `test_lsp` (no diagnostics) gates.
2. **`thoxa check examples/kilo/Terminal.thx` clean** — the real module type-checks.
3. **Automated raw-mode smoke (pty)** — `tests/test_kilo_terminal.py` also drives the
   real `Main.thx` through a **pseudo-terminal**, feeding `a`, an arrow-up escape, Ctrl-C,
   and `q`, and asserting the window size, that each byte arrives immediately (arrow =
   `27 91 65`), that Ctrl-C is byte `3` (ISIG off, process survives), that `q` quits with
   no Enter, and that the terminal restores — the success path a plain non-tty run can't
   reach. Plus deterministic Python tests: the struct classification, the **60-byte**
   `Termios` layout (the memory-safety property), the off-tty `Fail` paths, and idempotent
   `restore()`. A human can still `thoxa run examples/kilo` for a live interactive check.

## 8. `SPEC.md`

No change. Stage 0 adds no language feature; the C-struct-FFI, `Address<T>`, and
bitwise sections that `SPEC.md` already carries (from LF-A/LF-B) cover every construct
this module uses.

## 9. Out of scope / deferred

- **Cursor-probe window-size fallback** — needs raw byte reads + escape parsing
  (Stage 1, Keys).
- **`readKey()` / VT100 decoding** — Stage 1. Stage 0's `Main` read is throwaway
  scaffolding.
- **`SIGWINCH` / `atexit`** — sidestepped by the roadmap: poll size each refresh,
  restore explicitly on every exit path (no C→Thoxa callback FFI).
- **Struct value-copy** — kilo's `raw = orig_termios` is reproduced by a second
  `tcgetattr`; a copy primitive waits for a real forcing use (LF-B §9).
- **`mut`-at-boundary enforcement / const pointers** — the LF-B "later pass"; Stage 0
  neither needs nor adds it.
