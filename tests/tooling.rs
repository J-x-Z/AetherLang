use std::{fs,io::Write,path::PathBuf,process::{Command,Stdio},sync::atomic::{AtomicU64,Ordering}};
use serde_json::{json,Value};
static NEXT:AtomicU64=AtomicU64::new(0);
struct Project(PathBuf);
impl Project {
    fn new()->Self{let path=std::env::temp_dir().join(format!("aether-tooling-{}-{}",std::process::id(),NEXT.fetch_add(1,Ordering::Relaxed)));fs::create_dir_all(&path).unwrap();Self(path)}
    fn file(&self,name:&str,text:&str)->PathBuf{let path=self.0.join(name);fs::write(&path,text).unwrap();path}
}
impl Drop for Project{fn drop(&mut self){let _=fs::remove_dir_all(&self.0);}}
fn compiler()->Command{Command::new(env!("CARGO_BIN_EXE_aethc"))}
fn uri(path:&std::path::Path)->String{format!("file://{}",path.to_string_lossy().replace('%',"%25").replace(' ',"%20"))}
#[test]
fn structured_diagnostics_use_original_script_and_imported_sources(){
    let p=Project::new();
    let script=p.file("unicode.ath","def main():\n    print(\"你好😀\")\n    return True\n");
    let run=compiler().args(["check","--error-format","json"]).arg(&script).output().unwrap();
    assert!(!run.status.success());
    let diagnostic:Value=serde_json::from_slice(&run.stderr).unwrap();
    assert_eq!(diagnostic["code"],"E2100");assert_eq!(diagnostic["location"]["line"],3);
    assert_eq!(diagnostic["location"]["file"],script.to_string_lossy().as_ref());
    assert!(!diagnostic["message"].as_str().unwrap().contains("__aeth_script"));
    let child=p.file("child.aeth","pub fn answer() -> i32 { return true; }");
    let main=p.file("main.aeth","use child::answer; fn main() -> i32 { return answer(); }");
    let run=compiler().args(["check","--error-format","json"]).arg(main).output().unwrap();
    let diagnostic:Value=serde_json::from_slice(&run.stderr).unwrap();
    assert_eq!(diagnostic["location"]["file"],child.to_string_lossy().as_ref());
}
#[test]
fn symbol_query_resolves_imported_function_and_distinct_shadowed_bindings(){
    let p=Project::new();let child=p.file("child.aeth","pub fn answer() -> i32 { return 42; }");
    let main=p.file("main.aeth","use child::answer; fn main() -> i32 { let x:i32=1; { let x:i32=2; answer(); x; } return x; }");
    let run=compiler().arg("symbols").arg(&main).output().unwrap();assert!(run.status.success(),"{}",String::from_utf8_lossy(&run.stderr));
    let index:Value=serde_json::from_slice(&run.stdout).unwrap();let symbols=index["symbols"].as_array().unwrap();
    let answer=symbols.iter().find(|s|s["name"]=="answer").unwrap();assert_eq!(answer["definition"]["file"],child.to_string_lossy().as_ref());
    let xs:Vec<_>=symbols.iter().filter(|s|s["name"]=="x").collect();assert_eq!(xs.len(),2);assert_ne!(xs[0]["id"],xs[1]["id"]);
    let refs=index["references"].as_array().unwrap();for x in xs{assert!(refs.iter().any(|r|r["symbol"]==x["id"]));}
}
#[test]
fn lsp_updates_unsaved_imports_and_serves_real_hover_definition_and_diagnostics(){
    let p=Project::new();let child=p.file("child.aeth","pub fn answer() -> i32 { return 42; }");
    let text="use child::answer;\nfn main() -> i32 { return answer(); }\n";let main=p.file("main.aeth",text);
    let pos=json!({"line":1,"character":26});
    let messages=vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        json!({"jsonrpc":"2.0","method":"initialized","params":{}}),
        json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":uri(&main),"version":1,"text":text}}}),
        json!({"jsonrpc":"2.0","id":2,"method":"textDocument/definition","params":{"textDocument":{"uri":uri(&main)},"position":pos}}),
        json!({"jsonrpc":"2.0","id":3,"method":"textDocument/hover","params":{"textDocument":{"uri":uri(&main)},"position":pos}}),
        json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{"textDocument":{"uri":uri(&child),"version":1,"text":"pub fn answer() -> i32 { return true; }"}}}),
        json!({"jsonrpc":"2.0","method":"textDocument/didChange","params":{"textDocument":{"uri":uri(&child),"version":2},"contentChanges":[{"text":"pub fn answer() -> i32 { return 7; }"}]}}),
        json!({"jsonrpc":"2.0","id":4,"method":"shutdown","params":null}),
        json!({"jsonrpc":"2.0","method":"exit","params":null}),
    ];
    let mut child_process=compiler().arg("lsp").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let mut input=child_process.stdin.take().unwrap();for message in messages{let body=serde_json::to_vec(&message).unwrap();write!(input,"Content-Length: {}\r\n\r\n",body.len()).unwrap();input.write_all(&body).unwrap();}drop(input);
    let output=child_process.wait_with_output().unwrap();assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));
    let mut bytes=output.stdout.as_slice();let mut replies=vec![];
    while !bytes.is_empty(){let boundary=bytes.windows(4).position(|w|w==b"\r\n\r\n").unwrap();let header=std::str::from_utf8(&bytes[..boundary]).unwrap();let count:usize=header.strip_prefix("Content-Length: ").unwrap().parse().unwrap();bytes=&bytes[boundary+4..];replies.push(serde_json::from_slice::<Value>(&bytes[..count]).unwrap());bytes=&bytes[count..];}
    assert_eq!(replies.iter().find(|r|r["id"]==2).unwrap()["result"]["uri"],uri(&child));
    assert!(replies.iter().find(|r|r["id"]==3).unwrap()["result"]["contents"]["value"].as_str().unwrap().contains("i32"));
    let changes:Vec<_>=replies.iter().filter(|r|r["method"]=="textDocument/publishDiagnostics"&&r["params"]["uri"]==uri(&child)).collect();
    assert!(changes.iter().any(|r|!r["params"]["diagnostics"].as_array().unwrap().is_empty()));
    assert!(changes.last().unwrap()["params"]["diagnostics"].as_array().unwrap().is_empty());
    assert_eq!(fs::read_to_string(child).unwrap(),"pub fn answer() -> i32 { return 42; }");
}
