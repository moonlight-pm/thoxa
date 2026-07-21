# Conformance suite — the executable spec

Each `*.thx` file here is a runnable Thoxa program that pins down a piece of `SPEC.md`.

**Format**

- `foo.thx` — the program.
- `foo.out` — *(optional)* the exact expected stdout. If present, the program's stdout must match
  byte-for-byte.
- `foo.exit` — *(optional)* the expected process exit code (integer). Defaults to `0`.

If neither `.out` nor `.exit` is present, the test passes as long as the program runs to completion
without error and exits `0`.

`tests/test_conformance.py` discovers every `*.thx` here and runs it through the real interpreter.

**The rule:** a language feature is only "done" when a conformance test exercises it and passes.
No feature is documented as working before its test is green.
