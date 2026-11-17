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

#[test]
fn explicit_modules_use_exact_files_real_siblings_and_shared_symbols() {
    let p = Project::new();
    let library = p.source("vendor/空 格=one/entry=main.aeth", "use sibling::base; fn hidden() -> i32 { base() + 1 } pub fn answer() -> i32 { hidden() }");
    p.source("vendor/空 格=one/sibling.aeth", "pub fn base() -> i32 { 20 }");
    p.source("app/sibling.aeth", "pub fn base() -> i32 { 100 }");
    p.source("roots/数学.aeth", "pub fn answer() -> i32 { 99 }");
    p.source("second/custom.aeth", "pub fn more() -> i32 { 21 }");
    p.source("app/main.aeth", "mod 数学; use 数学::answer; use bonus::more; fn main() -> i32 { answer() + more() }");
    let mappings = ["数学=vendor/空 格=one/entry=main.aeth", "bonus=second/custom.aeth"];
    success(&p.compiler(&["--module", mappings[0], "check", "app/main.aeth", "--module", mappings[1], "-I", "roots"]));
    let symbols = p.compiler(&["symbols", "app/main.aeth", "--module", mappings[0], "--module", mappings[1], "-I", "roots"]);
    success(&symbols);
    let index: serde_json::Value = serde_json::from_slice(&symbols.stdout).unwrap();
    let answer = index["symbols"].as_array().unwrap().iter().find(|symbol| symbol["name"] == "answer").unwrap();
    assert_eq!(answer["definition"]["file"], fs::canonicalize(library).unwrap().to_string_lossy().as_ref());
    for backend in backends() { for level in ["0", "2"] {
        success(&p.compiler(&["build", "app/main.aeth", "--module", mappings[0], "--module", mappings[1], "-I", "roots", "--backend", backend, "-O", level, "-o", "program"]));
        run(&p.0.join("program"), 42);
    } }
}

#[test]
fn explicit_modules_validate_identifiers_files_duplicates_and_preserve_output() {
    let p = Project::new();
    p.source("main.aeth", "fn main() -> i32 { 0 }");
    p.source("library.aeth", "pub fn value() -> i32 { 1 }");
    p.source("library.ath", "def value():\n    return 1\n");
    fs::create_dir(p.0.join("directory.aeth")).unwrap();
    p.source("program.c", "USER OUTPUT");
    for (mapping, message) in [
        ("library", "expected NAME=FILE"), ("=library.aeth", "invalid module name"),
        ("fn=library.aeth", "invalid module name"), ("a::b=library.aeth", "invalid module name"),
        ("two words=library.aeth", "invalid module name"), (" name=library.aeth", "invalid module name"),
        ("name =library.aeth", "invalid module name"), ("name/*comment*/=library.aeth", "invalid module name"),
        ("name=", "requires a source file"), ("name=missing.aeth", "explicit module 'name'"),
        ("name=library.ath", "exact .aeth source file"), ("name=directory.aeth", "exact .aeth source file"),
    ] {
        for command in ["check", "build", "symbols"] {
            let result = p.compiler(&[command, "main.aeth", "--module", mapping, "--emit-c", "-o", "program.c"]);
            assert!(!result.status.success(), "accepted {mapping}");
            assert!(String::from_utf8_lossy(&result.stderr).contains(message), "{}", String::from_utf8_lossy(&result.stderr));
            assert_eq!(fs::read_to_string(p.0.join("program.c")).unwrap(), "USER OUTPUT");
        }
    }
    let duplicate = p.compiler(&["check", "main.aeth", "--module", "name=library.aeth", "--module", "name=./library.aeth"]);
    assert!(!duplicate.status.success());
    assert!(String::from_utf8_lossy(&duplicate.stderr).contains("duplicate explicit module 'name'"));
    let invalid_lsp = p.compiler(&["lsp", "--module", "name=missing.aeth"]);
    assert!(!invalid_lsp.status.success());
}

#[test]
fn explicit_modules_reject_inline_and_different_local_sources() {
    let p = Project::new();
    p.source("vendor/library.aeth", "pub fn answer() -> i32 { 42 }");
    for source in ["mod named { pub fn answer() -> i32 { 1 } } fn main() {}", "mod outer { mod named {} } fn main() {}"] {
        p.source("app/main.aeth", source);
        let result = p.compiler(&["check", "app/main.aeth", "--module", "named=vendor/library.aeth"]);
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("conflicts with inline module"));
    }
    p.source("app/main.aeth", "use named::answer; fn main() -> i32 { answer() }");
    for filename in ["app/named.aeth", "app/named/mod.aeth"] {
        let local = p.source(filename, "pub fn answer() -> i32 { 7 }");
        for command in ["check", "build", "symbols"] {
            let result = p.compiler(&[command, "app/main.aeth", "--module", "named=vendor/library.aeth", "--emit-c", "-o", "program.c"]);
            assert!(!result.status.success());
            assert!(String::from_utf8_lossy(&result.stderr).contains("conflicts with local module"));
        }
        fs::remove_file(local).unwrap();
    }
    p.source("app/main.aeth", "use named::missing; fn main() {}");
    let result = p.compiler(&["check", "app/main.aeth", "--module", "named=vendor/library.aeth"]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("no public import"));
}

#[test]
fn explicit_modules_keep_canonical_identity_and_do_not_change_search_order() {
    let p = Project::new();
    p.source("app/named.aeth", "pub fn answer() -> i32 { 21 }");
    p.source("app/main.aeth", "use named::answer; use second::answer as other; fn main() -> i32 { answer() + other() }");
    success(&p.compiler(&["build", "app/main.aeth", "--module", "named=app/./named.aeth", "--module", "second=app/named.aeth", "-o", "program"]));
    run(&p.0.join("program"), 42);
    p.source("vendor/entry.aeth", "pub fn answer() -> i32 { 0 }");
    p.source("vendor/sibling.aeth", "pub fn value() -> i32 { 99 }");
    p.source("app/main.aeth", "use sibling::value; fn main() -> i32 { value() }");
    let result = p.compiler(&["check", "app/main.aeth", "--module", "named=vendor/entry.aeth"]);
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("module 'sibling' not found"));
    p.source("first/sibling.aeth", "pub fn value() -> i32 { 11 }");
    p.source("second/sibling.aeth", "pub fn value() -> i32 { 22 }");
    for (a, b, code) in [("first", "second", 11), ("second", "first", 22)] {
        success(&p.compiler(&["build", "app/main.aeth", "-I", a, "-I", b, "--module", "named=vendor/entry.aeth", "-o", "program"]));
        run(&p.0.join("program"), code);
    }
}

#[cfg(unix)]
#[test]
fn explicit_modules_accept_same_canonical_local_symlink() {
    let p = Project::new();
    let library = p.source("vendor/real.aeth", "use sibling::value; pub fn answer() -> i32 { value() }");
    p.source("vendor/sibling.aeth", "pub fn value() -> i32 { 42 }");
    p.source("alias/sibling.aeth", "pub fn value() -> i32 { 99 }");
    p.source("app/main.aeth", "use named::answer; fn main() -> i32 { answer() }");
    std::os::unix::fs::symlink(&library, p.0.join("app/named.aeth")).unwrap();
    std::os::unix::fs::symlink(&library, p.0.join("alias/entry.aeth")).unwrap();
    success(&p.compiler(&["build", "app/main.aeth", "--module", "named=alias/entry.aeth", "-o", "program"]));
    run(&p.0.join("program"), 42);
}

#[test]
fn explicit_modules_lsp_uses_mapped_sources_and_unsaved_overlays() {
    use serde_json::json;
    let p = Project::new();
    let library = p.source("vendor/entry=real.aeth", "pub fn answer() -> i32 { 42 }");
    let text = "use api::answer;\nfn main() -> i32 { return answer(); }\n";
    let main = p.source("app/main.aeth", text);
    let uri = |path: &Path| format!("file://{}", fs::canonicalize(path).unwrap().to_string_lossy().replace('%', "%25").replace(' ', "%20").replace('=', "%3D"));
    let messages = vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":uri(&main),"version":1,"text":text}}}),
        json!({"jsonrpc":"2.0","id":2,"method":"textDocument/definition","params":{"textDocument":{"uri":uri(&main)},"position":{"line":1,"character":26}}}),
        json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":uri(&library),"version":1,"text":"pub fn answer() -> i32 { true }"}}}),
        json!({"jsonrpc":"2.0","method":"textDocument/didChange","params":{"textDocument":{"uri":uri(&library),"version":2},"contentChanges":[{"text":"pub fn answer() -> i32 { 7 }"}]}}),
        json!({"jsonrpc":"2.0","id":3,"method":"shutdown","params":null}),
        json!({"jsonrpc":"2.0","method":"exit","params":null}),
    ];
    let replies = lsp_round_trip(&p, &["--module", "api=vendor/entry=real.aeth"], messages);
    assert_eq!(replies.iter().find(|reply| reply["id"] == 2).unwrap()["result"]["uri"], uri(&library));
    let changes: Vec<_> = replies.iter().filter(|reply| reply["method"] == "textDocument/publishDiagnostics" && reply["params"]["uri"] == uri(&library)).collect();
    assert!(changes.iter().any(|reply| !reply["params"]["diagnostics"].as_array().unwrap().is_empty()));
    assert!(changes.last().unwrap()["params"]["diagnostics"].as_array().unwrap().is_empty());
    assert_eq!(fs::read_to_string(library).unwrap(), "pub fn answer() -> i32 { 42 }");
}

fn lsp_round_trip(p: &Project, args: &[&str], messages: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
    use std::{io::Write, process::Stdio};
    use serde_json::Value;
    let mut server = Command::new(env!("CARGO_BIN_EXE_aethc")).current_dir(&p.0)
        .arg("lsp").args(args)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let mut input = server.stdin.take().unwrap();
    for message in messages { let body = serde_json::to_vec(&message).unwrap(); write!(input, "Content-Length: {}\r\n\r\n", body.len()).unwrap(); input.write_all(&body).unwrap(); }
    drop(input);
    let output = server.wait_with_output().unwrap(); success(&output);
    let mut bytes = output.stdout.as_slice(); let mut replies: Vec<Value> = Vec::new();
    while !bytes.is_empty() {
        let boundary = bytes.windows(4).position(|part| part == b"\r\n\r\n").unwrap();
        let count: usize = std::str::from_utf8(&bytes[..boundary]).unwrap().strip_prefix("Content-Length: ").unwrap().parse().unwrap();
        bytes = &bytes[boundary + 4..]; replies.push(serde_json::from_slice(&bytes[..count]).unwrap()); bytes = &bytes[count..];
    }
    replies
}

#[test]
fn module_namespace_does_not_capture_real_foreign_abi_symbols() {
    let p = Project::new();
    let native = p.source("host.c", "#include <stdint.h>\nint32_t __aeth_m0_local(void) { return 99; }\nint32_t __aeth_1_m0_local(void) { return 98; }\n");
    let output = Command::new(std::env::var_os("CC").unwrap_or_else(|| "clang".into()))
        .arg("-c").arg(native).arg("-o").arg(p.0.join("host.o")).output().unwrap();
    success(&output);
    p.source("same.aeth", "extern \"C\" { fn __aeth_m0_local() -> i32; } fn local() -> i32 { 7 } fn main() -> i32 { if __aeth_m0_local() == 99 && local() == 7 { 0 } else { 1 } }");
    p.source("late.aeth", "use later::second; extern \"C\" { fn __aeth_m0_local() -> i32; } pub fn foreign() -> i32 { if second() == 98 { __aeth_m0_local() } else { 0 } }");
    p.source("later.aeth", "extern \"C\" { fn __aeth_1_m0_local() -> i32; } pub fn second() -> i32 { __aeth_1_m0_local() }");
    p.source("main.aeth", "use late::foreign; fn local() -> i32 { 7 } fn main() -> i32 { if foreign() == 99 && local() == 7 { 0 } else { 1 } }");
    for source in ["same.aeth", "main.aeth"] {
        success(&p.compiler(&["check", source]));
        for backend in backends() { for level in ["0", "2"] {
            success(&p.compiler(&["build", source, "--backend", backend, "-O", level, "--link-object", "host.o", "-o", "program"]));
            run(&p.0.join("program"), 0);
        } }
    }
    let output = p.compiler(&["symbols", "main.aeth"]); success(&output);
    let index: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let symbols = index["symbols"].as_array().unwrap();
    for name in ["__aeth_m0_local", "__aeth_1_m0_local"] {
        let foreign = symbols.iter().find(|symbol| symbol["name"] == name).unwrap();
        assert_eq!(foreign["kind"], "extern function");
        let text = fs::read_to_string(foreign["definition"]["file"].as_str().unwrap()).unwrap();
        assert_eq!(&text[foreign["definition"]["start"].as_u64().unwrap() as usize..foreign["definition"]["end"].as_u64().unwrap() as usize], name);
    }
    assert_eq!(symbols.iter().filter(|symbol| symbol["name"] == "local").count(), 1);
}

#[test]
fn module_namespace_cannot_be_captured_by_lexical_bindings() {
    let p = Project::new();
    let text = "struct Number { value:i32 } const answer:i32=7; fn main()->i32 { let __aeth_m0_answer:i32=99; let __aeth_1_m0_answer:i32=98; let number:Number=Number{value:answer}; if number.value==7 && __aeth_m0_answer==99 && __aeth_1_m0_answer==98 {0} else {1} }";
    p.source("main.aeth", text);
    for backend in backends() { for level in ["0", "2"] {
        success(&p.compiler(&["build", "main.aeth", "--backend", backend, "-O", level, "-o", "program"]));
        run(&p.0.join("program"), 0);
    } }
    let output = p.compiler(&["symbols", "main.aeth"]); success(&output);
    let index: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let symbols = index["symbols"].as_array().unwrap();
    for name in ["Number", "answer", "__aeth_m0_answer", "__aeth_1_m0_answer", "number"] {
        let symbol = symbols.iter().find(|symbol| symbol["name"] == name).unwrap();
        assert_eq!(&text[symbol["definition"]["start"].as_u64().unwrap() as usize..symbol["definition"]["end"].as_u64().unwrap() as usize], name);
    }
    assert_eq!(symbols.iter().find(|symbol| symbol["name"] == "number").unwrap()["type_name"], "Number");
    let constant = symbols.iter().find(|symbol| symbol["name"] == "answer").unwrap();
    let reference = index["references"].as_array().unwrap().iter().find(|reference| reference["symbol"] == constant["id"]).unwrap();
    assert_eq!(reference["location"]["start"].as_u64().unwrap() as usize, text.find("value:answer").unwrap() + "value:".len());
}

#[test]
fn module_namespace_preserves_normal_spelling_and_editor_source_names() {
    use serde_json::json;
    let p = Project::new();
    p.source("ordinary.aeth", "// __aeth_m0_local in a comment is not an identifier\nfn local() -> i32 { 7 } fn main()->i32 { local() }");
    success(&p.compiler(&["ordinary.aeth", "--emit-ir", "-o", "ordinary.ir"]));
    let ir = fs::read_to_string(p.0.join("ordinary.ir")).unwrap();
    assert!(ir.contains("__aeth_m0_local"));
    assert!(!ir.contains("__aeth_1_m"));
    let text = "const answer:i32=7;\nfn main()->i32 {\n    let __aeth_m0_answer:i32=99;\n    return answer;\n}\n";
    let source = p.source("editor.aeth", text);
    let uri = format!("file://{}", fs::canonicalize(source).unwrap().to_string_lossy().replace('%', "%25").replace(' ', "%20"));
    let replies = lsp_round_trip(&p, &[], vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":uri,"version":1,"text":text}}}),
        json!({"jsonrpc":"2.0","id":2,"method":"textDocument/documentSymbol","params":{"textDocument":{"uri":uri}}}),
        json!({"jsonrpc":"2.0","id":3,"method":"textDocument/hover","params":{"textDocument":{"uri":uri},"position":{"line":2,"character":10}}}),
        json!({"jsonrpc":"2.0","id":4,"method":"textDocument/definition","params":{"textDocument":{"uri":uri},"position":{"line":3,"character":12}}}),
        json!({"jsonrpc":"2.0","id":5,"method":"shutdown","params":null}),
        json!({"jsonrpc":"2.0","method":"exit","params":null}),
    ]);
    let document = replies.iter().find(|reply| reply["id"] == 2).unwrap()["result"].as_array().unwrap();
    assert!(document.iter().any(|symbol| symbol["name"] == "__aeth_m0_answer"));
    assert_eq!(document.iter().filter(|symbol| symbol["name"] == "answer").count(), 1);
    assert_eq!(replies.iter().find(|reply| reply["id"] == 3).unwrap()["result"]["contents"]["value"], "__aeth_m0_answer: i32");
    assert_eq!(replies.iter().find(|reply| reply["id"] == 4).unwrap()["result"]["range"]["start"], json!({"line":0,"character":6}));
}

#[test]
fn unsized_types_and_untyped_uninitialized_locals_fail_before_codegen() {
    let p = Project::new();
    for (source, code) in [
        ("fn width(x:[i32])->usize{size_of([i32])} fn main()->i32{0}", "E2406"),
        ("fn width(x:&[u8])->usize{1} fn main()->i32{0}", "E2406"),
        ("fn main()->i32{let mut x; x=1; x}", "E1000"),
    ] {
        p.source("bad.aeth", source);
        let mut diagnostics = Vec::new();
        for backend in backends() {
            for command in ["check", "build"] {
                let output = p.compiler(&[command, "bad.aeth", "--backend", backend, "--error-format", "json", "-o", "bad"]);
                assert_eq!(output.status.code(), Some(1), "{source}: {output:?}");
                let diagnostic: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
                assert_eq!(diagnostic["code"], code);
                assert!(diagnostic["location"]["column"].is_number());
                diagnostics.push(diagnostic);
            }
        }
        assert!(diagnostics.windows(2).all(|pair| pair[0] == pair[1]));
        assert!(!p.0.join("bad").exists());
    }
    p.source("good.aeth", "fn main()->i32{let mut x:i16; x=3; let a:[i16;2]=[x,x]; a[1] as i32}");
    for backend in backends() {
        success(&p.compiler(&["build", "good.aeth", "--backend", backend, "-o", "good"]));
        run(&p.0.join("good"), 3);
    }
}
