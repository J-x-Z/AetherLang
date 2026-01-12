//! AetherLang Compiler
//! 
//! A self-hosting systems programming language.

mod frontend;
mod middle;
mod backend;
mod types;
mod utils;

use clap::Parser;
use std::path::PathBuf;

/// AetherLang Compiler
#[derive(Parser, Debug)]
#[command(name = "aethc")]
#[command(author = "Z1529")]
#[command(version = "0.1.0")]
#[command(about = "AetherLang compiler - simpler than Rust, safer than C")]
struct Cli {
    /// Input source file (.aeth)
    #[arg(value_name = "FILE")]
    input: PathBuf,

    /// Output file
    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,

    /// Emit LLVM IR instead of binary
    #[arg(long)]
    emit_llvm: bool,

    /// Emit Aether IR (for debugging)
    #[arg(long)]
    emit_ir: bool,

    /// Optimization level (0-3)
    #[arg(short = 'O', default_value = "0")]
    opt_level: u8,

    /// Target triple (e.g., x86_64-pc-windows-msvc)
    #[arg(long)]
    target: Option<String>,
}

fn main() {
    env_logger::init();
    
    let cli = Cli::parse();
    
    println!("AetherLang Compiler v0.1.0");
    println!("Input: {:?}", cli.input);
    
    // TODO: Implement compilation pipeline
    // 1. Read source file
    // 2. Lexer -> Tokens
    // 3. Parser -> AST
    // 4. Semantic Analysis -> Typed AST
    // 5. IR Generation -> Aether IR
    // 6. Optimization -> Optimized IR
    // 7. Code Generation -> Binary
    
    println!("Compilation not yet implemented.");
}
