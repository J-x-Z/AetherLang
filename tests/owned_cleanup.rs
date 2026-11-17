//! Destructor contracts reject ambiguity and propagate actual cleanup effects.
use std::{fs, process::Command};

#[test]
fn destructor_registration_and_cleanup_effects_are_checked() {
    let work = std::env::temp_dir().join(format!("aether-drop-contracts-{}", std::process::id()));
    fs::create_dir_all(&work).unwrap();
    fs::write(work.join("common.aeth"), "pub struct H{v:i32}").unwrap();
    fs::write(
        work.join("left.aeth"),
        "use common::H;@drop fn cleanup(x:own H){}",
    )
    .unwrap();
    fs::write(
        work.join("right.aeth"),
        "use common::H;@drop fn cleanup(x:own H){}",
    )
    .unwrap();
    for (name,source,code) in [
        ("duplicate","struct H{v:i32} @drop fn one(x:own H){} @drop fn two(x:own H){} fn main(){}","E2405"),
        ("alias","struct H{v:i32} type Alias=H; @drop fn one(x:own H){} @drop fn two(x:own Alias){} fn main(){}","E2405"),
        ("modules","mod left;mod right;fn main(){}","E2405"),
        ("copy_parameter","struct H{v:i32}@drop fn bad(x:H){}fn main(){}","E2405"),
        ("return_value","struct H{v:i32}@drop fn bad(x:own H)->i32{0}fn main(){}","E2405"),
        ("generic","struct H{v:i32}@drop fn bad<T>(x:T){}fn main(){}","E2405"),
        ("foreign","struct H{v:i32}extern \"C\"{@drop fn bad(x:i32);}fn main(){}","E2405"),
        ("marker","@drop struct H{v:i32}fn main(){}","E2405"),
        ("mutation","struct H{v:i32}fn mutate(x:&mut own H){x.v=2;}fn main(){}","E2405"),
        ("mutable_field_borrow","struct H{v:i32}fn mutate(x:&mut own H)->&mut i32{&mut x.v}fn main(){}","E2405"),
        ("effect","struct H{v:i32}@drop fn destroy(x:own H){println_i64(x.v as i64);}fn discard(x:own H) pure{}fn main(){}","E2200"),
        ("wrapper_effect","struct H{v:i32}@drop fn destroy(x:own H){println_i64(x.v as i64);}fn discard(x:own H){}fn wrapper(x:own H) pure{discard(x);}fn main(){}","E2200"),
        ("temporary_effect","struct H{v:i32}@drop fn destroy(x:own H){println_i64(x.v as i64);}fn take(x:own H,n:i32){}fn fail(x:own H) pure{take(x,{return;});}fn main(){}","E2200"),
    ] {
        let path=work.join(format!("{name}.aeth"));fs::write(&path,source).unwrap();
        for mode in ["check","build"] {
            let mut command=Command::new(env!("CARGO_BIN_EXE_aethc"));command.args([mode,"--error-format","json"]).arg(&path);
            if mode=="build" {command.args(["--backend","c","-o"]).arg(work.join(name));}
            let output=command.output().unwrap();
            assert_eq!(output.status.code(),Some(1),"{name}/{mode}: {output:?}");
            let diagnostic:serde_json::Value=serde_json::from_slice(&output.stderr).unwrap();
            assert_eq!(diagnostic["code"],code,"{name}/{mode}: {diagnostic}");
        }
    }
    let source = work.join("pure_transfer.aeth");
    fs::write(&source,"struct H{v:i32}@drop fn destroy(x:own H){println_i64(x.v as i64);}fn identity(x:own H)->own H pure{x}fn borrow(x:&own H)->i32 effect[read]{x.v}fn main(){}").unwrap();
    assert!(Command::new(env!("CARGO_BIN_EXE_aethc"))
        .arg("check")
        .arg(source)
        .status()
        .unwrap()
        .success());
    fs::remove_dir_all(work).unwrap();
}
