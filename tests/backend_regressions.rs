//! Semantic regression programs must agree across native backends and optimization levels.
use std::{fs, path::PathBuf, process::{Command, Stdio}, thread, time::{Duration, Instant}};

#[test]
fn native_semantics_matrix() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let directory = std::env::temp_dir().join(format!("aether-backend-regressions-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let mut backends = vec!["c"];
    if cfg!(feature = "llvm") { backends.push("llvm"); }
    for name in ["control_flow", "short_circuit", "numeric_types", "integer_literals", "places", "struct_returns", "ffi_variadic", "string_bytes", "scalar_match", "struct_layouts", "integer_wrapping", "divide_zero", "divide_overflow", "constants_aliases"] {
        for backend in &backends {
            for level in [0, 2, 3] {
                let executable = directory.join(format!("{name}-{backend}-{level}{}", std::env::consts::EXE_SUFFIX));
                let compiled = Command::new(env!("CARGO_BIN_EXE_aethc"))
                    .arg(root.join("tests/regressions").join(format!("{name}.aeth")))
                    .args(["--backend", backend, "-O", &level.to_string(), "-o"])
                    .arg(&executable).output().unwrap();
                assert!(compiled.status.success(), "{name}/{backend}/O{level} compile failed:\n{}\n{}",
                    String::from_utf8_lossy(&compiled.stdout), String::from_utf8_lossy(&compiled.stderr));
                let mut child = Command::new(&executable).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
                let deadline = Instant::now() + Duration::from_secs(10);
                while child.try_wait().unwrap().is_none() {
                    if Instant::now() >= deadline {
                        child.kill().unwrap();
                        let _ = child.wait();
                        panic!("{name}/{backend}/O{level} did not terminate");
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                let output = child.wait_with_output().unwrap();
                let status_ok = if name.starts_with("divide_") {
                    #[cfg(unix)] { use std::os::unix::process::ExitStatusExt; output.status.signal().is_some() }
                    #[cfg(not(unix))] { !output.status.success() }
                } else { output.status.success() };
                let expected_stdout = if name == "constants_aliases" { b"constants and aliases\n".as_slice() } else { b"".as_slice() };
                assert!(status_ok && output.stdout == expected_stdout && output.stderr.is_empty(),
                    "{name}/{backend}/O{level}: status {}, stdout {:?}, stderr {:?}", output.status,
                    String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
            }
        }
    }
    fs::remove_dir_all(directory).unwrap();
}

#[test]
#[cfg(any(target_arch = "aarch64", target_arch = "x86_64"))]
fn native_inline_assembly_uses_typed_places_and_source_operand_order() {
    let directory = std::env::temp_dir().join(format!("aether-asm-regression-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    #[cfg(target_arch = "aarch64")]
    let assembly = r#"
        asm!("add $0, $0, #5", inout("r") value);
        asm!("add $1, $0, #3", in("r") input, out("r") output);
    "#;
    #[cfg(target_arch = "x86_64")]
    let assembly = r#"
        asm!("addq $$5, $0", inout("r") value);
        asm!("movq $0, $1\naddq $$3, $1", in("r") input, out("r") output);
    "#;
    let source = format!(r#"
        fn main() -> i32 {{
            let mut value: i64 = 10;
            let input: i64 = 7;
            let output: i64;
            unsafe {{ {assembly} asm!("nop", clobber("memory")); }}
            if value != 15 || output != 10 {{ return 1; }}
            return 0;
        }}
    "#);
    let input = directory.join("native.aeth");
    fs::write(&input, source).unwrap();
    let mut backends = vec!["c"];
    if cfg!(feature = "llvm") { backends.push("llvm"); }
    for backend in backends {
        for level in [0, 2, 3] {
            let executable = directory.join(format!("asm-{backend}-{level}{}", std::env::consts::EXE_SUFFIX));
            let compiled = Command::new(env!("CARGO_BIN_EXE_aethc")).arg(&input)
                .args(["--backend", backend, "-O", &level.to_string(), "-o"]).arg(&executable).output().unwrap();
            assert!(compiled.status.success(), "asm/{backend}/O{level}: {}", String::from_utf8_lossy(&compiled.stderr));
            assert!(Command::new(executable).status().unwrap().success(), "asm/{backend}/O{level} wrong result");
        }
    }
    fs::remove_dir_all(directory).unwrap();
}
