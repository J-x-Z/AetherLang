//! Recursive reference provenance must terminate without dropping local origins.
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

fn run_bounded(command: &mut Command) -> std::process::Output {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("reference-flow analysis did not converge within 10 seconds");
        }
        thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn recursive_reference_regions_terminate_and_preserve_escapes() {
    let work = std::env::temp_dir().join(format!("aether-recursive-flow-{}", std::process::id()));
    fs::create_dir_all(&work).unwrap();
    let compiler = env!("CARGO_BIN_EXE_aethc");
    let plain = work.join("plain.aeth");
    fs::write(&plain,"struct Node{next:&Node,value:i32}fn walk(start:&Node,count:i32)->i32{let mut p:&Node=start;let mut i:i32=0;while i<count{p=p.next;i+=1;}p.value}fn main()->i32{0}").unwrap();
    let checked = run_bounded(Command::new(compiler).arg("check").arg(&plain));
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/regressions/semantics/recursive_references.aeth");
    let checked = run_bounded(Command::new(compiler).arg("check").arg(&fixture));
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    for backend in if cfg!(feature = "llvm") {
        vec!["c", "llvm"]
    } else {
        vec!["c"]
    } {
        let executable = work.join(format!("recursive-{backend}"));
        let built = run_bounded(
            Command::new(compiler)
                .arg("build")
                .arg(&fixture)
                .args(["--backend", backend, "-O2", "-o"])
                .arg(&executable),
        );
        assert!(
            built.status.success(),
            "{}",
            String::from_utf8_lossy(&built.stderr)
        );
        assert!(run_bounded(&mut Command::new(executable)).status.success());
    }
    let prefix="enum Next{More(&Node),End}struct Node{next:Next,value:i32}fn last(start:&Node)->&Node{let mut p:&Node=start;loop{match p.next{Next::More(n)=>{p=n;},Next::End=>{return p;}}}}";
    let prefix = format!("{prefix}fn last_value(start:&Node)->&i32{{match start.next{{Next::More(next)=>last_value(next),Next::End=>&start.value}}}}");
    for (name,body) in [
        ("return-field-local","fn bad()->&i32{let end:Node=Node{next:Next::End,value:1};let head:Node=Node{next:Next::More(&end),value:2};last_value(&head)}"),
        ("return-local","fn bad()->&Node{let end:Node=Node{next:Next::End,value:1};let head:Node=Node{next:Next::More(&end),value:2};last(&head)}"),
        ("write-local","fn bad(out:&mut &Node,base:&Node){let local:Node=Node{next:Next::More(base),value:1};*out=last(&local);}"),
    ] {
        let path=work.join(format!("{name}.aeth"));fs::write(&path,format!("{prefix}{body}fn main()->i32{{0}}" )).unwrap();
        let checked=run_bounded(Command::new(compiler).args(["check","--error-format","json"]).arg(path));
        assert!(!checked.status.success(),"{name} unexpectedly accepted");
        let diagnostic:serde_json::Value=serde_json::from_slice(&checked.stderr).unwrap();
        assert_eq!(diagnostic["code"],"E2302","{name}: {diagnostic}");
    }
    fs::remove_dir_all(work).unwrap();
}

#[test]
fn nested_enum_coverage_and_array_references_execute() {
    let work = std::env::temp_dir().join(format!("aether-pattern-array-{}", std::process::id()));
    fs::create_dir_all(&work).unwrap();
    let compiler = env!("CARGO_BIN_EXE_aethc");
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/regressions/semantics/patterns_arrays.aeth");
    for backend in if cfg!(feature = "llvm") {
        vec!["c", "llvm"]
    } else {
        vec!["c"]
    } {
        for level in ["-O0", "-O2"] {
            let binary = work.join(format!("patterns-{backend}-{level}"));
            let built = run_bounded(
                Command::new(compiler)
                    .arg("build")
                    .arg(&source)
                    .args(["--backend", backend, level, "-o"])
                    .arg(&binary),
            );
            assert!(
                built.status.success(),
                "{}",
                String::from_utf8_lossy(&built.stderr)
            );
            let ran = run_bounded(&mut Command::new(binary));
            assert!(ran.status.success(), "{backend}/{level}: {:?}", ran.status);
        }
    }
    fs::remove_dir_all(work).unwrap();
}
