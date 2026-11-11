use std::process::Command;

fn main() {
    // The engine commit this launcher is built with (the project's
    // submodule): the installed app's engine starts there (engine.rs, PIN).
    println!("cargo:rerun-if-changed=../.git/modules/engine/HEAD");
    let head = Command::new("git").args(["-C", "../engine", "rev-parse", "HEAD"]).output();
    if let Some(out) = head.ok().filter(|o| o.status.success()) {
        println!("cargo:rustc-env=NXL_ENGINE_PIN={}", String::from_utf8_lossy(&out.stdout).trim());
    }
    tauri_build::build()
}
