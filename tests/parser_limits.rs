//! Deep source must produce a diagnostic, never terminate the compiler or LSP.
use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Project(PathBuf);
impl Project {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "aether-parser-limits-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, text).unwrap();
        path
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn compiler() -> Command {
    Command::new(env!("CARGO_BIN_EXE_aethc"))
}
fn run(mut command: Command, input: Vec<u8>) -> Output {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = thread::spawn(move || stdin.write_all(&input));
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let out = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let err = thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("parser/LSP exceeded 15 seconds");
        }
        thread::sleep(Duration::from_millis(10));
    };
    let _ = writer.join().unwrap(); // An abort is diagnosed by the status assertion below.
    Output {
        status,
        stdout: out.join().unwrap(),
        stderr: err.join().unwrap(),
    }
}
fn deep_sum() -> String {
    format!(
        "fn main()->i32 {{\n return {};\n}}\n",
        vec!["1"; 4096].join("+")
    )
}
fn check(path: &Path) -> Output {
    let mut command = compiler();
    command.args(["check", "--error-format", "json"]).arg(path);
    run(command, Vec::new())
}
fn assert_limit(output: Output, path: &Path) {
    assert_eq!(
        output.status.code(),
        Some(1),
        "abnormal status {:?}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let diagnostic: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(diagnostic["code"], "E1002", "{diagnostic}");
    assert_eq!(
        diagnostic["location"]["file"],
        path.to_string_lossy().as_ref()
    );
    assert!(diagnostic["location"]["line"].as_u64().unwrap() >= 1);
    assert!(diagnostic["location"]["column"].as_u64().unwrap() >= 1);
}

#[test]
fn excessive_expression_depth_is_a_bounded_source_diagnostic() {
    let project = Project::new();
    let source = project.file("deep.aeth", &deep_sum());
    assert_limit(check(&source), &source);
    let cases = [
        (
            "unary",
            format!("fn main()->i32{{return {}1;}}", "-".repeat(4096)),
        ),
        (
            "postfix",
            format!("fn main()->i32{{return value{};}}", ".next".repeat(4096)),
        ),
        (
            "parentheses",
            format!(
                "fn main()->i32{{return {}1{};}}",
                "(".repeat(1024),
                ")".repeat(1024)
            ),
        ),
        (
            "assignment",
            format!(
                "fn main()->i32{{let mut x:i32=0;{}0;return x;}}",
                "x=".repeat(1024)
            ),
        ),
        (
            "type",
            format!(
                "fn f(value:{}i32){{}}fn main()->i32{{return 0;}}",
                "*".repeat(1024)
            ),
        ),
        (
            "block",
            format!(
                "fn main()->i32{{{}{}return 0;}}",
                "{".repeat(1024),
                "}".repeat(1024)
            ),
        ),
        (
            "else_if",
            format!(
                "fn main()->i32{{{}if true{{}}return 0;}}",
                "if false{}else ".repeat(1024)
            ),
        ),
    ];
    for (name, text) in cases {
        let path = project.file(&format!("{name}.aeth"), &text);
        assert_limit(check(&path), &path);
    }
    let child = project.file(
        "child.aeth",
        &format!(
            "pub fn calculate()->i32{{return {};}}",
            vec!["1"; 4096].join("+")
        ),
    );
    let root = project.file(
        "root.aeth",
        "use child::calculate;fn main()->i32{return calculate();}",
    );
    assert_limit(check(&root), &child);
}

#[test]
fn ordinary_expression_depth_and_wide_programs_remain_supported() {
    let project = Project::new();
    let source = format!("fn identity(x:i32)->i32{{x}}fn main()->i32{{let total:i32={};let nested:i32={}identity(total){};if nested!=128{{return 1;}}return 0;}}", vec!["1";128].join("+"), "(".repeat(16), ")".repeat(16));
    let path = project.file("ordinary.aeth", &source);
    assert_eq!(check(&path).status.code(), Some(0));
    let wide = project.file(
        "wide.aeth",
        &format!(
            "fn main()->i32{{let values:[i32;2048]=[{}];{}return values[2047]-1;}}",
            vec!["1"; 2048].join(","),
            "identity(1);".repeat(1024)
        )
        .replace("fn main", "fn identity(x:i32)->i32{x}fn main"),
    );
    let result = check(&wide);
    assert_eq!(
        result.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let mut backends = vec!["c"];
    if cfg!(any(feature = "llvm", feature = "llvm23")) {
        backends.push("llvm");
    }
    for backend in backends {
        let executable = project.0.join(format!(
            "ordinary-{backend}{}",
            std::env::consts::EXE_SUFFIX
        ));
        let mut command = compiler();
        command
            .arg(&path)
            .args(["--backend", backend, "-O", "0", "-o"])
            .arg(&executable);
        let result = run(command, Vec::new());
        assert_eq!(
            result.status.code(),
            Some(0),
            "{backend}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            run(Command::new(executable), Vec::new()).status.code(),
            Some(0)
        );
    }
}

#[test]
fn lsp_recovers_from_a_depth_diagnostic_after_editing() {
    for (filename, excessive, repaired, code) in [
        (
            "editing.aeth",
            deep_sum(),
            "fn main()->i32{return 0;}",
            "E1002",
        ),
        (
            "editing.ath",
            format!("def main():\n    return {}\n", vec!["1"; 4096].join("+")),
            "def main():\n    return 0\n",
            "E1100",
        ),
    ] {
        let project = Project::new();
        let source = project.file(filename, repaired);
        let uri = format!(
            "file://{}",
            source
                .to_string_lossy()
                .replace('%', "%25")
                .replace(' ', "%20")
        );
        let messages = [
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
            json!({"jsonrpc":"2.0","method":"initialized","params":{}}),
            json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":uri,"version":1,"text":excessive}}}),
            json!({"jsonrpc":"2.0","method":"textDocument/didChange","params":{"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":repaired}]}}),
            json!({"jsonrpc":"2.0","id":2,"method":"shutdown","params":null}),
            json!({"jsonrpc":"2.0","method":"exit","params":null}),
        ];
        let mut input = Vec::new();
        for message in messages {
            let body = serde_json::to_vec(&message).unwrap();
            write!(input, "Content-Length: {}\r\n\r\n", body.len()).unwrap();
            input.extend(body);
        }
        let mut command = compiler();
        command.arg("lsp");
        let output = run(command, input);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let mut bytes = output.stdout.as_slice();
        let mut replies = Vec::new();
        while !bytes.is_empty() {
            let boundary = bytes
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .unwrap();
            let count: usize = std::str::from_utf8(&bytes[..boundary])
                .unwrap()
                .strip_prefix("Content-Length: ")
                .unwrap()
                .parse()
                .unwrap();
            bytes = &bytes[boundary + 4..];
            replies.push(serde_json::from_slice::<Value>(&bytes[..count]).unwrap());
            bytes = &bytes[count..];
        }
        let diagnostics: Vec<_> = replies
            .iter()
            .filter(|reply| {
                reply["method"] == "textDocument/publishDiagnostics"
                    && reply["params"]["uri"] == uri
            })
            .collect();
        assert!(
            diagnostics
                .iter()
                .any(|reply| reply["params"]["diagnostics"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|diagnostic| diagnostic["code"] == code)),
            "{replies:?}"
        );
        assert!(
            diagnostics.last().unwrap()["params"]["diagnostics"]
                .as_array()
                .unwrap()
                .is_empty(),
            "{replies:?}"
        );
        assert!(
            replies
                .iter()
                .any(|reply| reply["id"] == 2 && reply.get("result").is_some()),
            "LSP did not respond after recovering"
        );
    }
}

#[test]
fn script_limits_report_original_locations_and_keep_ordinary_programs() {
    let project = Project::new();
    for (name, source) in [
        (
            "sum",
            format!("def main():\n    return {}\n", vec!["1"; 4096].join("+")),
        ),
        (
            "unary",
            format!("def main():\n    return {}1\n", "-".repeat(1024)),
        ),
        (
            "postfix",
            format!("def main():\n    return value{}\n", ".next".repeat(1024)),
        ),
        (
            "parentheses",
            format!(
                "def main():\n    return {}1{}\n",
                "(".repeat(1024),
                ")".repeat(1024)
            ),
        ),
        (
            "elif",
            format!(
                "if False:\n    pass\n{}",
                "elif False:\n    pass\n".repeat(1024)
            ),
        ),
    ] {
        let path = project.file(&format!("{name}.ath"), &source);
        let output = check(&path);
        assert_eq!(
            output.status.code(),
            Some(1),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let diagnostic: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(diagnostic["code"], "E1100", "{diagnostic}");
        assert!(
            diagnostic["message"].as_str().unwrap().contains("exceeds"),
            "{diagnostic}"
        );
        assert_eq!(
            diagnostic["location"]["file"],
            path.to_string_lossy().as_ref()
        );
        assert!(diagnostic["location"]["line"].as_u64().unwrap() >= 1);
    }
    let source = format!("def identity(x: int) -> int:\n    return x\ndef main():\n    total = {}\n    {}return identity(total) - 16\n", vec!["1";16].join("+"), "identity(1)\n    ".repeat(1024));
    let path = project.file("ordinary.ath", &source);
    let output = check(&path);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut backends = vec!["c"];
    if cfg!(any(feature = "llvm", feature = "llvm23")) {
        backends.push("llvm");
    }
    for backend in backends {
        let executable = project
            .0
            .join(format!("script-{backend}{}", std::env::consts::EXE_SUFFIX));
        let mut command = compiler();
        command
            .arg(&path)
            .args(["--backend", backend, "-O", "0", "-o"])
            .arg(&executable);
        let output = run(command, Vec::new());
        assert_eq!(
            output.status.code(),
            Some(0),
            "{backend}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            run(Command::new(executable), Vec::new()).status.code(),
            Some(0)
        );
    }
}

#[test]
fn generated_expression_mutations_preserve_diagnostic_boundaries() {
    let project = Project::new();
    // Fixed seed and finite grammar make a failing input reproducible. The
    // original is well-typed; each edit breaks exactly one specified rule.
    let mut state = 947_u32;
    for sample in 0..24 {
        let mut expression = "α".to_owned();
        for _ in 0..sample % 7 + 1 {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            let op = ["+", "-", "*", "^"][(state >> 30) as usize];
            expression = format!("({expression} {op} {})", (state >> 12) % 13);
        }
        let function = format!("fn main()->i32 {{ /* λ */ let α:i32=2; return {expression}; }}");
        let path = project.file("mutation.aeth", &function);
        assert!(check(&path).status.success(), "{function}");
        for (broken, expected) in [
            (function.replace("=2;", "=0x;"), "E1000"),
            (function.replacen('(', "[", 1), "E1000"),
            (function.replace("α:i32", "α:bool"), "E2100"),
        ] {
            let path = project.file("mutation.aeth", &broken);
            let out = check(&path);
            assert_eq!(out.status.code(), Some(1), "{broken}");
            let d: Value = serde_json::from_slice(&out.stderr).unwrap();
            assert_eq!(d["code"], expected, "{broken}: {d}");
            let loc = &d["location"];
            for key in ["start", "end"] {
                let offset = loc[key].as_u64().unwrap() as usize;
                assert!(broken.is_char_boundary(offset), "{broken}: {d}");
            }
        }
    }
}

#[test]
fn recursive_specialization_is_bounded_and_templates_are_checked_when_used() {
    let project = Project::new();
    for (source, expected) in [
        ("fn count<T>(v:T,n:i32)->i32 {if n==0{return 42;} count(v,n-1)} fn main()->i32 {count(1,3)}", None),
        ("type A=B; type B=A; fn main(){let v:A=1;}", Some("finite type expansion")),
        ("fn grow<T>(v:T){grow(&v);} fn main(){grow(1);}", Some("finite type expansion")),
        ("fn f<T>(v:T)->bool{3} fn main()->i32{0}", None),
        ("fn f<T>(v:T)->bool{3} fn main()->i32{if f(2){0}else{1}}", Some("Type mismatch")),
    ] {
        let path = project.file("specialize.aeth", source);
        let out = check(&path);
        assert_eq!(out.status.code(), Some(if expected.is_some() { 1 } else { 0 }), "{source}");
        if let Some(expected) = expected {
            let d: Value = serde_json::from_slice(&out.stderr).unwrap();
            assert_eq!(d["code"], "E2100");
            assert!(d["message"].as_str().unwrap().contains(expected));
            assert_eq!(d["location"]["file"].as_str().unwrap(), path.to_str().unwrap());
        }
    }
}
