//! AetherLang compiler driver: one frontend for checking and building.
mod intrinsics;
mod frontend;
mod middle;
mod backend;
mod types;
mod utils;
mod stdlib;
mod ai_ir;
mod feedback;
mod lsp;
mod script;
mod driver_output;

use clap::{Parser, Subcommand, ValueEnum};
use std::{fs, path::{Path, PathBuf}, process::{self, Command}, sync::atomic::{AtomicU64, Ordering}};
use frontend::{resolve::Resolver, session::{Session, Diagnostic}};
use middle::{ir_gen::IRGenerator, optimize::Optimizer, ir_printer::print_ir};
use backend::CCodeGen;
use driver_output::{write, OutputFile};
#[cfg(feature = "llvm")]
use backend::codegen::CodeGen;

#[derive(Clone, Copy, Debug, ValueEnum)]
enum Backend { C, Llvm }

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ErrorFormat { Human, Json }

#[derive(Parser, Debug)]
#[command(name = "aethc", version, about = "AetherLang compiler")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
    #[arg(value_name = "FILE")]
    input: Option<PathBuf>,
    /// Output file (also applies to --emit-ir and --emit-c)
    #[arg(short, long, global = true)]
    output: Option<PathBuf>,
    #[arg(long, global = true, conflicts_with_all = ["emit_ir", "emit_obj", "emit_llvm"])]
    emit_c: bool,
    #[arg(long, global = true, conflicts_with_all = ["emit_obj", "emit_llvm"])]
    emit_ir: bool,
    /// Emit an object file without linking
    #[arg(long, global = true)]
    emit_obj: bool,
    /// Emit verified LLVM IR after optimization (requires the LLVM backend)
    #[arg(long, global = true, conflicts_with = "emit_obj")]
    emit_llvm: bool,
    #[arg(short = 'O', default_value_t = 0, global = true, value_parser = clap::value_parser!(u8).range(0..=3))]
    opt_level: u8,
    #[arg(long, value_enum, default_value = default_backend(), global = true)]
    backend: Backend,
    #[arg(long, default_value = "native", global = true)]
    target: String,
    #[arg(long, global = true)]
    linker_script: Option<PathBuf>,
    /// Additional module search directory (repeatable)
    #[arg(short = 'I', long = "module-path", global = true)]
    module_paths: Vec<PathBuf>,
    /// Installation prefix containing stdlib/ or lib/aetherlang/stdlib/
    #[arg(long, global = true)]
    sysroot: Option<PathBuf>,
    /// C compiler/linker executable; defaults to CC, then clang/gcc/cc
    #[arg(long, global = true)]
    cc: Option<PathBuf>,
    #[arg(short, long, global = true)]
    quiet: bool,
    /// Diagnostic output format (stable codes and UTF-8 spans in JSON mode)
    #[arg(long, value_enum, default_value = "human", global = true)]
    error_format: ErrorFormat,
    /// Additional native object or archive to link (repeatable)
    #[arg(long, global = true)]
    link_object: Vec<PathBuf>,
    /// Native library name passed to the linker (repeatable)
    #[arg(short = 'l', long, global = true)]
    link_library: Vec<String>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Compile and link a source file (.aeth or .ath)
    Build { input: PathBuf },
    /// Parse, resolve imports and type-check without generating code
    Check { input: PathBuf },
    /// Emit resolved symbols and references as JSON
    Symbols { input: PathBuf },
    /// Serve the Language Server Protocol over stdin/stdout
    Lsp,
    Version,
}

fn default_backend() -> &'static str { if cfg!(feature = "llvm") { "llvm" } else { "c" } }

fn main() {
    env_logger::init();
    let cli = Cli::parse();
    if let Err(error) = run(&cli) {
        match cli.error_format {
            ErrorFormat::Human => eprintln!("error: {}", error.render()),
            ErrorFormat::Json => eprintln!("{}", serde_json::to_string(&error).expect("serializable diagnostic")),
        }
        process::exit(1);
    }
}

fn run(cli: &Cli) -> Result<(), Diagnostic> {
    let (input, check) = match &cli.command {
        Some(Commands::Build { input }) => (input, false),
        Some(Commands::Check { input }) => (input, true),
        Some(Commands::Lsp) => return lsp::serve(search_paths(cli)?).map_err(Diagnostic::from),
        Some(Commands::Symbols { input }) => (input, true),
        Some(Commands::Version) => { println!("aethc {}", env!("CARGO_PKG_VERSION")); return Ok(()); }
        None => (cli.input.as_ref().ok_or("no input file; use aethc build <FILE> or aethc check <FILE>")?, false),
    };
    let source = fs::read_to_string(input).map_err(|e| format!("{}: {e}", input.display()))?;
    let session = Session::parse(input, source, search_paths(cli)?, Default::default())?;
    let facts = session.check()?;
    if matches!(cli.command, Some(Commands::Symbols { .. })) {
        println!("{}", serde_json::to_string(&frontend::symbols::SymbolIndex::new(&session)).expect("serializable symbols"));
        return Ok(());
    }
    if check {
        if !cli.quiet && matches!(cli.error_format, ErrorFormat::Human) { println!("Checked {}", input.display()); }
        return Ok(());
    }
    let resolver = &session.resolver;
    let mut ir = IRGenerator::new(input.file_stem().and_then(|s| s.to_str()).unwrap_or("module"))
        .with_semantic_facts(facts)
        .generate(&session.program).map_err(|e| session.diagnostic(&e))?;
    if cli.opt_level > 0 { Optimizer::new().optimize(&mut ir); }
    if cli.emit_ir {
        let output = cli.output.clone().unwrap_or_else(|| input.with_extension("ir"));
        protect_sources(&resolver, &output)?;
        write(&output, print_ir(&ir).as_bytes())?;
        if !cli.quiet { println!("IR: {}", output.display()); }
        return Ok(());
    }
    let output = cli.output.clone().unwrap_or_else(|| {
        if cli.emit_c { input.with_extension("c") }
        else if cli.emit_obj { input.with_extension("o") }
        else if cli.emit_llvm { input.with_extension("ll") }
        else { input.with_extension(if cfg!(windows) { "exe" } else { "" }) }
    });
    protect_sources(&resolver, &output)?;
    if cli.emit_llvm && matches!(cli.backend, Backend::C) {
        return Err("--emit-llvm requires --backend llvm".into());
    }
    if cli.emit_c || matches!(cli.backend, Backend::C) {
        let c = CCodeGen::new(&cli.target).generate_source(&ir).map_err(|e| e.to_string())?;
        if cli.emit_c { write(&output, c.as_bytes())?; }
        else {
            let temp = Temporary::new("c", c.as_bytes())?;
            native_compile(cli, &temp.path, &output, cli.emit_obj)?;
        }
    } else {
        #[cfg(feature = "llvm")]
        {
            let target = target_triple(&cli.target);
            let mut codegen = backend::llvm::LLVMCodeGen::new(&target).with_opt_level(cli.opt_level);
            if cli.emit_llvm {
                let text = codegen.generate_llvm_ir(&ir).map_err(|e| e.to_string())?;
                write(&output, text.as_bytes())?;
            } else {
                let bytes = codegen.generate(&ir).map_err(|e| e.to_string())?;
                if cli.emit_obj { write(&output, &bytes)?; }
                else {
                    let temp = Temporary::new("o", &bytes)?;
                    native_compile(cli, &temp.path, &output, false)?;
                }
            }
        }
        #[cfg(not(feature = "llvm"))]
        return Err("LLVM backend is not built; rebuild with --features llvm or use --backend c".into());
    }
    if !cli.quiet { println!("Output: {}", output.display()); }
    Ok(())
}

fn search_paths(cli: &Cli) -> Result<Vec<PathBuf>, String> {
    let mut paths = cli.module_paths.clone();
    if let Some(env) = std::env::var_os("AETHER_PATH") { paths.extend(std::env::split_paths(&env)); }
    let library_at = |prefix: &Path| {
        [prefix.join("stdlib"), prefix.join("lib/aetherlang/stdlib")]
            .into_iter().find(|path| path.is_dir())
    };
    if let Some(prefix) = &cli.sysroot {
        let library = library_at(prefix).ok_or_else(|| format!(
            "sysroot {} has no stdlib/ or lib/aetherlang/stdlib/ directory", prefix.display()))?;
        paths.push(library);
        return Ok(paths);
    }
    let executable = std::env::current_exe().map_err(|e| format!("locating compiler installation: {e}"))?;
    if let Some(directory) = executable.parent() {
        let installed = library_at(directory).or_else(|| {
            if directory.file_name().is_some_and(|name| name == "bin") {
                directory.parent().and_then(library_at)
            } else { None }
        });
        if let Some(library) = installed {
            paths.push(library);
            return Ok(paths);
        }
    }
    // Only binaries in their actual Cargo build tree may use developer sources.
    // A relocated installation must never silently borrow a different checkout.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let build_root = option_env!("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| manifest.join("target"));
    let build_root = if build_root.is_absolute() { build_root } else { manifest.join(build_root) };
    if let Ok(build_root) = fs::canonicalize(build_root) {
        if executable.starts_with(build_root) { paths.push(manifest.join("stdlib")); }
    }
    Ok(paths)
}

fn protect_sources(resolver: &Resolver, output: &Path) -> Result<(), String> {
    for source in &resolver.sources {
        let same_path = source.path == output || match (fs::canonicalize(&source.path), fs::canonicalize(output)) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        };
        #[cfg(unix)]
        let same_file = {
            use std::os::unix::fs::MetadataExt;
            match (fs::metadata(&source.path), fs::metadata(output)) {
                (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
                _ => false,
            }
        };
        #[cfg(not(unix))]
        let same_file = false;
        if same_path || same_file {
            return Err(format!("output must not overwrite an input source: {}", source.path.display()));
        }
    }
    Ok(())
}

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
struct Temporary { path: PathBuf }
impl Temporary {
    fn new(extension: &str, bytes: &[u8]) -> Result<Self, String> {
        use std::io::Write;
        for _ in 0..100 {
            let path = std::env::temp_dir().join(format!("aethc-{}-{}.{}", process::id(), NEXT_TEMP.fetch_add(1, Ordering::Relaxed), extension));
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)] { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
            match options.open(&path) {
                Ok(mut f) => { let temporary = Self { path }; f.write_all(bytes).map_err(|e| e.to_string())?; return Ok(temporary); }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(format!("creating compiler temporary file: {e}")),
            }
        }
        Err("cannot allocate a unique compiler temporary file".into())
    }
}
impl Drop for Temporary { fn drop(&mut self) { let _ = fs::remove_file(&self.path); } }

fn native_compile(cli: &Cli, input: &Path, output: &Path, object: bool) -> Result<(), String> {
    let staged = OutputFile::new(output)?;
    let compilers = if let Some(cc) = cli.cc.clone().or_else(|| std::env::var_os("CC").map(PathBuf::from)) { vec![cc] }
        else { ["clang", "gcc", "cc"].iter().map(PathBuf::from).collect() };
    for compiler in &compilers {
        let mut command = Command::new(compiler);
        command.arg(format!("-O{}", cli.opt_level));
        if cli.target != "native" { command.arg(format!("--target={}", cli.target)); }
        if object { command.arg("-c"); }
        command.arg(input).arg("-o").arg(&staged.path);
        if !object {
            command.args(&cli.link_object);
            for library in &cli.link_library { command.arg(format!("-l{}", library)); }
        }
        if let Some(script) = &cli.linker_script { command.arg("-T").arg(script).arg("-nostdlib"); }
        match command.output() {
            Ok(result) if result.status.success() => return staged.publish(output),
            Ok(result) => return Err(format!("{} failed ({}):\n{}{}", compiler.display(), result.status,
                String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("starting {}: {e}", compiler.display())),
        }
    }
    Err(format!("no C compiler found (tried {}); set --cc or CC", compilers.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")))
}

#[cfg(feature = "llvm")]
fn target_triple(target: &str) -> String {
    if target != "native" { return target.into(); }
    unsafe {
        let raw = llvm_sys::target_machine::LLVMGetDefaultTargetTriple();
        let target = std::ffi::CStr::from_ptr(raw).to_string_lossy().into_owned();
        llvm_sys::core::LLVMDisposeMessage(raw);
        target
    }
}
