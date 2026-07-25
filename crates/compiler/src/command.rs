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
    // Thoxa-first: if the line already parses as a module fragment, keep it.
    if line_parses_as_thoxa(trimmed) {
        return Ok(line.to_string());
    }
    match desugar_command_line(trimmed) {
        Ok(Some(thx)) => {
            // Preserve leading indentation from the original line.
            let indent: String = line.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
            Ok(format!("{indent}{thx}"))
        }
        Ok(None) => Ok(line.to_string()),
        Err(e) => Err(e),
    }
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
    use crate::ast::{BlockItem, ExprKind, Item};
    let Ok(tokens) = Lexer::new(line).tokenize() else {
        return false;
    };
    let Ok(module) = Parser::new(tokens).parse_module() else {
        return false;
    };
    // Word-like lines parse as a sequence of Name / String session exprs
    // (`echo hello`, `echo "hi there"`). That is command skin, not Thoxa.
    // Real Thoxa has calls, operators, assigns, keywords, etc.
    if module.items.is_empty() {
        return false;
    }
    let command_shaped = module.items.iter().all(|item| match item {
        Item::SessionStmt(BlockItem::Expr(e)) => matches!(
            e.kind,
            ExprKind::Name(_) | ExprKind::String(_) | ExprKind::Int(_) | ExprKind::Bool(_)
        ),
        _ => false,
    });
    !command_shaped
}

/// Returns `Some(thoxa_stmt)` when `line` is a command; `None` if empty words.
pub fn desugar_command_line(line: &str) -> Result<Option<String>> {
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
    fn keep_println() {
        let src = desugar_session_source(r#"println("hi")"#).unwrap();
        assert!(src.contains("println"));
        assert!(!src.contains("shell = shell"));
    }

    #[test]
    fn desugar_session_echo_line() {
        let src = desugar_session_source("echo hello\n").unwrap();
        assert!(src.contains(r#"shell = shell.echo(["hello"])"#));
    }
}
