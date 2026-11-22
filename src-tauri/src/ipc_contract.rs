use specta_typescript::Typescript;
use std::fs;
use std::path::PathBuf;
use tauri_specta::{collect_commands, Builder};

#[test]
fn ipc_contract() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../src/ipc.d.ts");
    let builder = Builder::<tauri::Wry>::new()
        .commands(collect_commands![super::commands::capture::js_log]);
    let dir = tempfile::tempdir().expect("create a temporary export directory");
    let generated = dir.path().join("ipc.d.ts");
    builder
        .export(Typescript::default(), &generated)
        .expect("export Tauri command signatures");
    let output = fs::read_to_string(&generated).expect("read generated Tauri contract");
    if std::env::var_os("FLOATBALL_REGENERATE_CONTRACT").is_some() {
        fs::write(&path, output).expect("write src/ipc.d.ts");
    } else {
        let current = fs::read_to_string(&path).expect("read src/ipc.d.ts");
        assert!(
            current.replace("\r\n", "\n") == output.replace("\r\n", "\n"),
            "src/ipc.d.ts is stale. Run cargo test ipc_contract with FLOATBALL_REGENERATE_CONTRACT=1."
        );
    }
}
