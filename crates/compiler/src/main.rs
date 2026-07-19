//! `thoxa` CLI.

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use thoxa::driver::{self, BuildOptions};
use thoxa::error::Error;

fn main() -> ExitCode {
    match try_main() {
        Ok(code) => ExitCode::from(code as u8),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

fn try_main() -> Result<i32, Error> {
    let mut args = env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() {
        print_usage();
        return Ok(2);
    }
    let cmd = args.remove(0);
    match cmd.as_str() {
        "build" => {
            let opts = parse_build_args(&args)?;
            let art = driver::build(&opts)?;
            if let Some(bin) = art.bin_path {
                eprintln!("built {}", bin.display());
            }
            Ok(0)
        }
        "run" => {
            let opts = parse_build_args(&args)?;
            driver::run(&opts)
        }
        "emit-c" => {
            let opts = parse_build_args(&args)?;
            let mut opts = opts;
            opts.emit_c_only = true;
            let path = driver::emit_c(&opts)?;
            println!("{}", path.display());
            Ok(0)
        }
        "runtime" => {
            driver::build_runtime()?;
            eprintln!("runtime → {}", driver::runtime_lib_path().display());
            Ok(0)
        }
        "-h" | "--help" | "help" => {
            print_usage();
            Ok(0)
        }
        other => Err(Error::new(format!(
            "unknown command `{other}` (try `thoxa help`)"
        ))),
    }
}

fn parse_build_args(args: &[String]) -> Result<BuildOptions, Error> {
    let mut source: Option<PathBuf> = None;
    let mut output_name: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--output" => {
                i += 1;
                let name = args
                    .get(i)
                    .ok_or_else(|| Error::new("-o requires a name"))?;
                output_name = Some(name.clone());
            }
            s if s.starts_with('-') => {
                return Err(Error::new(format!("unknown flag `{s}`")));
            }
            s => {
                if source.is_some() {
                    return Err(Error::new("multiple source files not supported yet"));
                }
                source = Some(PathBuf::from(s));
            }
        }
        i += 1;
    }
    let source = source.ok_or_else(|| Error::new("missing source file"))?;
    Ok(BuildOptions {
        source,
        output_name,
        emit_c_only: false,
    })
}

fn print_usage() {
    eprintln!(
        "\
thoxa — native Thoxa compiler (compile-to-C)

USAGE:
    thoxa build  <file.thx> [-o name]
    thoxa run    <file.thx>
    thoxa emit-c <file.thx>
    thoxa runtime

Build products land under targets/."
    );
}
