use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os()
        .nth(1)
        .ok_or("Usage: gen_ipc <output-path>")?;
    floatball_lib::export_ipc(&PathBuf::from(output))
}
