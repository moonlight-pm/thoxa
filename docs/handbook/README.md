# Thoxa manual

The manual for Thoxa as it is implemented today. The site renders these files,
in this order, as one page at [thoxa.dev/manual](https://thoxa.dev/manual).

| Chapter | Contents |
|---------|----------|
| [Introduction](introduction.md) | What Thoxa is, status, building from source, a first program |
| [Language](language.md) | Syntax, types, expressions, functions, modules, concurrency, typed data, C interop |
| [Standard library](std.md) | The `std/…` modules |
| [Session](session.md) | The interactive shell and session scripts |
| [Command line](cli.md) | `thoxa` commands, diagnostics, exit status, environment |
| [Editor](editor.md) | Zed support and `thoxa lsp` |

Engineering status and plans are elsewhere:
[`../capabilities.md`](../capabilities.md) (what exists, with gaps),
[`../roadmap.md`](../roadmap.md), and [`../../CURRENT.md`](../../CURRENT.md).

## Rules for this manual

- Document shipped behaviour only. A feature belongs here when a conformance
  test exercises it. A partial feature is described with its limits, or left
  out.
- Never describe plans as facts. "Will" does not appear here.
- Write for people using the language: plain, exact, upfront about limits, no
  promotional language. State each rule once, in the section where a reader
  would look for it, and follow it with a short example.
- Decision ids (`D182`) belong in [`../capabilities.md`](../capabilities.md)
  and [`../decisions.md`](../decisions.md), not in this text.
- Every complete program in the manual must compile and run as shown.
- The site's renderer supports a Markdown subset; stay inside it:
  - one `#` heading per file (the chapter title, unnumbered); `##` and `###`
    for sections, `####` sparingly;
  - paragraphs, `- ` and `1. ` lists (continuation lines indented to the item's
    text), fenced code marked `thx`, `bash`, or `text`;
  - pipe tables with a header row and a `|---|` separator; `\|` for a literal
    pipe inside a cell;
  - inline `code`, **bold**, *italic* (asterisks), and `[text](target)` links;
  - `> ` blockquotes;
  - no raw HTML, images, footnotes, reference links, setext headings, or
    horizontal rules.
- Link another chapter as `std.md#section-slug`, the same chapter as `#slug`,
  and anything outside the manual by a relative path (`../decisions.md`); the
  site turns those into links to the repository.
