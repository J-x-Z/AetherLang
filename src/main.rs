//! AetherLang compiler driver: one frontend for checking and building.
#[cfg(all(feature = "llvm", feature = "llvm23"))]
compile_error!("select one LLVM version: use the default LLVM 21 build, or use --no-default-features --features llvm23 for LLVM 23");

#[cfg(all(feature = "llvm23", not(feature = "llvm")))]
extern crate llvm_sys_23 as llvm_sys;

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
use frontend::{resolve::{ModuleSearch, Resolver}, session::{Session, Diagnostic}};
use middle::{ir_gen::IRGenerator, optimize::Optimizer, ir_printer::print_ir};
use backend::CCodeGen;
use driver_output::{write, OutputFile};
#[cfg(any(feature = "llvm", feature = "llvm23"))]
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
    /// Bind a module name to an exact .aeth source file (repeatable)
    #[arg(long = "module", value_name = "NAME=FILE", global = true)]
    modules: Vec<String>,
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

fn default_backend() -> &'static str { if cfg!(any(feature = "llvm", feature = "llvm23")) { "llvm" } else { "c" } }

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
    let llvm_backend = matches!(cli.backend, Backend::Llvm) && !cli.emit_c && !cli.emit_ir;
    let (input, check) = match &cli.command {
        Some(Commands::Build { input }) => (input, false),
        Some(Commands::Check { input }) => (input, true),
        Some(Commands::Lsp) => return lsp::serve(module_search(cli)?, cli.target.clone(), llvm_backend).map_err(Diagnostic::from),
        Some(Commands::Symbols { input }) => (input, true),
        Some(Commands::Version) => { println!("aethc {}", env!("CARGO_PKG_VERSION")); return Ok(()); }
        None => (cli.input.as_ref().ok_or("no input file; use aethc build <FILE> or aethc check <FILE>")?, false),
    };
    let source = fs::read_to_string(input).map_err(|e| format!("{}: {e}", input.display()))?;
    let session = Session::parse_for_target(input, source, module_search(cli)?, Default::default(), &cli.target)?;
    let facts = session.check(&cli.target, llvm_backend)?;
    if matches!(cli.command, Some(Commands::Symbols { .. })) {
        println!("{}", serde_json::to_string(&frontend::symbols::SymbolIndex::new(&session, Some(&facts))).expect("serializable symbols"));
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
    let target = effective_target(&cli.target)?;
    if cli.emit_c || matches!(cli.backend, Backend::C) {
        let c = CCodeGen::new(&target.triple).generate_source(&ir).map_err(|e| e.to_string())?;
        if cli.emit_c { write(&output, c.as_bytes())?; }
        else {
            let temp = Temporary::new("c", c.as_bytes())?;
            native_compile(cli, &target, &temp.path, &output, cli.emit_obj, &ir)?;
        }
    } else {
        #[cfg(any(feature = "llvm", feature = "llvm23"))]
        {
            let triple = target_triple(&target.triple);
            let mut codegen = backend::llvm::LLVMCodeGen::new(&triple).with_opt_level(cli.opt_level);
            if cli.emit_llvm {
                let text = codegen.generate_llvm_ir(&ir).map_err(|e| e.to_string())?;
                write(&output, text.as_bytes())?;
            } else {
                let bytes = codegen.generate(&ir).map_err(|e| e.to_string())?;
                if cli.emit_obj { write(&output, &bytes)?; }
                else {
                    let temp = Temporary::new("o", &bytes)?;
                    native_compile(cli, &target, &temp.path, &output, false, &ir)?;
                }
            }
        }
        #[cfg(not(any(feature = "llvm", feature = "llvm23")))]
        return Err("LLVM backend is not built; use --backend c, rebuild with --features llvm (LLVM 21), or rebuild with --no-default-features --features llvm23 (LLVM 23)".into());
    }
    if !cli.quiet { println!("Output: {}", output.display()); }
    Ok(())
}

fn module_search(cli: &Cli) -> Result<ModuleSearch, String> {
    let mut search = ModuleSearch::new(search_paths(cli)?);
    for specification in &cli.modules { search.add_module(specification)?; }
    Ok(search)
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
    // Darwin may retain executable or directory symlinks in current_exe().
    // Discover libraries beside the real tool, rather than beside its alias.
    let executable = std::env::current_exe().and_then(fs::canonicalize)
        .map_err(|e| format!("locating compiler installation: {e}"))?;
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
    // Only the actual Cargo profile executable may use developer sources.
    // In particular, copied installations/test prefixes anywhere below target/
    // must not silently borrow a different checkout's library.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let build_root = option_env!("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| manifest.join("target"));
    let build_root = if build_root.is_absolute() { build_root } else { manifest.join(build_root) };
    if let Ok(build_root) = fs::canonicalize(build_root) {
        let filename = format!("aethc{}", std::env::consts::EXE_SUFFIX);
        if ["debug", "release"].iter().any(|profile| {
            fs::canonicalize(build_root.join(profile))
                .is_ok_and(|directory| executable == directory.join(&filename))
        }) {
            paths.push(manifest.join("stdlib"));
        }
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

fn native_compile(cli: &Cli, target: &EffectiveTarget, input: &Path, output: &Path, object: bool, ir: &middle::ir::IRModule) -> Result<(), String> {
    let staged = OutputFile::new(output)?;
    let compilers = if let Some(cc) = cli.cc.clone().or_else(|| std::env::var_os("CC").map(PathBuf::from)) { vec![cc] }
        else { ["clang", "gcc", "cc"].iter().map(PathBuf::from).collect() };
    for compiler in &compilers {
        let mut command = Command::new(compiler);
        command.arg(format!("-O{}", cli.opt_level));
        backend::system_attributes::c_machine_flags(&mut command, &target.triple, ir);
        if target.pass_target { command.arg(format!("--target={}", target.triple)); }
        if let Some(version) = &target.macos_deployment {
            command.arg(format!("-mmacosx-version-min={version}"));
            // A versioned CLI target wins over a conflicting environment value.
            command.env("MACOSX_DEPLOYMENT_TARGET", version);
        }
        if let Some(sdk) = &target.sdk { command.arg("-isysroot").arg(sdk); }
        if object { command.arg("-c"); }
        command.arg(input).arg("-o").arg(&staged.path);
        if !object {
            if target.macos_deployment.is_some() && ir.has_packed_relocations() {
                command.arg("-Wl,-no_fixup_chains");
            }
            command.args(&cli.link_object);
            for library in &cli.link_library { command.arg(format!("-l{}", library)); }
        }
        if let Some(script) = &cli.linker_script { command.arg("-T").arg(script).arg("-nostdlib"); }
        match command.output() {
            Ok(result) if result.status.success() => {
                // Linkers can succeed while diagnosing incompatible objects.
                // Quiet suppresses progress, never meaningful tool diagnostics.
                if !result.stderr.is_empty() { eprint!("{}", String::from_utf8_lossy(&result.stderr)); }
                return staged.publish(output);
            },
            Ok(result) => return Err(format!("{} failed ({}):\n{}{}", compiler.display(), result.status,
                String::from_utf8_lossy(&result.stdout), String::from_utf8_lossy(&result.stderr))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("starting {}: {e}", compiler.display())),
        }
    }
    Err(format!("no C compiler found (tried {}); set --cc or CC", compilers.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")))
}

#[cfg(any(feature = "llvm", feature = "llvm23"))]
fn target_triple(target: &str) -> String {
    if target != "native" { return target.into(); }
    unsafe {
        let raw = llvm_sys::target_machine::LLVMGetDefaultTargetTriple();
        let target = std::ffi::CStr::from_ptr(raw).to_string_lossy().into_owned();
        llvm_sys::core::LLVMDisposeMessage(raw);
        target
    }
}

/// One macOS deployment choice is shared by code generation and the C driver.
/// Darwin kernel versions are not macOS deployment versions; never derive one
/// from LLVM's host triple (which can be newer than the selected SDK).
struct EffectiveTarget {
    triple: String,
    pass_target: bool,
    macos_deployment: Option<String>,
    sdk: Option<PathBuf>,
}

fn deployment_version(value: &str) -> Result<String, String> {
    let parts = value.split('.').collect::<Vec<_>>();
    if parts.is_empty() || parts.len() > 3 || parts.iter().any(|part| part.is_empty()
        || !part.bytes().all(|byte| byte.is_ascii_digit()) || part.parse::<u16>().is_err()) {
        return Err(format!("invalid macOS deployment version '{value}'; use a numeric version such as 13.0"));
    }
    let mut numbers = parts.iter().map(|part| part.parse::<u16>().unwrap()).collect::<Vec<_>>();
    if numbers[0] < 10 { return Err(format!("invalid macOS deployment version '{value}'")); }
    while numbers.len() < 2 { numbers.push(0); }
    Ok(numbers.iter().map(u16::to_string).collect::<Vec<_>>().join("."))
}

fn macos_target(requested: &str) -> Result<Option<(String, Option<String>)>, String> {
    if requested == "native" {
        return Ok(if cfg!(target_os = "macos") {
            Some((format!("{}-apple-macosx", std::env::consts::ARCH), None))
        } else { None });
    }
    let Some((prefix, system)) = requested.split_once("-apple-") else { return Ok(None); };
    let version = if let Some(version) = system.strip_prefix("macosx") { version }
        else if let Some(version) = system.strip_prefix("macos") { version }
        else if system == "darwin" { "" }
        else if system.starts_with("darwin") {
            return Err("a versioned Darwin kernel triple is ambiguous for macOS deployment; use --target <arch>-apple-macosx<version>".into());
        } else { return Ok(None); };
    Ok(Some((format!("{prefix}-apple-macosx"), if version.is_empty() { None } else { Some(deployment_version(version)?) })))
}

/// Resolve standard SDK paths without spawning xcrun for every compilation.
/// Explicit SDKROOT/DEVELOPER_DIR and xcode-select's on-disk selection win.
fn macos_sdk() -> Result<Option<PathBuf>, String> {
    if let Some(root) = std::env::var_os("SDKROOT").filter(|value| !value.is_empty()) {
        let root = PathBuf::from(root);
        if !root.is_dir() { return Err(format!("SDKROOT {} is not an SDK directory", root.display())); }
        return Ok(Some(root));
    }
    let developer = std::env::var_os("DEVELOPER_DIR").filter(|value| !value.is_empty()).map(PathBuf::from)
        .or_else(|| fs::read_link("/var/db/xcode_select_link").ok());
    let developer = developer.map(|path| {
        if path.extension().is_some_and(|extension| extension == "app") { path.join("Contents/Developer") } else { path }
    });
    let candidates = if let Some(developer) = developer {
        vec![developer.join("Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk"), developer.join("SDKs/MacOSX.sdk")]
    } else if cfg!(target_os = "macos") {
        vec![PathBuf::from("/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk")]
    } else { vec![] };
    Ok(candidates.into_iter().find(|path| path.is_dir()))
}

fn sdk_deployment(sdk: &Path) -> Result<Option<String>, String> {
    let settings = sdk.join("SDKSettings.json");
    let text = match fs::read_to_string(&settings) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("reading {}: {error}", settings.display())),
    };
    let settings: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| format!("reading {}: {error}", settings.display()))?;
    if let Some(targets) = settings.get("SupportedTargets") {
        if targets.get("macosx").is_none() && targets.get("macos").is_none() {
            return Err(format!("SDK {} does not support a macOS target", sdk.display()));
        }
    }
    let version = settings.pointer("/SupportedTargets/macosx/DefaultDeploymentTarget")
        .or_else(|| settings.pointer("/SupportedTargets/macos/DefaultDeploymentTarget"))
        .or_else(|| settings.get("DefaultDeploymentTarget"))
        .or_else(|| settings.pointer("/DefaultProperties/MACOSX_DEPLOYMENT_TARGET"))
        .and_then(serde_json::Value::as_str);
    version.map(deployment_version).transpose()
}

fn effective_target(requested: &str) -> Result<EffectiveTarget, String> {
    let Some((prefix, explicit)) = macos_target(requested)? else {
        return Ok(EffectiveTarget { triple: requested.into(), pass_target: requested != "native", macos_deployment: None, sdk: None });
    };
    let sdk = macos_sdk()?;
    // Validate platform compatibility even when deployment was selected explicitly.
    let sdk_default = sdk.as_deref().map(sdk_deployment).transpose()?.flatten();
    let version = if let Some(version) = explicit { version }
        else if let Some(version) = std::env::var_os("MACOSX_DEPLOYMENT_TARGET").filter(|value| !value.is_empty()) {
            deployment_version(version.to_str().ok_or("MACOSX_DEPLOYMENT_TARGET must be a numeric UTF-8 version")?)?
        } else {
            sdk_default.ok_or("cannot determine macOS deployment target; set MACOSX_DEPLOYMENT_TARGET, use a versioned --target, or select a macOS SDK with SDKROOT")?
        };
    if (prefix.starts_with("aarch64-") || prefix.starts_with("arm64-")) && version.split('.').next().unwrap().parse::<u16>().unwrap() < 11 {
        return Err("macOS arm64 requires deployment target 11.0 or later".into());
    }
    Ok(EffectiveTarget { triple: format!("{prefix}{version}"), pass_target: requested != "native", macos_deployment: Some(version), sdk })
}

#[cfg(test)]
mod target_tests {
    use super::*;
    #[test]
    fn macos_deployments_are_distinct_from_darwin_kernel_versions() {
        assert_eq!(macos_target("arm64-apple-macosx13.2.1").unwrap(), Some(("arm64-apple-macosx".into(), Some("13.2.1".into()))));
        assert_eq!(macos_target("x86_64-apple-macos12").unwrap(), Some(("x86_64-apple-macosx".into(), Some("12.0".into()))));
        assert_eq!(macos_target("aarch64-apple-darwin").unwrap(), Some(("aarch64-apple-macosx".into(), None)));
        assert!(macos_target("aarch64-apple-darwin27.0").is_err());
        for target in ["aarch64-unknown-linux-gnu", "aarch64-apple-ios18.0", "x86_64-pc-windows-msvc"] {
            assert!(macos_target(target).unwrap().is_none());
        }
        for version in ["", "13..0", "13.0beta", "13.0.0.1", "999999", "-13", "0"] {
            assert!(deployment_version(version).is_err(), "{version}");
        }
    }
}
