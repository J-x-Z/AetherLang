//! AetherLang Compiler
//! 
//! A self-hosting systems programming language.

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

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::fs;
use std::process;

use frontend::lexer::Lexer;
use frontend::parser::Parser as AethParser;
use frontend::semantic::SemanticAnalyzer;
use middle::ir_gen::IRGenerator;
use middle::optimize::Optimizer;
use middle::ir_printer::print_ir;
use backend::{CCodeGen, codegen::CodeGen};

/// AetherLang Compiler
#[derive(Parser, Debug)]
#[command(name = "aethc")]
#[command(author = "Z1529")]
#[command(version = "0.1.0")]
#[command(about = "AetherLang compiler - A self-hosting systems programming language")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Input source file (.aeth)
    #[arg(value_name = "FILE")]
    input: Option<PathBuf>,

    /// Output file
    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,

    /// Emit C code instead of binary
    #[arg(long)]
    emit_c: bool,

    /// Emit Aether IR (for debugging)
    #[arg(long)]
    emit_ir: bool,

    /// Optimization level (0-3)
    #[arg(short = 'O', default_value = "0")]
    opt_level: u8,

    /// Backend to use (c, llvm)
    #[arg(long, default_value = "c")]
    backend: String,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Compile a source file
    Build {
        /// Input source file
        input: PathBuf,
        
        /// Output file
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Check a source file for errors
    Check {
        /// Input source file
        input: PathBuf,
    },
    /// Print version information
    Version,
    
    /// (Hidden) Test Linker
    #[command(hide = true)]
    LinkTest {
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

fn main() {
    env_logger::init();
    
    let cli = Cli::parse();
    
    // Handle subcommands
    match &cli.command {
        Some(Commands::Build { input, output }) => {
            compile_file(input, output.clone(), &cli);
        }
        Some(Commands::Check { input }) => {
            check_file(input);
        }
        Some(Commands::Version) => {
            println!("aethc 0.1.0");
            println!("AetherLang Compiler");
            println!("License: Apache-2.0");
        }
        Some(Commands::LinkTest { output }) => {
            use backend::linker::{Linker, PF_R, PF_X, SHT_PROGBITS, SHF_ALLOC, SHF_EXECINSTR};
            
            println!("Testing Self-Hosted Linker...");
            let mut linker = Linker::new();
            
            // Minimal shellcode: exit(42)
            let code = vec![
                0x48, 0xc7, 0xc7, 0x2a, 0x00, 0x00, 0x00, // mov rdi, 42
                0x48, 0xc7, 0xc0, 0x3c, 0x00, 0x00, 0x00, // mov rax, 60
                0x0f, 0x05                                // syscall
            ];
            
            // Add as .text section
            // In our current simple implementation, segments and sections must be added in sync
            // or the linker should auto-manage segments.
            // For now, we manually manage both but let's try to be consistent.
            // Our emit logic assumes Sections == Segments 1:1 for the loop.
            // So we add text segment AND text section.
            
            let vaddr = 0x400000 + 0x40;
            linker.add_segment(code.clone(), PF_R | PF_X, vaddr);
            linker.add_section(".text", code, SHT_PROGBITS, SHF_ALLOC | SHF_EXECINSTR, vaddr);
            
            linker.set_entry_point(vaddr);
            
            // In a real implementation, we would calculate exact header size.
            // For this test, we rely on the linker's emit function handling offsets,
            // but the virtual mapping must match.
            // Let's rely on simple PT_LOAD at 0x400000 mapping the whole file?
            // Actually, our simple linker appends segments AFTER headers.
            // So code will be at file offset ~120 bytes.
            // We need to be careful with vaddr.
            
            let out_path = output.clone().unwrap_or_else(|| PathBuf::from("test_elf"));
            if let Err(e) = linker.emit(&out_path) {
                eprintln!("Linker error: {}", e);
            } else {
                println!("Generated ELF: {}", out_path.display());
            }
        }
        None => {
            // Default: compile the input file
            if let Some(ref input) = cli.input {
                compile_file(input, cli.output.clone(), &cli);
            } else {
                eprintln!("Error: No input file specified");
                eprintln!("Usage: aethc <FILE> or aethc build <FILE>");
                process::exit(1);
            }
        }
    }
}

/// Compile a source file (.aeth or .ath)
fn compile_file(input: &PathBuf, output: Option<PathBuf>, cli: &Cli) {
    println!("AetherLang Compiler v0.1.0");
    println!("Compiling: {}", input.display());
    
    // 1. Read source file
    let source = match fs::read_to_string(input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading file: {}", e);
            process::exit(1);
        }
    };
    
    // 1.5. Check if Aether Script (.ath) - Transpile to Core first
    let core_source = if input.extension().map(|e| e == "ath").unwrap_or(false) {
        println!("  [Script] Detected Aether Script (.ath)");
        
        // Parse Script
        let mut script_parser = script::parser::Parser::new(&source);
        let script_module = match script_parser.parse() {
            Ok(m) => m,
            Err(e) => {
                eprintln!("Script Parse error: {}", e);
                process::exit(1);
            }
        };
        println!("  [✓] Script parsed ({} statements)", script_module.stmts.len());
        
        // Transpile to Core with source mapping enabled
        let source_path = input.to_string_lossy().to_string();
        let mut transpiler = script::transpiler::Transpiler::new()
            .with_source_file(&source_path);
        let generated = transpiler.transpile(&script_module);
        println!("  [✓] Transpiled to Aether Core ({} bytes)", generated.len());
        
        // Optionally write generated .aeth to disk for debugging
        let gen_path = input.with_extension("gen.aeth");
        if let Err(e) = fs::write(&gen_path, &generated) {
            eprintln!("  [!] Could not write generated Core: {}", e);
        } else {
            println!("  [→] Generated Core written to: {}", gen_path.display());
        }
        
        generated
    } else {
        source
    };
    
    // 2. Lexer -> Tokens (using Core source)
    let lexer = Lexer::new(&core_source, 0);
    
    // 3. Parser -> AST
    let mut parser = AethParser::new(lexer);
    let program = match parser.parse_program() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Parse error: {}", e);
            process::exit(1);
        }
    };
    println!("  [✓] Parsed {} items", program.items.len());
    
    // 4. Semantic Analysis -> Typed AST
    let mut analyzer = SemanticAnalyzer::new();
    if let Err(e) = analyzer.analyze(&program) {
        eprintln!("Semantic error: {}", e);
        process::exit(1);
    }
    println!("  [✓] Semantic analysis passed");
    
    // 5. IR Generation -> Aether IR
    let module_name = input.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("module");
    let mut ir_gen = IRGenerator::new(module_name);
    let mut ir_module = match ir_gen.generate(&program) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("IR generation error: {}", e);
            process::exit(1);
        }
    };
    println!("  [✓] Generated IR ({} functions)", ir_module.functions.len());
    
    // Emit IR if requested
    if cli.emit_ir {
        let ir_text = print_ir(&ir_module);
        let ir_path = input.with_extension("ir");
        if let Err(e) = fs::write(&ir_path, &ir_text) {
            eprintln!("Error writing IR: {}", e);
        } else {
            println!("  [✓] Wrote IR to {}", ir_path.display());
        }
        println!("\n{}", ir_text);
        return;
    }
    
    // 6. Optimization -> Optimized IR
    if cli.opt_level > 0 {
        let mut optimizer = Optimizer::new();
        optimizer.optimize(&mut ir_module);
        println!("  [✓] Optimized (level {})", cli.opt_level);
    }
    
    // 7. Code Generation
    match cli.backend.as_str() {
        "c" => {
            let mut codegen = CCodeGen::new("x86_64-pc-windows-msvc");
            
            // Generate C source
            let c_source = match codegen.generate_source(&ir_module) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Code generation error: {}", e);
                    process::exit(1);
                }
            };
            
            if cli.emit_c {
                // Just output C code
                let c_path = output.unwrap_or_else(|| input.with_extension("c"));
                if let Err(e) = fs::write(&c_path, &c_source) {
                    eprintln!("Error writing C file: {}", e);
                    process::exit(1);
                }
                println!("  [✓] Generated C code: {}", c_path.display());
            } else {
                // Compile C code to executable
                let _obj_path = input.with_extension("o");
                let exe_path = output.unwrap_or_else(|| {
                    #[cfg(windows)]
                    { input.with_extension("exe") }
                    #[cfg(not(windows))]
                    { input.with_extension("") }
                });
                
                // Write C source
                let c_path = input.with_extension("c");
                if let Err(e) = fs::write(&c_path, &c_source) {
                    eprintln!("Error writing C file: {}", e);
                    process::exit(1);
                }
                
                // Compile with clang/gcc
                let compilers = ["clang", "gcc", "cc"];
                let mut compiled = false;
                
                for compiler in &compilers {
                    let result = std::process::Command::new(compiler)
                        .args(&["-o"])
                        .arg(&exe_path)
                        .arg(&c_path)
                        .output();
                    
                    if let Ok(output) = result {
                        if output.status.success() {
                            compiled = true;
                            println!("  [✓] Compiled with {}", compiler);
                            break;
                        }
                    }
                }
                
                // Cleanup temp C file
                let _ = fs::remove_file(&c_path);
                
                if !compiled {
                    eprintln!("Error: Could not find C compiler (clang/gcc)");
                    process::exit(1);
                }
                
                println!("\n✅ Output: {}", exe_path.display());
            }
        }
        #[cfg(feature = "llvm")]
        "llvm" => {
            use backend::llvm::LLVMCodeGen;
            // Use native target triple for current platform
            #[cfg(target_os = "macos")]
            let mut codegen = LLVMCodeGen::new("arm64-apple-darwin");
            #[cfg(not(target_os = "macos"))]
            let mut codegen = LLVMCodeGen::new("x86_64-unknown-linux-gnu");
            
            match codegen.generate(&ir_module) {
                Ok(bytes) => {
                    let obj_path = output.unwrap_or_else(|| input.with_extension("o"));
                    if let Err(e) = fs::write(&obj_path, &bytes) {
                        eprintln!("Error writing object file: {}", e);
                        process::exit(1);
                    }
                    println!("  [✓] Generated object file: {}", obj_path.display());
                }
                Err(e) => {
                    eprintln!("LLVM code generation error: {}", e);
                    process::exit(1);
                }
            }
        }
        _ => {
            eprintln!("Unknown backend: {}. Use 'c' or 'llvm'", cli.backend);
            process::exit(1);
        }
    }
}

/// Check a source file for errors without generating code
fn check_file(input: &PathBuf) {
    println!("Checking: {}", input.display());
    
    let source = match fs::read_to_string(input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error reading file: {}", e);
            process::exit(1);
        }
    };
    
    let lexer = Lexer::new(&source, 0);
    let mut parser = AethParser::new(lexer);
    
    let program = match parser.parse_program() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Parse error: {}", e);
            process::exit(1);
        }
    };
    
    let mut analyzer = SemanticAnalyzer::new();
    if let Err(e) = analyzer.analyze(&program) {
        eprintln!("Semantic error: {}", e);
        process::exit(1);
    }
    
    println!("✅ No errors found");
}
