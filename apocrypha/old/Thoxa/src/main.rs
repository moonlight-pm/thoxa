use clap::Parser;
use miette::{Context, IntoDiagnostic, Result};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use tempfile::NamedTempFile;

#[derive(Parser)]
#[command(name = "thoxa")]
#[command(about = "The Thoxa programming language compiler", long_about = None)]
struct Cli {
    /// Source file to compile
    file: PathBuf,

    /// Compile to executable (instead of compile-and-run)
    #[arg(short = 'c', long)]
    compile: bool,

    /// Output file name (default: a.out when compiling)
    #[arg(short = 'o', long)]
    output: Option<PathBuf>,

    /// Arguments to pass to the program (when running)
    #[arg(trailing_var_arg = true)]
    args: Vec<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.compile {
        // Compile mode: generate an executable
        compile_file(&cli.file, cli.output)?;
    } else {
        // Run mode: compile to temp file and execute
        run_file(&cli.file, &cli.args)?;
    }
    
    Ok(())
}

/// Compile and run a Thoxa file
fn run_file(input_path: &PathBuf, args: &[String]) -> Result<()> {
    // Create a temporary executable file
    let temp_exe = NamedTempFile::new()
        .into_diagnostic()
        .context("Failed to create temporary file")?;
    
    let temp_path = temp_exe.path().to_path_buf();
    
    // Compile to the temporary file (without printing messages)
    compile_to_executable(input_path, &temp_path, false)?;
    
    // Make the file executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&temp_path)
            .into_diagnostic()?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&temp_path, perms)
            .into_diagnostic()?;
    }
    
    // Execute the compiled program
    let mut cmd = Command::new(&temp_path);
    cmd.args(args);
    cmd.stdin(Stdio::inherit());
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());
    
    let status = cmd.status()
        .into_diagnostic()
        .context("Failed to execute compiled program")?;
    
    // Exit with the same code as the program
    std::process::exit(status.code().unwrap_or(1));
}

/// Compile a Thoxa file to an executable
fn compile_file(input_path: &PathBuf, output_path: Option<PathBuf>) -> Result<()> {
    let output_file = output_path.unwrap_or_else(|| PathBuf::from("a.out"));
    compile_to_executable(input_path, &output_file, true)?;
    Ok(())
}

/// Core compilation logic
fn compile_to_executable(
    input_path: &PathBuf, 
    output_path: &PathBuf, 
    verbose: bool
) -> Result<()> {
    // Read source file
    let source = fs::read_to_string(input_path)
        .into_diagnostic()
        .context(format!("Failed to read file: {:?}", input_path))?;
    
    // Parse
    let mut parser = thoxa::parser::Parser::new(&source)
        .map_err(|e| miette::miette!("Failed to create parser: {}", e))?;
    
    let module = parser.parse_module()
        .map_err(|e| miette::miette!("Parse error: {}", e))?;
    
    // Generate QBE IR
    let codegen = thoxa::codegen::CodeGen::new();
    let qbe_ir = codegen.generate(&module)
        .map_err(|e| miette::miette!("Code generation error: {}", e))?;
    
    // Create temporary files for intermediate stages
    let (qbe_file, _qbe_temp);
    if verbose {
        qbe_file = input_path.with_extension("qbe");
        _qbe_temp = None;
    } else {
        let temp = NamedTempFile::with_suffix(".qbe")
            .into_diagnostic()?;
        qbe_file = temp.path().to_path_buf();
        _qbe_temp = Some(temp);
    }
    
    fs::write(&qbe_file, &qbe_ir)
        .into_diagnostic()
        .context("Failed to write QBE file")?;
    
    if verbose {
        println!("Generated QBE IR: {:?}", qbe_file);
    }
    
    // Check if qbe is available
    if which::which("qbe").is_ok() {
        if verbose {
            println!("Found QBE compiler, generating assembly...");
        }
        
        // Generate assembly with QBE
        let (asm_file, _asm_temp);
        if verbose {
            asm_file = input_path.with_extension("s");
            _asm_temp = None;
        } else {
            let temp = NamedTempFile::with_suffix(".s")
                .into_diagnostic()?;
            asm_file = temp.path().to_path_buf();
            _asm_temp = Some(temp);
        }
        
        let qbe_output = Command::new("qbe")
            .arg(&qbe_file)
            .output()
            .into_diagnostic()
            .context("Failed to run QBE")?;
        
        if !qbe_output.status.success() {
            let stderr = String::from_utf8_lossy(&qbe_output.stderr);
            return Err(miette::miette!("QBE compilation failed:\n{}", stderr));
        }
        
        // Write the assembly output to file
        fs::write(&asm_file, &qbe_output.stdout)
            .into_diagnostic()
            .context("Failed to write assembly file")?;
        
        if verbose {
            println!("Generated assembly: {:?}", asm_file);
        }
        
        // Link with system C compiler and runtime library
        let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
        
        // Find the runtime libraries - for now use simple paths
        let runtime_path = PathBuf::from("target/runtime/thoxa_runtime.o");
        let memory_path = PathBuf::from("target/runtime/thoxa_memory.o");
        let trial_path = PathBuf::from("target/runtime/thoxa_trial.o");
        let box_path = PathBuf::from("target/runtime/thoxa_box.o");
        let box_unified_path = PathBuf::from("target/runtime/thoxa_box_unified.o");
        let types_path = PathBuf::from("target/runtime/thoxa_types.o");
        let string_path = PathBuf::from("target/runtime/thoxa_string.o");
        let error_path = PathBuf::from("target/runtime/thoxa_error.o");
        let may_path = PathBuf::from("target/runtime/thoxa_may.o");
        let union_path = PathBuf::from("target/runtime/thoxa_union.o");
        let runtime_exists = runtime_path.exists();
        let memory_exists = memory_path.exists();
        let trial_exists = trial_path.exists();
        let box_exists = box_path.exists();
        let box_unified_exists = box_unified_path.exists();
        let types_exists = types_path.exists();
        let string_exists = string_path.exists();
        let error_exists = error_path.exists();
        let may_exists = may_path.exists();
        let union_exists = union_path.exists();
        
        let mut link_cmd = Command::new(&cc);
        link_cmd.arg(&asm_file);
        
        // Add runtime libraries if found
        if runtime_exists {
            link_cmd.arg(&runtime_path);
        }
        if memory_exists {
            link_cmd.arg(&memory_path);
        }
        if trial_exists {
            link_cmd.arg(&trial_path);
        }
        if box_exists {
            link_cmd.arg(&box_path);
        }
        if box_unified_exists {
            link_cmd.arg(&box_unified_path);
        }
        if types_exists {
            link_cmd.arg(&types_path);
        }
        if string_exists {
            link_cmd.arg(&string_path);
        }
        if error_exists {
            link_cmd.arg(&error_path);
        }
        if may_exists {
            link_cmd.arg(&may_path);
        }
        if union_exists {
            link_cmd.arg(&union_path);
        }
        
        link_cmd.arg("-o").arg(&output_path);
        
        let link_output = link_cmd
            .output()
            .into_diagnostic()
            .context("Failed to link executable")?;
        
        if !link_output.status.success() {
            let stderr = String::from_utf8_lossy(&link_output.stderr);
            return Err(miette::miette!("Linking failed:\n{}", stderr));
        }
        
        if verbose {
            println!("Successfully compiled to: {:?}", output_path);
        }
        
        // Temporary files are automatically cleaned up when dropped
    } else {
        if verbose {
            println!("QBE compiler not found. Install QBE to generate native executables.");
            println!("QBE IR saved to: {:?}", qbe_file);
            println!("\nTo compile manually:");
            println!("  qbe {} -o {}.s", qbe_file.display(), input_path.display());
            println!("  cc {}.s -o {}", input_path.display(), output_path.display());
        } else {
            return Err(miette::miette!("QBE compiler not found. Install QBE to run Thoxa programs."));
        }
    }
    
    Ok(())
}
