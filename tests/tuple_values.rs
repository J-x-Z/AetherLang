//! Unsupported callable values and tuple boundaries fail before either backend.
use std::{fs, process::Command};

#[test]
fn tuple_and_callable_boundaries_share_check_and_build_diagnostics() {
    let work = std::env::temp_dir().join(format!("aether-tuple-boundaries-{}", std::process::id()));
    fs::create_dir_all(&work).unwrap();
    for (name, source, code) in [
        (
            "projection",
            "fn main(){let pair:(i32,bool)=(1,true);pair.2;}",
            "E2102",
        ),
        ("closure", "fn main(){|x:i64|->i64{x+1};}", "E2403"),
        (
            "closure_body",
            "fn id<T>(value:T)->T{value}fn main(){|x:i64|{id()};}",
            "E2403",
        ),
        (
            "variadic",
            "extern \"C\"{fn take(tag:i32,...);}fn main(){take(1,(2,3));}",
            "E2404",
        ),
        (
            "callable",
            "fn apply(f:fn(i32)->i32)->i32{f(1)}fn main(){}",
            "E2403",
        ),
        (
            "foreign",
            "extern \"C\"{fn take(value:(i32,i64));}fn main(){}",
            "E2404",
        ),
        (
            "export",
            "#[no_mangle] fn make()->(i32,i64){(1,2)}fn main(){}",
            "E2404",
        ),
    ] {
        let source_path = work.join(format!("{name}.aeth"));
        fs::write(&source_path, source).unwrap();
        for mode in ["check", "build"] {
            let mut command = Command::new(env!("CARGO_BIN_EXE_aethc"));
            command
                .arg(mode)
                .args(["--error-format", "json"])
                .arg(&source_path);
            if mode == "build" {
                command.args(["--backend", "c", "-o"]).arg(work.join(name));
            }
            let output = command.output().unwrap();
            assert_eq!(output.status.code(), Some(1), "{name}/{mode}: {output:?}");
            let diagnostic: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
            assert_eq!(diagnostic["code"], code, "{name}/{mode}: {diagnostic}");
        }
    }
    fs::remove_dir_all(work).unwrap();
}
