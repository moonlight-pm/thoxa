//! Stamp the git identity into the binary for `thoxa version` and the LSP.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace = manifest_dir
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| manifest_dir.join("../.."));
    emit_version_env(&workspace);
}

/// Stamp short git hash (+ dirty) into the binary for identity checks.
///
/// Overrides: `THOXA_GIT_HASH`, `THOXA_GIT_DIRTY` (`0`/`1`).
fn emit_version_env(workspace: &Path) {
    println!("cargo:rerun-if-env-changed=THOXA_GIT_HASH");
    println!("cargo:rerun-if-env-changed=THOXA_GIT_DIRTY");
    let head = workspace.join(".git/HEAD");
    if head.is_file() {
        println!("cargo:rerun-if-changed={}", head.display());
    }
    // Rebuild when the checked-out commit changes (symbolic HEAD → ref file).
    if let Ok(contents) = fs::read_to_string(&head) {
        if let Some(rest) = contents.trim().strip_prefix("ref: ") {
            let ref_path = workspace.join(".git").join(rest);
            if ref_path.is_file() {
                println!("cargo:rerun-if-changed={}", ref_path.display());
            }
        }
    }

    let hash = env::var("THOXA_GIT_HASH").unwrap_or_else(|_| git_short_hash(workspace));
    let dirty = env::var("THOXA_GIT_DIRTY").unwrap_or_else(|_| {
        if git_is_dirty(workspace) {
            "1".into()
        } else {
            "0".into()
        }
    });
    println!("cargo:rustc-env=THOXA_GIT_HASH={hash}");
    println!("cargo:rustc-env=THOXA_GIT_DIRTY={dirty}");
}

fn git_short_hash(workspace: &Path) -> String {
    let out = Command::new("git")
        .args([
            "-C",
            &workspace.display().to_string(),
            "rev-parse",
            "--short=12",
            "HEAD",
        ])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if s.is_empty() {
                "unknown".into()
            } else {
                s
            }
        }
        _ => "unknown".into(),
    }
}

fn git_is_dirty(workspace: &Path) -> bool {
    let out = Command::new("git")
        .args([
            "-C",
            &workspace.display().to_string(),
            "status",
            "--porcelain",
        ])
        .output();
    match out {
        Ok(o) if o.status.success() => !String::from_utf8_lossy(&o.stdout).trim().is_empty(),
        _ => false,
    }
}
