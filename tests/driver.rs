//! Public CLI regressions; use real source files and run generated executables.
use std::{fs, path::{Path, PathBuf}, process::{Command, Output}, sync::atomic::{AtomicU64, Ordering}};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Project(PathBuf);
impl Project {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("aether-driver-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir_all(&dir).unwrap(); Self(dir)
    }
    fn source(&self, path: &str, source: &str) -> PathBuf { let p = self.0.join(path); if let Some(d) = p.parent() { fs::create_dir_all(d).unwrap(); } fs::write(&p, source).unwrap(); p }
    fn compiler(&self, args: &[&str]) -> Output { Command::new(env!("CARGO_BIN_EXE_aethc")).current_dir(&self.0).args(args).output().unwrap() }
}
impl Drop for Project { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
fn success(output: &Output) { assert!(output.status.success(), "stdout={}\nstderr={}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)); }
fn run(exe: &Path, code: i32) { assert_eq!(Command::new(exe).status().unwrap().code(), Some(code)); }

#[test]
fn imports_compile_bodies_aliases_and_private_helpers_from_any_cwd() {
    let p = Project::new();
    p.source("src/math.aeth", "fn hidden(x: i32) -> i32 { return x * 2; } pub fn twice(x: i32) -> i32 { return hidden(x); }");
    p.source("src/other.aeth", "fn hidden(x: i32) -> i32 { return x + 1; } pub fn next(x: i32) -> i32 { return hidden(x); }");
    p.source("src/main.aeth", "use math::{twice as double}; use other::next; fn main() -> i32 { return next(double(20)); }");
    success(&p.compiler(&["check", "src/main.aeth"]));
    for backend in backends() { for opt in ["0", "2"] {
        success(&p.compiler(&["build", "src/main.aeth", "--backend", backend, "-O", opt, "-o", "program"]));
        run(&p.0.join("program"), 41);
    } }
    assert!(!p.0.join("src/main.c").exists());
}
fn backends() -> Vec<&'static str> { if cfg!(feature = "llvm") { vec!["c", "llvm"] } else { vec!["c"] } }

#[test]
fn imports_do_not_expose_private_or_missing_symbols() {
    let p = Project::new();
    p.source("child.aeth", "fn hidden() -> i32 { return 1; } pub fn visible() -> i32 { return hidden(); }");
    for source in ["use child::hidden; fn main() -> i32 { return hidden(); }", "use child::missing; fn main() -> i32 { return missing(); }"] {
        p.source("main.aeth", source);
        let output = p.compiler(&["check", "main.aeth"]);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("no public import"));
    }
}

#[test]
fn diamond_imports_compile_once_and_cycles_report_the_path() {
    let p = Project::new();
    p.source("base.aeth", "pub fn value() -> i32 { return 21; }");
    p.source("left.aeth", "use base::value; pub fn left() -> i32 { return value(); }");
    p.source("right.aeth", "use base::value; pub fn right() -> i32 { return value(); }");
    p.source("main.aeth", "use left::left; use right::right; fn main() -> i32 { return left() + right(); }");
    success(&p.compiler(&["build", "main.aeth", "-o", "program"])); run(&p.0.join("program"), 42);
    p.source("base.aeth", "use left::left; pub fn value() -> i32 { return left(); }");
    let o = p.compiler(&["check", "main.aeth"]);
    assert!(!o.status.success()); assert!(String::from_utf8_lossy(&o.stderr).contains("circular import"));
}

#[test]
fn imported_errors_point_to_the_imported_file() {
    let p = Project::new();
    p.source("child.aeth", "pub fn bad() -> i32 {\n    return true;\n}\n");
    p.source("main.aeth", "use child::bad; fn main() -> i32 { return bad(); }");
    let o = p.compiler(&["check", "main.aeth"]);
    assert!(!o.status.success()); assert!(String::from_utf8_lossy(&o.stderr).contains("child.aeth:2:"));
}

#[test]
fn compile_preserves_adjacent_c_source_and_ir_honors_output() {
    let p = Project::new();
    p.source("main.aeth", "fn main() -> i32 { return 0; }");
    p.source("main.c", "user maintained C source\n");
    success(&p.compiler(&["build", "main.aeth", "-o", "program"]));
    assert_eq!(fs::read_to_string(p.0.join("main.c")).unwrap(), "user maintained C source\n");
    success(&p.compiler(&["main.aeth", "--emit-ir", "-o", "custom.ir"]));
    assert!(p.0.join("custom.ir").is_file()); assert!(!p.0.join("main.ir").exists());
}

#[cfg(unix)]
#[test]
fn external_compiler_failure_keeps_real_diagnostic() {
    use std::os::unix::fs::PermissionsExt;
    let p = Project::new();
    p.source("main.aeth", "fn main() -> i32 { return 0; }");
    let cc = p.source("cc-test", "#!/bin/sh\necho deliberate-compiler-error >&2\nexit 23\n");
    fs::set_permissions(&cc, fs::Permissions::from_mode(0o755)).unwrap();
    let o = p.compiler(&["main.aeth", "--cc", cc.to_str().unwrap()]);
    assert!(!o.status.success());
    let stderr = String::from_utf8_lossy(&o.stderr);
    assert!(stderr.contains("deliberate-compiler-error")); assert!(stderr.contains("23"));
}

#[test]
fn script_check_and_build_use_the_same_pipeline() {
    let p = Project::new();
    p.source("main.ath", "def main():\n    return 0\n");
    success(&p.compiler(&["check", "main.ath"]));
    success(&p.compiler(&["build", "main.ath", "-o", "program"]));
    run(&p.0.join("program"), 0);
    assert!(!p.0.join("main.gen.aeth").exists());
}

#[test]
fn inline_modules_and_qualified_calls_use_the_same_resolver() {
    let p = Project::new();
    p.source("main.aeth", "mod math { pub fn answer() -> i32 { return 42; } fn hidden() -> i32 { return 1; } } fn main() -> i32 { return math::answer(); }");
    success(&p.compiler(&["build", "main.aeth", "-o", "program"])); run(&p.0.join("program"), 42);
    p.source("main.aeth", "mod math { fn hidden() -> i32 { return 1; } } fn main() -> i32 { return math::hidden(); }");
    let o = p.compiler(&["check", "main.aeth"]); assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("no public symbol"));
}

#[test]
fn duplicate_foreign_declarations_must_have_the_same_abi() {
    let p = Project::new();
    p.source("child.aeth", "extern \"C\" { fn external(x: i32) -> i32; } pub fn call() -> i32 { return external(1); }");
    p.source("main.aeth", "use child::call; extern \"C\" { fn external(x: i64) -> i32; } fn main() -> i32 { return call(); }");
    let o = p.compiler(&["check", "main.aeth"]); assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("conflicting foreign declarations"));
}

#[test]
fn all_output_modes_protect_the_input_source() {
    let p = Project::new();
    let source = "fn main() -> i32 { return 0; }";
    p.source("main.aeth", source);
    for mode in ["--emit-ir", "--emit-c", "--emit-obj"] {
        let o = p.compiler(&["main.aeth", mode, "-o", "./main.aeth"]);
        assert!(!o.status.success()); assert!(String::from_utf8_lossy(&o.stderr).contains("must not overwrite"));
        assert_eq!(fs::read_to_string(p.0.join("main.aeth")).unwrap(), source);
    }
}

#[test]
fn language_functions_do_not_collide_with_c_library_names() {
    let p = Project::new();
    p.source("main.aeth", "fn puts(x: i32) -> i32 { return x + 1; } fn heapsort(x: i32) -> i32 { return puts(x); } fn main() -> i32 { return heapsort(40); }");
    for backend in backends() {
        success(&p.compiler(&["main.aeth", "--backend", backend, "-o", "program"]));
        run(&p.0.join("program"), 41);
    }
}

#[test]
fn constants_aliases_and_foreign_aliases_share_semantic_facts() {
    let p = Project::new();
    p.source("numbers.aeth", "pub type Number = i32; const BASE: Number = 20; const ANSWER: Number = BASE * 2 + 2; extern \"C\" { fn abs(x: Number) -> Number; } pub fn positive(x: Number) -> Number { return abs(x); }");
    p.source("main.aeth", "use numbers::{ANSWER, positive}; extern \"C\" { fn abs(x: i32) -> i32; } fn main() -> i32 { let n: numbers::Number = ANSWER; return positive(-n); }");
    success(&p.compiler(&["check", "main.aeth"]));
    for backend in backends() { for opt in ["0", "2"] {
        success(&p.compiler(&["main.aeth", "--backend", backend, "-O", opt, "-o", "program"]));
        run(&p.0.join("program"), 42);
    } }
}

#[test]
fn default_entry_has_a_defined_process_status() {
    let p = Project::new();
    for (source, expected) in [("fn main() {}", 0), ("fn main() { return; }", 0), ("fn main() { return 17; }", 17), ("fn main() { 23 }", 0), ("fn main() -> i32 { 23 }", 23)] {
        p.source("main.aeth", source);
        for backend in backends() {
            success(&p.compiler(&["main.aeth", "--backend", backend, "-o", "program"]));
            run(&p.0.join("program"), expected);
        }
    }
    p.source("main.aeth", "fn main() -> f64 { return 0.0; }");
    assert!(!p.compiler(&["check", "main.aeth"]).status.success());
}

#[test]
fn output_protects_imported_sources_and_hardlinks() {
    let p = Project::new();
    let main = "use child::value; fn main() -> i32 { return value(); }";
    let child = "pub fn value() -> i32 { return 0; }";
    p.source("main.aeth", main);
    p.source("child.aeth", child);
    for mode in ["--emit-c", "--emit-ir", "--emit-obj"] {
        assert!(!p.compiler(&["main.aeth", mode, "-o", "child.aeth"]).status.success());
        assert_eq!(fs::read_to_string(p.0.join("child.aeth")).unwrap(), child);
    }
    #[cfg(unix)] {
        fs::hard_link(p.0.join("main.aeth"), p.0.join("alias.c")).unwrap();
        assert!(!p.compiler(&["main.aeth", "--emit-c", "-o", "alias.c"]).status.success());
        assert_eq!(fs::read_to_string(p.0.join("main.aeth")).unwrap(), main);
    }
}
