//! Embeds the built web display (apps/cluster-web/dist) if present, so the desktop
//! binary can serve it with no extra files. Build it with `scripts/build-web.sh`.

use std::path::{Path, PathBuf};

fn main() {
    let dist = std::env::var("DASHCLUSTER_WEB_DIST")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/cluster-web/dist"));
    println!("cargo:rerun-if-env-changed=DASHCLUSTER_WEB_DIST");
    println!("cargo:rerun-if-changed={}", dist.display());
    let mut entries = String::new();
    if let Ok(rd) = std::fs::read_dir(&dist) {
        let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
        files.sort();
        for f in files {
            println!("cargo:rerun-if-changed={}", f.display());
            let name = f.file_name().unwrap().to_string_lossy().into_owned();
            let abs = f.canonicalize().unwrap();
            entries.push_str(&format!("    ({name:?}, include_bytes!({:?})),\n", abs.display().to_string()));
        }
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("web_assets.rs");
    std::fs::write(out, format!("pub static WEB_ASSETS: &[(&str, &[u8])] = &[\n{entries}];\n")).unwrap();
}
