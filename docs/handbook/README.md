# Thoxa handbook

User-facing documentation for **native Thoxa as it works today**.

This is the product manual: shipped behavior only. Engineering progress and
roadmap live under [`../capabilities.md`](../capabilities.md),
[`../roadmap.md`](../roadmap.md), and root
[`../../CURRENT.md`](../../CURRENT.md).

> **Pre-alpha.** Surface grows when conformance tests land. Oracle SPEC under
> `apocrypha/old/ThoxaPython` is a reference, not a promise.

## Contents

| Page | Topic |
|------|--------|
| [CLI](cli.md) | Commands, build products, common workflows |
| [Language](language.md) | Types, control flow, errors, modules (shipped subset) |
| [Session](session.md) | Bare `thoxa` REPL (emacs editor + Tab), scripts, command sugar, rc |
| [Standard library](std.md) | `std/…` modules |
| [Editor](editor.md) | Zed + `thoxa lsp` (honest limits) |

## Quickstart

```bash
cargo make test
cargo make run tests/conformance/hello.thx
cargo make run examples/http          # Crema-lite HTTP example (port 9091; experiment)
cargo make run containers/landing     # thoxa.dev (port 8088)
cargo make run containers/sola        # sola.computer teaser (port 8089)
cargo make run containers/oath        # oath.wicket.cloud (port 8090)
cargo run -p thoxa -- run examples/kilo examples/kilo/sample.txt  # TTY editor
cargo run -p thoxa --                 # session REPL
cargo run -p thoxa -- run path.thx    # program or session script
```

## Rules of this handbook

- Document **what works**.
- Label **partial** surfaces with limits.
- Do not present deferred features (Perceus, effects, full SPEC) as available.
