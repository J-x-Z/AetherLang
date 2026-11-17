//! Nominal recursive types keep one identity across forward field snapshots.
use std::{
    fs,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

fn check(source: &std::path::Path) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_aethc"))
        .args(["check", "--error-format", "json"])
        .arg(source)
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
            panic!("recursive value type analysis failed to terminate");
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn recursive_vec_references_resolve_without_structural_type_conflation() {
    let work = std::env::temp_dir().join(format!("aether-recursive-values-{}", std::process::id()));
    fs::create_dir_all(&work).unwrap();
    let prefix = "use vec::{Vec,vec_new};pub struct Node{number:i32,children:Vec<Node>}";
    // The original reported program: it does not execute its child access.
    // The native recursive_vec fixture separately proves nonempty push/access/free.
    let original = work.join("original.aeth");
    fs::write(&original, format!("{prefix} fn visit(node:*Node){{let child:*Node=&node.children.data[0] as *Node;if child.number==1{{return;}}}}fn main()->i32{{let mut root:Node=Node{{number:0,children:vec_new()}};if root.children.len>0{{visit(&root as *Node);}}0}}" )).unwrap();
    let result = check(&original);
    assert_eq!(
        result.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );

    for (name, source, code) in [
        ("distinct_nominal", format!("{prefix} struct Other{{number:i32,children:Vec<Node>}}fn bad(node:*Node)->*Other{{&node.children.data[0] as *Other}}fn main()->i32{{0}}"), "E2100"),
        ("distinct_instantiation", "struct Item<T>{value:T}fn bad(value:&Item<i32>)->*Item<i64>{value as *Item<i64>}fn main()->i32{0}".into(), "E2100"),
        ("immutable_reference", format!("{prefix} fn bad(node:*Node){{let invalid:&mut Node=&node.children.data[0];}}fn main()->i32{{0}}"), "E2100"),
        ("nested_reference_qualifier", format!("{prefix} fn bad(value:& &mut Node)->*&Node{{value as *&Node}}fn main()->i32{{0}}"), "E2100"),
        ("missing_field", format!("{prefix} fn bad(node:*Node)->i32{{node.children.data[0].missing}}fn main()->i32{{0}}"), "E2102"),
        ("module_identity", "mod left{pub struct Node{value:i32}}mod right{pub struct Node{value:i32}}fn bad(value:&left::Node)->*right::Node{value as *right::Node}fn main()->i32{0}".into(), "E2100"),
    ] {
        let path = work.join(format!("{name}.aeth"));
        fs::write(&path, source).unwrap();
        let result = check(&path);
        assert_eq!(result.status.code(), Some(1), "{name}: {:?}\n{}", result.status, String::from_utf8_lossy(&result.stderr));
        let diagnostic: serde_json::Value = serde_json::from_slice(&result.stderr).unwrap();
        assert_eq!(diagnostic["code"], code, "{name}: {diagnostic}");
    }
    fs::remove_dir_all(work).unwrap();
}
