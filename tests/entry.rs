//! Entry-point normalization is part of the public source-to-executable contract.
use std::{fs, path::PathBuf, process::Command};

#[test]
fn default_entry_returns_zero_and_keeps_module_main_as_an_ordinary_function() {
    let directory = std::env::temp_dir().join(format!("aether-entry-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("helper.aeth"), "pub fn main() { return; }").unwrap();
    let sources = [
        "fn main() {}",
        "fn main() { if true { return; } }",
        "fn main() { 73 }",
        "use helper::main as helper_main; fn main() { helper_main(); }",
    ];
    let backends = if cfg!(feature = "llvm") { vec!["c", "llvm"] } else { vec!["c"] };
    for source in sources {
        fs::write(directory.join("main.aeth"), source).unwrap();
        for backend in &backends {
            let executable: PathBuf = directory.join("entry-program");
            let compiled = Command::new(env!("CARGO_BIN_EXE_aethc")).current_dir(&directory)
                .args(["build", "main.aeth", "--backend", backend, "-o"]).arg(&executable).output().unwrap();
            assert!(compiled.status.success(), "{}\n{}", source, String::from_utf8_lossy(&compiled.stderr));
            assert_eq!(Command::new(&executable).status().unwrap().code(), Some(0));
        }
    }
    for source in ["fn main() -> void {}", "fn main() -> f64 { 1.0 }", "fn main(x: i64) -> i32 { 0 }"] {
        fs::write(directory.join("main.aeth"), source).unwrap();
        assert!(!Command::new(env!("CARGO_BIN_EXE_aethc")).current_dir(&directory).args(["check", "main.aeth"]).output().unwrap().status.success());
    }
    fs::write(directory.join("main.aeth"), "#![no_main] fn main() -> f64 { 0.0 }").unwrap();
    assert!(Command::new(env!("CARGO_BIN_EXE_aethc")).current_dir(&directory).args(["check", "main.aeth"]).output().unwrap().status.success());
    fs::remove_dir_all(directory).unwrap();
}
