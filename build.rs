//! Copies the DLSS runtime DLLs next to the built executable (they must ship with the game).
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=DLSS_SDK");
    let Ok(sdk) = std::env::var("DLSS_SDK") else { return };
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    // OUT_DIR = target/<profile>/build/<pkg>/out -> target/<profile>
    let Some(profile_dir) = out.ancestors().nth(3) else { return };
    for dll in ["nvngx_dlss.dll", "nvngx_dlssd.dll"] {
        let src = PathBuf::from(&sdk).join("lib/Windows_x86_64/rel").join(dll);
        if src.exists() {
            let _ = std::fs::copy(&src, profile_dir.join(dll));
        }
    }
}
