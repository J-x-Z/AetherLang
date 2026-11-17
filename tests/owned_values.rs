//! Affine owners transfer once, with the same diagnostic in check and build.
use std::{fs, process::Command};

#[test]
fn owned_transfer_boundaries_are_checked_before_codegen() {
    let work = std::env::temp_dir().join(format!("aether-owned-values-{}", std::process::id()));
    fs::create_dir_all(&work).unwrap();
    let prefix="struct Handle{raw:i32}fn make()->own Handle{unsafe{Handle{raw:3} as own Handle}}fn consume(value:own Handle){}";
    for (name, body, code) in [
        ("copy_close", "fn main(){let first:own Handle=make();let second:own Handle=first;consume(first);consume(second);}","E2301"),
        ("parameter", "fn main(){let first:own Handle=make();consume(first);consume(first);}","E2301"),
        ("return", "fn moved(value:own Handle)->own Handle{let next:own Handle=value;value}fn main(){}","E2301"),
        ("branch", "fn moved(flag:bool){let value:own Handle=make();if flag{consume(value);}consume(value);}fn main(){}","E2301"),
        ("loop", "fn moved(flag:bool){let value:own Handle=make();while flag{consume(value);}}fn main(){}","E2301"),
        ("borrowed", "fn moved(value:&own Handle)->own Handle{*value}fn main(){}","E2405"),
        ("temporary_borrow", "fn main(){let value:own Handle=make();let copy:own Handle=*(&value);}","E2405"),
        ("replace_live", "fn main(){let mut value:own Handle=make();value=make();}","E2405"),
        ("replace_maybe_live", "fn moved(flag:bool){let mut value:own Handle=make();if flag{consume(value);}value=make();}fn main(){}","E2405"),
        ("tuple", "fn main(){let value:own Handle=make();(value,);}","E2405"),
        ("array", "fn main(){let value:own Handle=make();[value];}","E2405"),
        ("foreign", "extern \"C\"{fn foreign(value:own Handle);}fn main(){}","E2405"),
        ("adopt", "fn main(){let value:own Handle=Handle{raw:3} as own Handle;}","E2405"),
        ("erase", "fn main(){let value:own Handle=make();let copied:Handle=value as Handle;}","E2405"),
        ("active_loan", "fn main(){let value:own Handle=make();let loan:&own Handle=&value;consume(value);let n:i32=loan.raw;}","E2302"),
        ("escape", "fn local()->&i32{let value:own Handle=make();&value.raw}fn main(){}","E2302"),
        ("alias", "type Owner=own Handle;fn main(){let value:Owner=make();let other:Owner=value;consume(value);}","E2301"),
    ] {
        let path=work.join(format!("{name}.aeth"));
        fs::write(&path,format!("{prefix}{body}")).unwrap();
        for mode in ["check","build"] {
            let mut command=Command::new(env!("CARGO_BIN_EXE_aethc"));
            command.args([mode,"--error-format","json"]).arg(&path);
            if mode=="build" {command.args(["--backend","c","-o"]).arg(work.join(name));}
            let output=command.output().unwrap();
            assert_eq!(output.status.code(),Some(1),"{name}/{mode}: {output:?}");
            let diagnostic:serde_json::Value=serde_json::from_slice(&output.stderr).unwrap();
            assert_eq!(diagnostic["code"],code,"{name}/{mode}: {diagnostic}");
        }
    }
    fs::remove_dir_all(work).unwrap();
}
