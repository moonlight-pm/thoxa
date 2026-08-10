//! Command-skin word split and desugar (session plan PR2).
//!
//! Thoxa-first: only rewrite a line when it is not already valid Thoxa module
//! syntax. Command lines become assignments on a `shell` binding that
//! [`session`] injects inside `Shell.step`.

use crate::error::{Error, Result};
use crate::lexer::Lexer;
use crate::parser::Parser;

/// Desugar session source: top-level lines that fail as Thoxa and look like
/// commands become `shell = shell.<builtin|run>(…)`.
///
/// Leaves programs that define `fn main` untouched (batch path).
/// Only rewrites at brace-depth 0 so method/type bodies are not mangled.
pub fn desugar_session_source(src: &str) -> Result<String> {
    if source_has_fn_main(src) {
        return Ok(src.to_string());
    }
    let mut out = String::with_capacity(src.len() + 64);
    let mut depth = 0i32;
    for (i, line) in src.lines().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if depth == 0 {
            out.push_str(&desugar_line(line)?);
        } else {
            out.push_str(line);
        }
        depth += brace_delta(line);
        if depth < 0 {
            depth = 0;
        }
    }
    if src.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

/// Approximate `{` / `}` depth change on a line (ignores strings — good enough for desugar).
fn brace_delta(line: &str) -> i32 {
    let mut d = 0i32;
    let mut in_str = false;
    let mut quote = b'\0';
    let mut esc = false;
    for &b in line.as_bytes() {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == quote {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' | b'\'' => {
                in_str = true;
                quote = b;
            }
            b'{' => d += 1,
            b'}' => d -= 1,
            _ => {}
        }
    }
    d
}

fn source_has_fn_main(src: &str) -> bool {
    // Crude but sufficient: whole-file programs keep batch semantics.
    for line in src.lines() {
        let t = trim_line_comment(line);
        if t.starts_with("fn main") || t.starts_with("export fn main") {
            return true;
        }
    }
    false
}

fn trim_line_comment(line: &str) -> &str {
    let t = line.trim();
    if let Some(i) = t.find("##") {
        t[..i].trim()
    } else {
        t
    }
}

fn desugar_line(line: &str) -> Result<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with("##") || trimmed.starts_with('#') {
        return Ok(line.to_string());
    }
    // Declarations stay as-is.
    if is_decl_prefix(trimmed) {
        return Ok(line.to_string());
    }
    let indent: String = line.chars().take_while(|c| *c == ' ' || *c == '\t').collect();

    // Pipelines before Thoxa-first: `echo hi | cat` parses as BitOr and would
    // otherwise be kept as broken/meaningless multi-stmt Thoxa. Pure integer
    // bitwise-or (`1 | 2`) stays Thoxa.
    if let Some(stages) = split_pipeline_stages(trimmed)? {
        if stages_are_pure_int_literals(&stages) {
            return Ok(line.to_string());
        }
        return Ok(format!("{indent}{}", desugar_pipeline(&stages)));
    }

    // Redirs before Thoxa-first: `echo hi > f` parses as Gt and would be kept.
    // Skip when the first word is an int literal so `1 > 0` stays Thoxa.
    if let Some(spec) = parse_redirect_command(trimmed)? {
        return Ok(format!("{indent}{}", desugar_redirect(&spec)));
    }

    // Thoxa-first: if the line already parses as a module fragment, keep it.
    if line_parses_as_thoxa(trimmed) {
        return Ok(line.to_string());
    }
    match desugar_command_line(trimmed) {
        Ok(Some(thx)) => Ok(format!("{indent}{thx}")),
        Ok(None) => Ok(line.to_string()),
        Err(e) => Err(e),
    }
}

/// `1 | 2` stages — keep as Thoxa BitOr, not `run(["1"]) | run(["2"])`.
fn stages_are_pure_int_literals(stages: &[Vec<String>]) -> bool {
    stages.iter().all(|stage| {
        stage.len() == 1 && stage[0].parse::<i64>().is_ok()
    })
}

fn is_decl_prefix(t: &str) -> bool {
    t.starts_with("fn ")
        || t.starts_with("export ")
        || t.starts_with("type ")
        || t.starts_with("extern ")
        || t.starts_with("import ")
        || t.starts_with("let ")
        || t.starts_with("mut ")
}

fn line_parses_as_thoxa(line: &str) -> bool {
    use crate::ast::{BlockItem, Item};
    let Ok(tokens) = Lexer::new(line).tokenize() else {
        return false;
    };
    let Ok(module) = Parser::new(tokens).parse_module() else {
        return false;
    };
    // Word-like / path-like / flag-like lines parse as session exprs that are
    // not intentional Thoxa statements:
    //   echo hello          → Name, Name
    //   echo "hi there"     → Name, String
    //   wc AGENTS.md        → Name, Field(AGENTS, md)   ← was mis-kept as Thoxa
    //   ls /tmp             → often Binary Div of names
    //   ls -al              → Binary Sub (ls - al)      ← was mis-kept as Thoxa
    //   ls --all            → Sub of Unary Neg
    // Real Thoxa has calls, assigns, most operators, keywords, etc.
    // Pure literal arith (`1 - 2`) is kept as Thoxa.
    if module.items.is_empty() {
        return false;
    }
    let command_shaped = module.items.iter().all(|item| match item {
        Item::SessionStmt(BlockItem::Expr(e)) => {
            expr_is_command_atom(e) && !expr_is_pure_literal_arith(e)
        }
        _ => false,
    });
    !command_shaped
}

/// True for atoms / trees that appear when a shell line is naively parsed as Thoxa.
///
/// Includes flag-like trees: `ls -al` → `ls - al` (Sub), `ls --all` → Sub of Unary Neg.
/// Pure literal arithmetic (`1 - 2`) is still an atom tree but filtered out in
/// [`line_parses_as_thoxa`] so it stays real Thoxa.
fn expr_is_command_atom(e: &crate::ast::Expr) -> bool {
    use crate::ast::{BinaryOp, ExprKind, UnaryOp};
    match &e.kind {
        ExprKind::Name(_) | ExprKind::String(_) | ExprKind::Int(_) | ExprKind::Bool(_) => true,
        // `AGENTS.md`, `foo.bar.baz` — path-like, not intentional field reads at stmt level
        ExprKind::Field { base, .. } => expr_is_command_atom(base),
        // Paths (`a/b`) and flags (`ls -al`, `cmd -a -b`, `ls --all`)
        ExprKind::Binary {
            op:
                BinaryOp::Div
                | BinaryOp::Sub
                | BinaryOp::Add
                | BinaryOp::Mul
                | BinaryOp::Rem,
            left,
            right,
        } => expr_is_command_atom(left) && expr_is_command_atom(right),
        // `--flag` right-hand side / leading `-word`
        ExprKind::Unary {
            op: UnaryOp::Neg,
            expr,
        } => expr_is_command_atom(expr),
        _ => false,
    }
}

/// Only Int/Bool and arith/neg — keep as Thoxa (`1 - 2`), not `run(["1","-","2"])`.
fn expr_is_pure_literal_arith(e: &crate::ast::Expr) -> bool {
    use crate::ast::{BinaryOp, ExprKind, UnaryOp};
    match &e.kind {
        ExprKind::Int(_) | ExprKind::Bool(_) => true,
        ExprKind::Binary {
            op:
                BinaryOp::Div
                | BinaryOp::Sub
                | BinaryOp::Add
                | BinaryOp::Mul
                | BinaryOp::Rem,
            left,
            right,
        } => expr_is_pure_literal_arith(left) && expr_is_pure_literal_arith(right),
        ExprKind::Unary {
            op: UnaryOp::Neg,
            expr,
        } => expr_is_pure_literal_arith(expr),
        _ => false,
    }
}

/// Returns `Some(thoxa_stmt)` when `line` is a command; `None` if empty words.
pub fn desugar_command_line(line: &str) -> Result<Option<String>> {
    // `a | b | c` → `shell = shell.pipeline(sessionPipelineBytes(…))`
    if let Some(stages) = split_pipeline_stages(line)? {
        return Ok(Some(desugar_pipeline(&stages)));
    }
    if let Some(spec) = parse_redirect_command(line)? {
        return Ok(Some(desugar_redirect(&spec)));
    }
    let words = split_words(line)?;
    if words.is_empty() {
        return Ok(None);
    }
    let cmd = &words[0];
    let args = &words[1..];
    let thx = match cmd.as_str() {
        "echo" => format!("shell = shell.echo({})", array_lit(args)),
        "pwd" => "shell = shell.pwd()".to_string(),
        "cd" => format!("shell = shell.cd({})", array_lit(args)),
        "status" => "shell = shell.showStatus()".to_string(),
        "help" => "shell = shell.help()".to_string(),
        "exit" | "quit" => {
            // Scripts: treat as no-op status preserve (REPL host handles later).
            "shell = shell".to_string()
        }
        _ => format!("shell = shell.run({})", array_lit(&words)),
    };
    Ok(Some(thx))
}

/// Parsed single-command line with optional file redirections.
struct RedirectSpec {
    argv: Vec<String>,
    stdin: Option<String>,
    stdout: Option<String>,
    append: bool,
}

/// `cmd > f`, `cmd >> f`, `cmd < f` — requires redir ops as separate words.
/// Returns `None` when no redirs, or when first word is an int (`1 > 0` Thoxa).
fn parse_redirect_command(line: &str) -> Result<Option<RedirectSpec>> {
    let words = match split_words(line) {
        Ok(w) => w,
        Err(_) => return Ok(None),
    };
    if words.is_empty() {
        return Ok(None);
    }
    let has_redir = words
        .iter()
        .any(|w| w == ">" || w == ">>" || w == "<");
    if !has_redir {
        return Ok(None);
    }
    // Protect pure numeric comparisons: `1 > 0`.
    if words[0].parse::<i64>().is_ok() {
        return Ok(None);
    }

    let mut argv: Vec<String> = Vec::new();
    let mut stdin: Option<String> = None;
    let mut stdout: Option<String> = None;
    let mut append = false;
    let mut i = 0usize;
    while i < words.len() {
        match words[i].as_str() {
            ">" | ">>" => {
                let is_append = words[i] == ">>";
                i += 1;
                if i >= words.len() {
                    return Err(Error::new("missing path after redirection"));
                }
                let path = words[i].clone();
                if path == ">" || path == ">>" || path == "<" {
                    return Err(Error::new("missing path after redirection"));
                }
                stdout = Some(path);
                append = is_append;
                i += 1;
            }
            "<" => {
                i += 1;
                if i >= words.len() {
                    return Err(Error::new("missing path after redirection"));
                }
                let path = words[i].clone();
                if path == ">" || path == ">>" || path == "<" {
                    return Err(Error::new("missing path after redirection"));
                }
                stdin = Some(path);
                i += 1;
            }
            _ => {
                argv.push(words[i].clone());
                i += 1;
            }
        }
    }
    if argv.is_empty() {
        return Err(Error::new("redirection without a command"));
    }
    Ok(Some(RedirectSpec {
        argv,
        stdin,
        stdout,
        append,
    }))
}

fn desugar_redirect(spec: &RedirectSpec) -> String {
    let stdin = spec.stdin.as_deref().unwrap_or("");
    let stdout = spec.stdout.as_deref().unwrap_or("");
    let append = if spec.append { "true" } else { "false" };
    format!(
        "shell = shell.runIo({}, \"{}\", \"{}\", {})",
        array_lit(&spec.argv),
        escape_thoxa_string(stdin),
        escape_thoxa_string(stdout),
        append
    )
}

/// Quote-aware split on `|` into command stages. Returns `None` when the line
/// is not a multi-stage pipeline (`||` is not a pipe; single stage → None).
fn split_pipeline_stages(line: &str) -> Result<Option<Vec<Vec<String>>>> {
    let bytes = line.as_bytes();
    let mut stage_ranges: Vec<(usize, usize)> = Vec::new();
    let mut stage_start = 0usize;
    let mut state = 0u8; // 0 normal, 1 double, 2 single
    let mut escape = false;
    let mut i = 0usize;
    let mut saw_pipe = false;
    while i < bytes.len() {
        let c = bytes[i];
        if escape {
            escape = false;
            i += 1;
            continue;
        }
        if state == 1 {
            if c == b'\\' {
                escape = true;
            } else if c == b'"' {
                state = 0;
            }
            i += 1;
            continue;
        }
        if state == 2 {
            if c == b'\'' {
                state = 0;
            }
            i += 1;
            continue;
        }
        match c {
            b'\\' => {
                escape = true;
                i += 1;
            }
            b'"' => {
                state = 1;
                i += 1;
            }
            b'\'' => {
                state = 2;
                i += 1;
            }
            b'|' => {
                // `||` is not a pipeline separator (leave for Thoxa if ever).
                if i + 1 < bytes.len() && bytes[i + 1] == b'|' {
                    return Ok(None);
                }
                stage_ranges.push((stage_start, i));
                stage_start = i + 1;
                saw_pipe = true;
                i += 1;
            }
            _ => i += 1,
        }
    }
    if state != 0 {
        return Err(Error::new("unclosed quote in command line"));
    }
    if !saw_pipe {
        return Ok(None);
    }
    stage_ranges.push((stage_start, bytes.len()));

    let mut stages: Vec<Vec<String>> = Vec::new();
    for (a, b) in stage_ranges {
        let part = std::str::from_utf8(&bytes[a..b]).unwrap_or("").trim();
        if part.is_empty() {
            return Err(Error::new("empty pipeline stage"));
        }
        let words = split_words(part)?;
        if words.is_empty() {
            return Err(Error::new("empty pipeline stage"));
        }
        stages.push(words);
    }
    if stages.len() < 2 {
        return Ok(None);
    }
    Ok(Some(stages))
}

fn desugar_pipeline(stages: &[Vec<String>]) -> String {
    // Nested sessionPipelineBytes(sessionEncodeArgv([...]), …)
    let mut expr = format!("sessionEncodeArgv({})", array_lit(&stages[0]));
    for stage in &stages[1..] {
        expr = format!(
            "sessionPipelineBytes({}, sessionEncodeArgv({}))",
            expr,
            array_lit(stage)
        );
    }
    format!("shell = shell.pipeline({expr})")
}

fn array_lit(parts: &[String]) -> String {
    if parts.is_empty() {
        // `[]` alone is Array<Void>; prelude provides a typed empty helper.
        return "sessionEmptyStrings()".to_string();
    }
    let mut s = String::from("[");
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        s.push_str(&format!("\"{}\"", escape_thoxa_string(p)));
    }
    s.push(']');
    s
}

fn escape_thoxa_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out
}

/// Quote-aware word split (thoxash v3 rules).
pub fn split_words(line: &str) -> Result<Vec<String>> {
    let bytes = line.as_bytes();
    let mut words: Vec<String> = Vec::new();
    let mut acc: Vec<u8> = Vec::new();
    let mut in_word = false;
    let mut state = 0u8; // 0 normal, 1 double, 2 single
    let mut escape = false;
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        if escape {
            acc.push(c);
            in_word = true;
            escape = false;
        } else if state == 1 {
            if c == b'\\' {
                escape = true;
            } else if c == b'"' {
                state = 0;
            } else {
                acc.push(c);
                in_word = true;
            }
        } else if state == 2 {
            if c == b'\'' {
                state = 0;
            } else {
                acc.push(c);
                in_word = true;
            }
        } else if c == b'\\' {
            escape = true;
            in_word = true;
        } else if c == b'"' {
            state = 1;
            in_word = true;
        } else if c == b'\'' {
            state = 2;
            in_word = true;
        } else if c == b' ' || c == b'\t' {
            if in_word {
                words.push(String::from_utf8_lossy(&acc).into_owned());
                acc.clear();
                in_word = false;
            }
        } else {
            acc.push(c);
            in_word = true;
        }
        i += 1;
    }
    if escape {
        acc.push(b'\\');
        in_word = true;
    }
    if state != 0 {
        return Err(Error::new("unclosed quote in command line"));
    }
    if in_word {
        words.push(String::from_utf8_lossy(&acc).into_owned());
    }
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_quotes() {
        let w = split_words(r#"echo "hi there""#).unwrap();
        assert_eq!(w, vec!["echo", "hi there"]);
    }

    #[test]
    fn desugar_echo() {
        let s = desugar_command_line(r#"echo "a b""#).unwrap().unwrap();
        assert_eq!(s, r#"shell = shell.echo(["a b"])"#);
    }

    #[test]
    fn desugar_run() {
        let s = desugar_command_line("uname -s").unwrap().unwrap();
        assert_eq!(s, r#"shell = shell.run(["uname", "-s"])"#);
    }

    #[test]
    fn keep_logging_call() {
        let src = desugar_session_source(r#"logging.info("hi")"#).unwrap();
        assert!(src.contains("logging.info"));
        assert!(!src.contains("shell = shell"));
    }

    #[test]
    fn desugar_session_echo_line() {
        let src = desugar_session_source("echo hello\n").unwrap();
        assert!(src.contains(r#"shell = shell.echo(["hello"])"#));
    }

    #[test]
    fn desugar_path_arg_with_dot() {
        // Was mis-parsed as Field(AGENTS, md) and kept as broken Thoxa.
        let src = desugar_session_source("wc AGENTS.md\n").unwrap();
        assert!(
            src.contains(r#"shell = shell.run(["wc", "AGENTS.md"])"#),
            "got: {src}"
        );
    }

    #[test]
    fn keep_real_field_in_call() {
        // Calls mean real Thoxa — do not steal.
        let src = desugar_session_source(r#"logging.info(p.name)"#).unwrap();
        assert!(src.contains("logging.info"));
        assert!(!src.contains("shell = shell.run"));
    }

    #[test]
    fn desugar_flags_not_subtraction() {
        // Was mis-parsed as `ls - al` / `ls - (-all)` and kept as broken Thoxa.
        let src = desugar_session_source("ls -al\n").unwrap();
        assert!(
            src.contains(r#"shell = shell.run(["ls", "-al"])"#),
            "got: {src}"
        );
        let src = desugar_session_source("ls --all\n").unwrap();
        assert!(
            src.contains(r#"shell = shell.run(["ls", "--all"])"#),
            "got: {src}"
        );
        let src = desugar_session_source("ls -a -l\n").unwrap();
        assert!(
            src.contains(r#"shell = shell.run(["ls", "-a", "-l"])"#),
            "got: {src}"
        );
    }

    #[test]
    fn keep_pure_literal_arith() {
        let src = desugar_session_source("1 - 2\n").unwrap();
        assert!(
            !src.contains("shell = shell.run"),
            "literal arith should stay Thoxa, got: {src}"
        );
        assert!(src.contains("1"), "got: {src}");
    }

    #[test]
    fn desugar_pipeline_two_stages() {
        let s = desugar_command_line("echo hi | cat").unwrap().unwrap();
        assert!(
            s.contains("shell = shell.pipeline(")
                && s.contains(r#"sessionEncodeArgv(["echo", "hi"])"#)
                && s.contains(r#"sessionEncodeArgv(["cat"])"#)
                && s.contains("sessionPipelineBytes"),
            "got: {s}"
        );
    }

    #[test]
    fn desugar_pipeline_three_stages() {
        let s = desugar_command_line("echo a | cat | cat").unwrap().unwrap();
        assert!(
            s.contains("shell = shell.pipeline(") && s.matches("sessionEncodeArgv").count() == 3,
            "got: {s}"
        );
    }

    #[test]
    fn pipeline_not_double_bar() {
        // `||` must not be treated as a pipeline (leave line alone / non-pipe path).
        let stages = split_pipeline_stages("true || false").unwrap();
        assert!(stages.is_none());
    }

    #[test]
    fn pipeline_respects_quotes() {
        let s = desugar_command_line(r#"echo "a|b" | cat"#).unwrap().unwrap();
        assert!(
            s.contains(r#"sessionEncodeArgv(["echo", "a|b"])"#),
            "got: {s}"
        );
    }

    #[test]
    fn desugar_session_pipeline_line() {
        let src = desugar_session_source("echo hello | cat\n").unwrap();
        assert!(
            src.contains("shell = shell.pipeline("),
            "got: {src}"
        );
    }

    #[test]
    fn keep_pure_int_bitor() {
        let src = desugar_session_source("1 | 2\n").unwrap();
        assert!(
            !src.contains("shell = shell.pipeline"),
            "integer BitOr should stay Thoxa, got: {src}"
        );
    }

    #[test]
    fn desugar_redirect_stdout() {
        let s = desugar_command_line("echo hi > /tmp/x").unwrap().unwrap();
        assert!(
            s.contains(r#"shell = shell.runIo(["echo", "hi"], "", "/tmp/x", false)"#),
            "got: {s}"
        );
    }

    #[test]
    fn desugar_redirect_append_and_stdin() {
        let s = desugar_command_line("cat < in.txt >> out.txt")
            .unwrap()
            .unwrap();
        assert!(
            s.contains(r#"shell = shell.runIo(["cat"], "in.txt", "out.txt", true)"#),
            "got: {s}"
        );
    }

    #[test]
    fn desugar_session_redirect_line() {
        let src = desugar_session_source("echo hello > /tmp/thoxa_redir\n").unwrap();
        assert!(
            src.contains("shell = shell.runIo("),
            "got: {src}"
        );
    }

    #[test]
    fn keep_pure_int_gt() {
        let src = desugar_session_source("1 > 0\n").unwrap();
        assert!(
            !src.contains("shell = shell.runIo"),
            "integer comparison should stay Thoxa, got: {src}"
        );
    }
}
