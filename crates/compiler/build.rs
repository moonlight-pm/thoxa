//! Build vendored TinyCC (`libtcc` + `libtcc1`) for Linux session steps (D105).
//!
//! Install prefix: `<workspace>/target/c/tinycc`
//! (stable for `tcc_set_lib_path` / libtcc1 at runtime).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    // D105.c: Linux x86_64 first; skip link on other hosts.
    if os != "linux" || arch != "x86_64" {
        println!("cargo:rustc-check-cfg=cfg(thoxa_libtcc)");
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace = manifest_dir
        .join("../..")
        .canonicalize()
        .expect("workspace root");
    let src = workspace.join("vendor/tinycc");
    if !src.join("configure").is_file() {
        panic!(
            "vendored TinyCC missing at {} (expected vendor/tinycc)",
            src.display()
        );
    }

    let build_dir = workspace.join("target/c/tinycc-build");
    let prefix = workspace.join("target/c/tinycc");
    let libtcc_a = prefix.join("lib/libtcc.a");
    let libtcc1_a = prefix.join("lib/tcc/libtcc1.a");

    println!("cargo:rerun-if-changed={}", src.join("libtcc.c").display());
    println!("cargo:rerun-if-changed={}", src.join("libtcc.h").display());
    println!("cargo:rerun-if-changed={}", src.join("configure").display());
    println!("cargo:rerun-if-changed={}", src.join("Makefile").display());
    println!("cargo:rerun-if-changed={}", src.join("tcc.h").display());
    println!("cargo:rerun-if-env-changed=CC");

    if !(libtcc_a.is_file() && libtcc1_a.is_file()) {
        build_tinycc(&src, &build_dir, &prefix);
    }

    if !libtcc_a.is_file() {
        panic!("libtcc.a missing after build at {}", libtcc_a.display());
    }
    if !libtcc1_a.is_file() {
        panic!("libtcc1.a missing after build at {}", libtcc1_a.display());
    }

    println!("cargo:rustc-link-search=native={}", prefix.join("lib").display());
    println!("cargo:rustc-link-lib=static=tcc");
    println!("cargo:rustc-link-lib=dylib=dl");
    println!("cargo:rustc-link-lib=dylib=pthread");
    println!("cargo:rustc-link-lib=dylib=m");

    // Runtime: directory containing libtcc1.a (+ tcc include stubs).
    let tccdir = prefix.join("lib/tcc");
    println!("cargo:rustc-env=THOXA_TCCDIR={}", tccdir.display());
    println!(
        "cargo:rustc-env=THOXA_LIBTCC_INCLUDE={}",
        prefix.join("include").display()
    );

    println!("cargo:rustc-cfg=thoxa_libtcc");
    println!("cargo:rustc-check-cfg=cfg(thoxa_libtcc)");
}

fn build_tinycc(src: &Path, build_dir: &Path, prefix: &Path) {
    let _ = fs::remove_dir_all(build_dir);
    fs::create_dir_all(build_dir).expect("create tinycc build dir");
    fs::create_dir_all(prefix).expect("create tinycc prefix");

    let sysincludes = gcc_sysinclude_paths();
    let glibc_lib = dirname_of_gcc_file("libc.so.6");
    let crt = dirname_of_gcc_file("crt1.o");

    let configure = src.join("configure");
    let status = Command::new(&configure)
        .current_dir(build_dir)
        .args([
            format!("--prefix={}", prefix.display()),
            format!("--source-path={}", src.display()),
            "--enable-static".into(),
            "--extra-cflags=-fPIC -O2".into(),
            "--config-bcheck=no".into(),
            format!("--sysincludepaths={sysincludes}"),
            format!("--libpaths={{B}}:{glibc_lib}:/usr/lib64:/usr/lib"),
            format!("--crtprefix={crt}"),
        ])
        .status()
        .expect("spawn tinycc configure");
    if !status.success() {
        panic!("tinycc configure failed with {status}");
    }

    let jobs = env::var("NUM_JOBS")
        .ok()
        .or_else(|| env::var("CARGO_BUILD_JOBS").ok())
        .unwrap_or_else(|| "4".into());

    // libtcc1 needs the `tcc` binary; `make install` pulls both.
    let status = Command::new("make")
        .current_dir(build_dir)
        .args([format!("-j{jobs}"), "libtcc.a".into()])
        .status()
        .expect("spawn make libtcc.a");
    if !status.success() {
        panic!("make libtcc.a failed with {status}");
    }

    let status = Command::new("make")
        .current_dir(build_dir)
        .args([format!("-j{jobs}"), "libtcc1.a".into()])
        .status()
        .expect("spawn make libtcc1.a");
    if !status.success() {
        panic!("make libtcc1.a failed with {status}");
    }

    let status = Command::new("make")
        .current_dir(build_dir)
        .arg("install")
        .status()
        .expect("spawn make install");
    if !status.success() {
        panic!("make install (tinycc) failed with {status}");
    }
}

fn gcc_sysinclude_paths() -> String {
    let out = Command::new("gcc")
        .args(["-E", "-Wp,-v", "-xc", "-", "-o", "/dev/null"])
        .stdin(std::process::Stdio::null())
        .output()
        .expect("run gcc for include paths");
    let stderr = String::from_utf8_lossy(&out.stderr);
    let mut paths = Vec::new();
    let mut in_list = false;
    for line in stderr.lines() {
        if line.contains("search starts here") {
            in_list = true;
            continue;
        }
        if line.contains("End of search list") {
            break;
        }
        if in_list {
            let p = line.trim();
            if !p.is_empty() && !p.starts_with("ignoring") {
                paths.push(p.to_string());
            }
        }
    }
    if paths.is_empty() {
        // Fallback for conventional FHS hosts.
        paths.push("/usr/include".into());
    }
    paths.join(":")
}

fn dirname_of_gcc_file(name: &str) -> String {
    let out = Command::new("gcc")
        .args(["-print-file-name", name])
        .output()
        .expect("gcc -print-file-name");
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Path::new(&path)
        .parent()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "/usr/lib".into())
}
