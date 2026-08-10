//! Build identity for CLI + LSP (`thoxa version`, serverInfo, start log).

use std::env;
use std::path::PathBuf;

/// Cargo package version (`0.1.0`, …).
pub fn package_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Short git hash from build time, or `"unknown"`.
pub fn git_hash() -> &'static str {
    env!("THOXA_GIT_HASH")
}

/// Whether the tree was dirty when this binary was built.
pub fn git_dirty() -> bool {
    env!("THOXA_GIT_DIRTY") == "1"
}

/// Compact version for `serverInfo` / comparison: `0.1.0+abc1234` or `…+abc1234.dirty`.
pub fn version_string() -> String {
    let mut s = format!("{}+{}", package_version(), git_hash());
    if git_dirty() {
        s.push_str(".dirty");
    }
    s
}

/// Path of this running binary, if known.
pub fn executable_path() -> Option<PathBuf> {
    env::current_exe().ok()
}

/// One-line identity for humans (CLI + stderr + LSP logMessage).
pub fn identity_line() -> String {
    let path = executable_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(unknown path)".into());
    format!("thoxa {}  path={path}", version_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_string_includes_package_and_hash() {
        let v = version_string();
        assert!(v.starts_with(package_version()), "{v}");
        assert!(v.contains('+'), "{v}");
        assert!(v.contains(git_hash()), "{v}");
    }
}
