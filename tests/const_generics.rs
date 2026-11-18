//! Const generic layout, inference and runtime semantics share one Core pipeline.
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[test]
fn core_and_script_const_layouts_execute_on_native_backends() {
    let work = std::env::temp_dir().join(format!("aether-const-runtime-{}", std::process::id()));
    fs::create_dir_all(&work).unwrap();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut backends = vec!["c"];
    if cfg!(any(feature = "llvm", feature = "llvm23")) {
        backends.push("llvm");
    }
    for source in [
        "tests/regressions/const_generics.aeth",
        "tests/regressions/script/const_generics.ath",
    ] {
        for backend in &backends {
            for level in [0, 2, 3] {
                let binary = work.join(format!(
                    "{}-{backend}-{level}{}",
                    if source.ends_with(".ath") {
                        "script"
                    } else {
                        "core"
                    },
                    std::env::consts::EXE_SUFFIX
                ));
                let compiled = Command::new(env!("CARGO_BIN_EXE_aethc"))
                    .arg(root.join(source))
                    .args(["--backend", backend, "-O", &level.to_string(), "-o"])
                    .arg(&binary)
                    .output()
                    .unwrap();
                assert!(
                    compiled.status.success(),
                    "{source}/{backend}/{level}: {compiled:?}"
                );
                let mut child = Command::new(binary)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap();
                let deadline = Instant::now() + Duration::from_secs(10);
                while child.try_wait().unwrap().is_none() {
                    if Instant::now() >= deadline {
                        child.kill().unwrap();
                        child.wait().unwrap();
                        panic!("const fixture timed out");
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                let output = child.wait_with_output().unwrap();
                assert!(
                    output.status.success() && output.stdout.is_empty() && output.stderr.is_empty(),
                    "{source}/{backend}/{level}: {output:?}"
                );
            }
        }
    }
    fs::remove_dir_all(work).unwrap();
}

#[test]
fn invalid_const_arguments_have_the_same_check_and_build_diagnostic() {
    let work = std::env::temp_dir().join(format!("aether-const-errors-{}", std::process::id()));
    fs::create_dir_all(&work).unwrap();
    for (name,source) in [
        ("empty_without_element","fn length<T,const N:usize>(a:[T;N])->usize{N}fn main(){length([]);}"),
        ("empty_extent_conflict","fn same<const N:usize>(a:[u8;N],b:[u8;N]){}fn main(){same([],[1]);}"),
        ("empty_unannotated","fn main(){let empty=[];}"),
        ("negative","struct B<const N:usize>{x:[u8;N]}fn main(){let x:B<{-1}>; }"),
        ("overflow","struct B<const N:u8>{x:i32}fn main(){let x:B<256>;}"),
        ("float","struct B<const N:f64>{x:i32}fn main(){}"),
        ("pointer","fn f<const P:*u8>(){}fn main(){}"),
        ("duplicate","fn f<const N:usize,const N:usize>(){}fn main(){}"),
        ("type_position","fn f<const N:usize>(x:N){}fn main(){f(1);}"),
        ("mismatch","fn same<const N:usize>(a:[i64;N],b:[i64;N]){}fn main(){same([1,2],[3]);}"),
        ("computed_mismatch","fn same<const N:usize>(a:[i64;N],b:[i64;N+1]){}fn main(){same([1,2],[3]);}"),
        ("unbound","fn f<const N:usize>()->usize{N}fn main(){f();}"),
        ("cyclic","const A:usize=B;const B:usize=A;struct S{a:[u8;A]}fn main(){}"),
        ("dimension_overflow","struct S{a:[u8;((18446744073709551615 as usize)+1)]}fn main(){}"),
        ("dimension_bool","struct S{a:[u8;true]}fn main(){}"),
        ("runtime_length","fn f(n:usize){let a:[u8;n];}fn main(){}"),
        ("const_shadow","fn f<const N:usize>(a:[u8;N]){let N:usize=1;}fn main(){f([1 as u8]);}"),
        ("nominal_identity","struct Marker<const N:usize>{v:i32}fn main(){let a:Marker<2>=Marker{v:1};let b:Marker<3>=a;}"),
        ("script_mismatch","def same[const N: int](a: Array[int,N], b: Array[int,N]):\n    pass\ndef main():\n    same([1,2],[3])\n"),
        ("script_runtime","def f(n: int):\n    pass\ndef bad(a: Array[int,n]):\n    pass\ndef main():\n    return 0\n"),
        ("script_shadow","def f[const N: int](a: Array[int,N]):\n    N = 5\ndef main():\n    f([1])\n"),
        ("script_mixed","def main():\n    data = [1,True]\n    return 0\n"),
    ] {
        let path=work.join(format!("{name}.{}",if name.starts_with("script_"){ "ath" }else{"aeth"}));
        fs::write(&path,source).unwrap();
        let mut previous=None;
        for mode in ["check","build"] {
            let mut command=Command::new(env!("CARGO_BIN_EXE_aethc"));
            command.args([mode,"--error-format","json"]).arg(&path);
            if mode=="build"{command.args(["--backend","c","-o"]).arg(work.join("rejected"));}
            let output=command.output().unwrap();
            assert_eq!(output.status.code(),Some(1),"{name}/{mode}: {output:?}");
            let diagnostic:serde_json::Value=serde_json::from_slice(&output.stderr).unwrap();
            if let Some(expected)=previous{assert_eq!(diagnostic,expected,"{name}: check/build mismatch");}
            previous=Some(diagnostic);
        }
    }
    fs::remove_dir_all(work).unwrap();
}

#[test]
fn const_usize_evaluation_uses_the_selected_target_before_specialization() {
    let work = std::env::temp_dir().join(format!("aether-const-target-{}", std::process::id()));
    fs::create_dir_all(&work).unwrap();
    let source = work.join("target.aeth");
    fs::write(&source,"struct Marker<const N:usize>{value:u8}fn main(){let marker:Marker<{(4294967295 as usize)+1}>=Marker{value:0};}").unwrap();
    for (target, success) in [
        ("x86_64-unknown-linux-gnu", true),
        ("wasm32-unknown-unknown", false),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_aethc"))
            .args(["check", "--target", target])
            .arg(&source)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(if success { 0 } else { 1 }),
            "{target}: {output:?}"
        );
        if !success {
            assert!(String::from_utf8_lossy(&output.stderr).contains("integer overflow"));
        }
    }
    fs::remove_dir_all(work).unwrap();
}

#[test]
fn module_aliases_and_lsp_use_the_same_const_layout_rules() {
    use serde_json::{json, Value};
    use std::io::Write;
    let work = std::env::temp_dir().join(format!("aether-const-modules-{}", std::process::id()));
    fs::create_dir_all(&work).unwrap();
    fs::write(work.join("shapes.aeth"),"pub type Dim=usize;pub const WIDTH:Dim=3;pub struct Buffer<T,const N:Dim>{data:[T;N]}pub fn length<T,const N:Dim>(data:[T;N])->Dim{N}").unwrap();
    let source = work.join("main.aeth");
    fs::write(&source,"use shapes::{Buffer,WIDTH,length};fn main()->i32{let b:Buffer<i64,WIDTH>=Buffer{data:[1,2,3]};if length(b.data)!=3{return 1;}0}").unwrap();
    let checked = Command::new(env!("CARGO_BIN_EXE_aethc"))
        .arg("check")
        .arg(&source)
        .output()
        .unwrap();
    assert!(checked.status.success(), "{checked:?}");
    let document="struct Marker<const N:usize>{value:u8}fn main(){let marker:Marker<{(4294967295 as usize)+1}>=Marker{value:0};}";
    let uri = format!("file://{}",source.to_string_lossy().replace(' ',"%20"));
    for (target, has_error) in [
        ("x86_64-unknown-linux-gnu", false),
        ("wasm32-unknown-unknown", true),
    ] {
        let messages = [
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
            json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":uri,"languageId":"aether","version":1,"text":document}}}),
            json!({"jsonrpc":"2.0","id":2,"method":"shutdown","params":null}),
            json!({"jsonrpc":"2.0","method":"exit","params":null}),
        ];
        let mut input = Vec::new();
        for message in messages {
            let body = serde_json::to_vec(&message).unwrap();
            write!(input, "Content-Length: {}\r\n\r\n", body.len()).unwrap();
            input.extend(body);
        }
        let mut child = Command::new(env!("CARGO_BIN_EXE_aethc"))
            .args(["lsp", "--target", target])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(&input).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{output:?}");
        let mut bytes = output.stdout.as_slice();
        let mut found = false;
        while !bytes.is_empty() {
            let end = bytes.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
            let header = std::str::from_utf8(&bytes[..end]).unwrap();
            let length: usize = header
                .strip_prefix("Content-Length: ")
                .unwrap()
                .parse()
                .unwrap();
            let message: Value = serde_json::from_slice(&bytes[end + 4..end + 4 + length]).unwrap();
            bytes = &bytes[end + 4 + length..];
            if message["method"] == "textDocument/publishDiagnostics" {
                found = true;
                let diagnostics = message["params"]["diagnostics"].as_array().unwrap();
                assert_eq!(!diagnostics.is_empty(), has_error, "{target}: {message}");
            }
        }
        assert!(found, "missing LSP diagnostics");
    }
    fs::remove_dir_all(work).unwrap();
}
