//! Collects the license texts of every third-party crate the desktop app (and the web
//! display it embeds) links, for shipping next to the binaries:
//!
//!     cargo run -p license-notices -- --out THIRD-PARTY-LICENSES.txt
//!
//! Walks `cargo metadata`'s resolved graph from the app crates through normal (not dev or
//! build) dependencies, reads each crate's LICENSE/COPYING/NOTICE files from its source, and
//! prints identical texts once with the list of crates that use them.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::process::Command;

const ROOTS: &[&str] = &["cluster-desktop", "cluster-web"];

fn license_files(dir: &Path) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let upper = name.to_ascii_uppercase();
            let is_license = ["LICENSE", "LICENCE", "COPYING", "NOTICE", "UNLICENSE", "COPYRIGHT"]
                .iter()
                .any(|p| upper.starts_with(p));
            (is_license && e.path().is_file())
                .then(|| std::fs::read_to_string(e.path()).ok().map(|t| (name, t.trim().to_owned())))
                .flatten()
        })
        .collect();
    out.sort();
    out
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let out_path = args.iter().position(|a| a == "--out").and_then(|i| args.get(i + 1));

    let output = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["metadata", "--format-version", "1"])
        .output()?;
    anyhow::ensure!(output.status.success(), "cargo metadata failed: {}", String::from_utf8_lossy(&output.stderr));
    let meta: Value = serde_json::from_slice(&output.stdout)?;

    let packages: HashMap<&str, &Value> =
        meta["packages"].as_array().into_iter().flatten().filter_map(|p| Some((p["id"].as_str()?, p))).collect();
    let workspace: BTreeSet<&str> =
        meta["workspace_members"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    let nodes: HashMap<&str, &Value> = meta["resolve"]["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|n| Some((n["id"].as_str()?, n)))
        .collect();

    // Breadth-first over normal dependencies from the app crates.
    let mut todo: Vec<&str> = packages
        .iter()
        .filter(|(id, p)| workspace.contains(*id) && ROOTS.contains(&p["name"].as_str().unwrap_or("")))
        .map(|(id, _)| *id)
        .collect();
    let mut seen: BTreeSet<&str> = todo.iter().copied().collect();
    while let Some(id) = todo.pop() {
        for dep in nodes.get(id).and_then(|n| n["deps"].as_array()).into_iter().flatten() {
            let normal = dep["dep_kinds"].as_array().into_iter().flatten().any(|k| k["kind"].is_null());
            if let Some(pkg) = dep["pkg"].as_str()
                && normal
                && seen.insert(pkg)
            {
                todo.push(pkg);
            }
        }
    }

    // license text → crates using it (identical texts are printed once).
    let mut by_text: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut crates = 0;
    for id in seen.iter().filter(|id| !workspace.contains(*id)) {
        let p = packages[id];
        crates += 1;
        let label = format!(
            "{} {} ({})",
            p["name"].as_str().unwrap_or("?"),
            p["version"].as_str().unwrap_or("?"),
            p["license"].as_str().unwrap_or("license not declared")
        );
        let dir = Path::new(p["manifest_path"].as_str().unwrap_or("")).parent().map(Path::to_path_buf);
        let files = dir.as_deref().map(license_files).unwrap_or_default();
        if files.is_empty() {
            by_text.entry(String::new()).or_default().insert(label.clone());
        }
        for (name, text) in files {
            by_text.entry(format!("--- {name} ---\n{text}")).or_default().insert(label.clone());
        }
    }

    let mut doc = format!(
        "Third-party licenses\n====================\n\nDashCluster (MIT, see LICENSE) is built from {crates} third-party Rust crates.\nTheir license texts follow; identical texts are listed once. The list errs on the side of\ncompleteness and may include crates used only on other platforms. See also\nTHIRD-PARTY-NOTICES.md (Slint, the Inter font, trademarks).\n"
    );
    for (text, users) in &by_text {
        doc.push_str("\n\n================================================================================\n");
        doc.push_str("Used by:\n");
        for u in users {
            doc.push_str(&format!("  {u}\n"));
        }
        if text.is_empty() {
            doc.push_str("\n(These crates ship no license file; their declared license applies,\nsee https://spdx.org/licenses/ for the standard text.)\n");
        } else {
            doc.push('\n');
            doc.push_str(text);
            doc.push('\n');
        }
    }
    match out_path {
        Some(p) => {
            std::fs::write(p, doc)?;
            eprintln!("wrote {p} ({crates} crates, {} distinct texts)", by_text.len());
        }
        None => print!("{doc}"),
    }
    Ok(())
}
