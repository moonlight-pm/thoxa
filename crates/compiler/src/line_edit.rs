//! TTY line editor for the session REPL (D156).
//!
//! Interactive `thoxa` uses rustyline in **emacs** mode: Tab completion, history,
//! and GNU-readline-style bindings. Piped / non-TTY input stays on cooked
//! `stdin` lines in [`crate::repl`].
//!
//! Completion is host-side (no libtcc on Tab): Shell methods, PATH commands,
//! files/dirs, `std/` modules, and names scraped from accumulated decls.

use crate::error::{Error, Result};
use crate::loader;
use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::history::DefaultHistory;
use rustyline::validate::Validator;
use rustyline::{ColorMode, CompletionType, Config, Context, EditMode, Editor, Helper};
use std::borrow::Cow;
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};

/// Shell command-skin words (first token of a stage).
pub const SHELL_WORDS: &[&str] = &["echo", "pwd", "cd", "status", "help", "exit", "quit", "run"];

/// Thoxa keywords offered as first-word / ident completions.
pub const KEYWORDS: &[&str] = &[
    "let", "mut", "fn", "type", "import", "export", "extern", "match", "if", "else", "for",
    "while", "return", "break", "continue",
];

const HISTORY_MAX: usize = 1000;

/// Outcome of one editor read.
pub enum ReadOutcome {
    Line(String),
    /// Ctrl-C: discard the line, keep the session.
    Interrupt,
    /// Ctrl-D on an empty line / EOF.
    Eof,
}

/// Inputs for [`complete_at`] (tests inject these; the helper fills from the process).
pub struct CompleteEnv<'a> {
    pub decls: &'a [String],
    pub path_cmds: &'a [String],
    pub cwd: &'a Path,
    pub std_root: Option<&'a Path>,
    pub home: Option<&'a Path>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Command,
    File,
    Dir,
    Ident,
    Std,
}

/// rustyline helper: completion + ANSI prompt pass-through.
#[derive(Default)]
struct SessionHelper {
    decls: RefCell<Vec<String>>,
    styled_prompt: RefCell<String>,
    path_cache: RefCell<PathCache>,
}

#[derive(Default)]
struct PathCache {
    path: String,
    names: Vec<String>,
}

impl SessionHelper {
    fn path_cmds(&self) -> Vec<String> {
        let path = std::env::var("PATH").unwrap_or_default();
        let mut cache = self.path_cache.borrow_mut();
        if cache.path != path {
            cache.path = path.clone();
            cache.names = list_path_commands(&path);
        }
        cache.names.clone()
    }
}

impl Completer for SessionHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let decls = self.decls.borrow();
        let path_cmds = self.path_cmds();
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let std_root = loader::std_lib_root().ok();
        let home = std::env::var("HOME")
            .ok()
            .filter(|h| !h.is_empty())
            .map(PathBuf::from);
        let env = CompleteEnv {
            decls: &decls,
            path_cmds: &path_cmds,
            cwd: &cwd,
            std_root: std_root.as_deref(),
            home: home.as_deref(),
        };
        let (start, names) = complete_at(line, pos, &env);
        Ok((
            start,
            names
                .into_iter()
                .map(|s| Pair {
                    display: s.clone(),
                    replacement: s,
                })
                .collect(),
        ))
    }
}

impl Hinter for SessionHelper {
    type Hint = String;
}

impl Highlighter for SessionHelper {
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        prompt: &'p str,
        _default: bool,
    ) -> Cow<'b, str> {
        let styled = self.styled_prompt.borrow();
        if styled.is_empty() {
            Cow::Borrowed(prompt)
        } else {
            Cow::Owned(styled.clone())
        }
    }
}

impl Validator for SessionHelper {}

impl Helper for SessionHelper {}

/// Interactive session line editor (emacs mode).
pub struct SessionEditor {
    rl: Editor<SessionHelper, DefaultHistory>,
    hist_path: Option<PathBuf>,
}

impl SessionEditor {
    pub fn new() -> Result<Self> {
        let mut builder = Config::builder()
            .edit_mode(EditMode::Emacs)
            .completion_type(CompletionType::List)
            .color_mode(ColorMode::Enabled)
            .auto_add_history(false);
        builder = builder
            .max_history_size(HISTORY_MAX)
            .map_err(|e| Error::new(format!("line editor: {e}")))?;
        builder = builder
            .history_ignore_dups(true)
            .map_err(|e| Error::new(format!("line editor: {e}")))?;
        let config = builder.build();
        let mut rl =
            Editor::with_config(config).map_err(|e| Error::new(format!("line editor: {e}")))?;
        rl.set_helper(Some(SessionHelper::default()));
        let hist_path = history_path();
        if let Some(p) = &hist_path {
            let _ = rl.load_history(p);
        }
        Ok(Self { rl, hist_path })
    }

    pub fn set_decls(&mut self, decls: &[String]) {
        if let Some(h) = self.rl.helper_mut() {
            h.decls.replace(decls.to_vec());
        }
    }

    pub fn read(&mut self, prompt: &str) -> Result<ReadOutcome> {
        let raw = strip_ansi(prompt);
        if let Some(h) = self.rl.helper_mut() {
            h.styled_prompt.replace(prompt.to_string());
        }
        match self.rl.readline(&(raw.as_str(), prompt)) {
            Ok(line) => {
                if !line.trim().is_empty() {
                    let _ = self.rl.add_history_entry(line.as_str());
                    self.persist_history();
                }
                Ok(ReadOutcome::Line(line))
            }
            Err(ReadlineError::Interrupted) => Ok(ReadOutcome::Interrupt),
            Err(ReadlineError::Eof) => Ok(ReadOutcome::Eof),
            Err(e) => Err(Error::new(format!("stdin: {e}"))),
        }
    }

    fn persist_history(&mut self) {
        let Some(p) = &self.hist_path else {
            return;
        };
        if let Some(parent) = p.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = self.rl.append_history(p);
    }
}

/// History file. Empty `THOXA_HISTFILE` skips history.
///
/// Order: `THOXA_HISTFILE` → `$XDG_STATE_HOME/thoxa/history` →
/// `$HOME/.local/state/thoxa/history`.
pub fn history_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("THOXA_HISTFILE") {
        if p.is_empty() {
            return None;
        }
        return Some(PathBuf::from(p));
    }
    if let Ok(xdg) = std::env::var("XDG_STATE_HOME") {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg).join("thoxa/history"));
        }
    }
    let home = std::env::var("HOME").ok().filter(|h| !h.is_empty())?;
    Some(PathBuf::from(home).join(".local/state/thoxa/history"))
}

/// Tab-complete `line[..pos]`. Returns `(word_start, replacements)`.
pub fn complete_at(line: &str, pos: usize, env: &CompleteEnv<'_>) -> (usize, Vec<String>) {
    let pos = pos.min(line.len());
    // Stay on a char boundary.
    let pos = if line.is_char_boundary(pos) {
        pos
    } else {
        line.floor_char_boundary(pos)
    };
    let start = word_start(line, pos);
    let prefix = &line[start..pos];
    let kind = classify(line, start, prefix);
    let mut out = Vec::new();
    match kind {
        Kind::Command => {
            push_matching(&mut out, SHELL_WORDS.iter().copied(), prefix);
            push_matching(&mut out, KEYWORDS.iter().copied(), prefix);
            let names = scrape_names(env.decls);
            push_matching(&mut out, names.iter().map(String::as_str), prefix);
            push_matching(&mut out, env.path_cmds.iter().map(String::as_str), prefix);
            if looks_like_path(prefix) {
                push_files(&mut out, env, prefix, false);
            }
        }
        Kind::Dir => push_files(&mut out, env, prefix, true),
        Kind::File => push_files(&mut out, env, prefix, false),
        Kind::Ident => {
            let names = scrape_names(env.decls);
            push_matching(&mut out, names.iter().map(String::as_str), prefix);
            push_matching(&mut out, KEYWORDS.iter().copied(), prefix);
        }
        Kind::Std => push_std(&mut out, env.std_root, prefix),
    }
    out.sort();
    out.dedup();
    (start, out)
}

fn classify(line: &str, start: usize, prefix: &str) -> Kind {
    if prefix.starts_with("std/") {
        return Kind::Std;
    }
    if looks_like_path(prefix) {
        return Kind::File;
    }
    if let Some(prev) = previous_word(line, start) {
        if prev == "cd" {
            return Kind::Dir;
        }
        if prev == ">" || prev == ">>" || prev == "<" {
            return Kind::File;
        }
    }
    if is_stage_start(line, start) {
        return Kind::Command;
    }
    if is_thoxa_head(line) {
        return Kind::Ident;
    }
    Kind::File
}

fn is_thoxa_head(line: &str) -> bool {
    let tok = first_token(line);
    KEYWORDS.iter().any(|k| *k == tok)
}

fn first_token(line: &str) -> &str {
    let t = line.trim_start();
    match t.char_indices().find(|(_, c)| is_word_sep(*c)) {
        Some((i, _)) => &t[..i],
        None => t,
    }
}

fn is_stage_start(line: &str, start: usize) -> bool {
    let before = line[..start].trim_end();
    before.is_empty() || before.ends_with('|')
}

fn previous_word(line: &str, start: usize) -> Option<&str> {
    let before = line[..start].trim_end();
    if before.is_empty() {
        return None;
    }
    let wstart = word_start(before, before.len());
    Some(&before[wstart..])
}

fn word_start(line: &str, pos: usize) -> usize {
    let prefix = &line[..pos];
    let mut start = 0;
    for (i, ch) in prefix.char_indices() {
        if is_word_sep(ch) {
            start = i + ch.len_utf8();
        }
    }
    start
}

fn is_word_sep(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            '|' | '<'
                | '>'
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
                | ','
                | ';'
                | '='
                | '"'
                | '\''
                | ':'
        )
}

fn looks_like_path(prefix: &str) -> bool {
    prefix.contains('/') || prefix.starts_with('.') || prefix.starts_with('~')
}

fn push_matching<'a, I: IntoIterator<Item = &'a str>>(
    out: &mut Vec<String>,
    items: I,
    prefix: &str,
) {
    for s in items {
        if s.starts_with(prefix) {
            out.push(s.to_string());
        }
    }
}

/// `fn` / `let` / `mut` / `type` names from accumulated decl sources.
pub fn scrape_names(decls: &[String]) -> Vec<String> {
    let mut names = Vec::new();
    for d in decls {
        for line in d.lines() {
            let t = line.trim();
            let t = t.strip_prefix("export ").unwrap_or(t).trim_start();
            let rest = if let Some(r) = t.strip_prefix("fn ") {
                r
            } else if let Some(r) = t.strip_prefix("let ") {
                r
            } else if let Some(r) = t.strip_prefix("mut ") {
                r
            } else if let Some(r) = t.strip_prefix("type ") {
                r
            } else {
                continue;
            };
            if let Some(name) = take_ident(rest) {
                names.push(name.to_string());
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

fn take_ident(s: &str) -> Option<&str> {
    let s = s.trim_start();
    let mut end = 0usize;
    for (i, c) in s.char_indices() {
        let ok = if i == 0 {
            c.is_ascii_alphabetic() || c == '_'
        } else {
            c.is_ascii_alphanumeric() || c == '_'
        };
        if !ok {
            break;
        }
        end = i + c.len_utf8();
    }
    if end == 0 {
        None
    } else {
        Some(&s[..end])
    }
}

fn expand_tilde<'a>(prefix: &'a str, home: Option<&Path>) -> Cow<'a, str> {
    if prefix == "~" {
        return Cow::Owned(
            home.map(|h| h.display().to_string())
                .unwrap_or_else(|| "~".into()),
        );
    }
    if let Some(rest) = prefix.strip_prefix("~/") {
        if let Some(h) = home {
            return Cow::Owned(format!("{}/{rest}", h.display()));
        }
    }
    Cow::Borrowed(prefix)
}

fn push_files(out: &mut Vec<String>, env: &CompleteEnv<'_>, prefix: &str, dirs_only: bool) {
    let (parent_disp, name_pre) = match prefix.rfind('/') {
        Some(i) => (&prefix[..=i], &prefix[i + 1..]),
        None => ("", prefix),
    };
    let parent_real = expand_tilde(parent_disp, env.home);
    let parent_path = if parent_real.is_empty() {
        env.cwd.to_path_buf()
    } else {
        PathBuf::from(parent_real.as_ref())
    };
    let Ok(rd) = fs::read_dir(&parent_path) else {
        return;
    };
    let show_hidden = name_pre.starts_with('.');
    for ent in rd.flatten() {
        let name = ent.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name == "." || name == ".." {
            continue;
        }
        if name.starts_with('.') && !show_hidden {
            continue;
        }
        if !name.starts_with(name_pre) {
            continue;
        }
        let is_dir = ent.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if dirs_only && !is_dir {
            continue;
        }
        let mut repl = String::from(parent_disp);
        repl.push_str(name);
        if is_dir && !repl.ends_with('/') {
            repl.push('/');
        }
        out.push(repl);
    }
}

fn push_std(out: &mut Vec<String>, std_root: Option<&Path>, prefix: &str) {
    let Some(root) = std_root else {
        return;
    };
    let rest = prefix.strip_prefix("std/").unwrap_or(prefix);
    let (parent_rest, name_pre) = match rest.rfind('/') {
        Some(i) => (&rest[..=i], &rest[i + 1..]),
        None => ("", rest),
    };
    let dir = if parent_rest.is_empty() {
        root.to_path_buf()
    } else {
        root.join(parent_rest.trim_end_matches('/'))
    };
    let Ok(rd) = fs::read_dir(&dir) else {
        return;
    };
    for ent in rd.flatten() {
        let name = ent.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        let is_dir = ent.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            if name.starts_with(name_pre) {
                out.push(format!("std/{parent_rest}{name}/"));
            }
            continue;
        }
        let Some(stem) = name.strip_suffix(".thx") else {
            continue;
        };
        if stem.starts_with(name_pre) {
            out.push(format!("std/{parent_rest}{stem}"));
        }
    }
}

pub fn list_path_commands(path: &str) -> Vec<String> {
    let mut names = Vec::new();
    for dir in path.split(':') {
        if dir.is_empty() {
            continue;
        }
        let Ok(rd) = fs::read_dir(dir) else {
            continue;
        };
        for ent in rd.flatten() {
            let fname = ent.file_name();
            let Some(s) = fname.to_str() else {
                continue;
            };
            if s.starts_with('.') {
                continue;
            }
            let p = ent.path();
            let Ok(meta) = p.metadata() else {
                continue;
            };
            if !meta.is_file() {
                continue;
            }
            if !is_executable(&meta) {
                continue;
            }
            names.push(s.to_string());
        }
    }
    names.sort();
    names.dedup();
    names
}

fn is_executable(meta: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        true
    }
}

/// Strip CSI / OSC sequences so rustyline can compute prompt width.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.peek().copied() {
            Some('[') => {
                chars.next();
                for d in chars.by_ref() {
                    if d.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            Some(']') => {
                chars.next();
                while let Some(d) = chars.next() {
                    if d == '\u{7}' {
                        break;
                    }
                    if d == '\u{1b}' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }
    out
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn env<'a>(
        decls: &'a [String],
        path_cmds: &'a [String],
        cwd: &'a Path,
        std_root: Option<&'a Path>,
        home: Option<&'a Path>,
    ) -> CompleteEnv<'a> {
        CompleteEnv {
            decls,
            path_cmds,
            cwd,
            std_root,
            home,
        }
    }

    fn names(line: &str, env: &CompleteEnv<'_>) -> Vec<String> {
        complete_at(line, line.len(), env).1
    }

    #[test]
    fn first_word_shell_and_keywords() {
        let cwd = Path::new(".");
        let e = env(&[], &[], cwd, None, None);
        let got = names("ec", &e);
        assert!(got.iter().any(|s| s == "echo"), "{got:?}");
        let got = names("le", &e);
        assert!(got.iter().any(|s| s == "let"), "{got:?}");
        let got = names("cd", &e);
        assert!(got.iter().any(|s| s == "cd"), "{got:?}");
    }

    #[test]
    fn path_commands_on_first_word() {
        let cwd = Path::new(".");
        let cmds = vec!["ls".into(), "cat".into(), "cargo".into()];
        let e = env(&[], &cmds, cwd, None, None);
        let got = names("ca", &e);
        assert!(got.iter().any(|s| s == "cat"), "{got:?}");
        assert!(got.iter().any(|s| s == "cargo"), "{got:?}");
        assert!(!got.iter().any(|s| s == "ls"), "{got:?}");
    }

    #[test]
    fn decl_names_on_thoxa_line() {
        let decls = vec!["fn greet() -> String { \"hi\" }\n".into()];
        let cwd = Path::new(".");
        let e = env(&decls, &[], cwd, None, None);
        let got = names("let x = gre", &e);
        assert!(got.iter().any(|s| s == "greet"), "{got:?}");
    }

    #[test]
    fn scrape_fn_let_type() {
        let decls = vec![r#"
fn prompt() -> String { "x" }
let answer = 42
type Foo(x: Int64) {}
export fn helper() -> Int64 { 0 }
"#
        .into()];
        let n = scrape_names(&decls);
        assert!(n.contains(&"prompt".into()), "{n:?}");
        assert!(n.contains(&"answer".into()), "{n:?}");
        assert!(n.contains(&"Foo".into()), "{n:?}");
        assert!(n.contains(&"helper".into()), "{n:?}");
    }

    #[test]
    fn files_after_echo_and_dirs_after_cd() {
        let dir = tempfile_dir("thoxa_comp_files");
        fs::write(dir.join("readme.txt"), "x").unwrap();
        fs::create_dir(dir.join("src")).unwrap();
        let e = env(&[], &[], &dir, None, None);

        let got = names("echo re", &e);
        assert!(got.iter().any(|s| s == "readme.txt"), "{got:?}");
        assert!(!got.iter().any(|s| s == "src/"), "{got:?}");

        let got = names("cd s", &e);
        assert!(got.iter().any(|s| s == "src/"), "{got:?}");
        assert!(!got.iter().any(|s| s == "readme.txt"), "{got:?}");
    }

    #[test]
    fn redir_completes_files() {
        let dir = tempfile_dir("thoxa_comp_redir");
        fs::write(dir.join("out.txt"), "").unwrap();
        let e = env(&[], &[], &dir, None, None);
        let got = names("echo hi > ou", &e);
        assert!(got.iter().any(|s| s == "out.txt"), "{got:?}");
    }

    #[test]
    fn pipeline_stage_is_command() {
        let cwd = Path::new(".");
        let cmds = vec!["cat".into()];
        let e = env(&[], &cmds, cwd, None, None);
        let got = names("echo hi | ca", &e);
        assert!(got.iter().any(|s| s == "cat"), "{got:?}");
    }

    #[test]
    fn std_modules() {
        let dir = tempfile_dir("thoxa_comp_std");
        fs::write(dir.join("shell.thx"), "").unwrap();
        fs::write(dir.join("logging.thx"), "").unwrap();
        fs::create_dir(dir.join("http")).unwrap();
        fs::write(dir.join("http").join("server.thx"), "").unwrap();
        let e = env(&[], &[], Path::new("."), Some(&dir), None);

        let got = names("std/sh", &e);
        assert!(got.iter().any(|s| s == "std/shell"), "{got:?}");
        let got = names("std/http/", &e);
        assert!(got.iter().any(|s| s == "std/http/server"), "{got:?}");
    }

    #[test]
    fn tilde_expands_for_files() {
        let home = tempfile_dir("thoxa_comp_home");
        fs::write(home.join("notes.md"), "").unwrap();
        let e = env(&[], &[], Path::new("."), None, Some(&home));
        let got = names("echo ~/no", &e);
        assert!(got.iter().any(|s| s == "~/notes.md"), "{got:?}");
    }

    #[test]
    fn list_path_commands_respects_exec_bit() {
        let dir = tempfile_dir("thoxa_comp_path");
        let bin = dir.join("mytool");
        fs::write(&bin, "#!/bin/sh\n").unwrap();
        let mut perms = fs::metadata(&bin).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&bin, perms).unwrap();
        fs::write(dir.join("readme"), "no").unwrap();
        let names = list_path_commands(&dir.to_string_lossy());
        assert!(names.iter().any(|s| s == "mytool"), "{names:?}");
        assert!(!names.iter().any(|s| s == "readme"), "{names:?}");
    }

    #[test]
    fn strip_ansi_sgr() {
        assert_eq!(strip_ansi("\u{1b}[31mhi\u{1b}[0m"), "hi");
        assert_eq!(strip_ansi("plain"), "plain");
        assert_eq!(strip_ansi("a\u{1b}[1;34mb\u{1b}[0m"), "ab");
    }

    #[test]
    fn word_start_after_separators() {
        assert_eq!(word_start("echo hi", 7), 5);
        assert_eq!(word_start("a | ca", 6), 4);
        assert_eq!(word_start("echo \"hel", 9), 6);
    }

    #[test]
    fn history_path_override() {
        let prev = std::env::var("THOXA_HISTFILE").ok();
        std::env::set_var("THOXA_HISTFILE", "/tmp/thoxa-hist-test");
        assert_eq!(history_path(), Some(PathBuf::from("/tmp/thoxa-hist-test")));
        std::env::set_var("THOXA_HISTFILE", "");
        assert!(history_path().is_none());
        match prev {
            Some(p) => std::env::set_var("THOXA_HISTFILE", p),
            None => std::env::remove_var("THOXA_HISTFILE"),
        }
    }

    fn tempfile_dir(prefix: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "{prefix}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        base
    }
}
