//! The set of dashboards the user can cycle through: built-ins, the user's dashboards
//! folder, and extra files (command line, or saved elsewhere with "Save as").

use cluster_layout::{BUILTIN, Compiled};
use std::path::{Path, PathBuf};

/// Where the user's own dashboards live (created on first use).
pub fn user_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|d| d.data_dir().join("DashCluster").join("dashboards"))
        .unwrap_or_else(|| PathBuf::from("dashboards"))
}

#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    Builtin {
        file: &'static str,
        src: &'static str,
    },
    /// A `.json5` file. A folder-style dashboard is `<folder>/dashboard.json5` with its
    /// images next to it. `label` is unique within the library.
    File {
        path: PathBuf,
        label: String,
    },
}

/// The name a file would get before de-duplication.
fn base_label(p: &Path) -> String {
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    if stem == "dashboard" {
        p.parent().and_then(Path::file_name).map_or(stem, |n| n.to_string_lossy().into_owned())
    } else {
        stem
    }
}

impl Entry {
    /// Unique name, used by the phone display and to remember the last dashboard.
    pub fn label(&self) -> String {
        match self {
            Entry::Builtin { file, .. } => file.trim_end_matches(".json5").to_owned(),
            Entry::File { label, .. } => label.clone(),
        }
    }

    pub fn path(&self) -> Option<&Path> {
        match self {
            Entry::File { path, .. } => Some(path),
            Entry::Builtin { .. } => None,
        }
    }

    pub fn base_dir(&self) -> Option<PathBuf> {
        self.path().and_then(Path::parent).map(Path::to_path_buf)
    }

    /// The dashboard's JSON5 text.
    pub fn text(&self) -> anyhow::Result<String> {
        match self {
            Entry::Builtin { src, .. } => Ok((*src).to_owned()),
            Entry::File { path, .. } => {
                let len = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                anyhow::ensure!(
                    len <= cluster_layout::compile::MAX_SOURCE_BYTES as u64,
                    "{}: file is too large to be a dashboard",
                    path.display()
                );
                std::fs::read_to_string(path).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))
            }
        }
    }

    /// Parses and validates the dashboard. Returns it with the folder its relative
    /// image paths resolve against.
    pub fn load(&self) -> anyhow::Result<(Compiled, Option<PathBuf>)> {
        let text = self.text()?;
        let c = Compiled::from_json5(&text).map_err(|e| match self.path() {
            Some(p) => anyhow::anyhow!("{}: {e}", p.display()),
            None => anyhow::anyhow!("{e}"),
        })?;
        Ok((c, self.base_dir()))
    }
}

pub struct Library {
    pub entries: Vec<Entry>,
    pub current: usize,
    extra: Vec<PathBuf>,
}

fn scan_dir(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            let inner = p.join("dashboard.json5");
            if inner.is_file() {
                out.push(inner);
            }
        } else if p.extension().is_some_and(|e| e == "json5" || e == "json") {
            out.push(p);
        }
    }
    out.sort();
    out
}

fn same_file(a: &Path, b: &Path) -> bool {
    let canon = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    canon(a) == canon(b)
}

impl Library {
    /// Built-ins, then the user's dashboards folder, then each `extra` path (a .json5
    /// file or a folder of them).
    pub fn new(extra: &[PathBuf]) -> Self {
        let mut lib = Library { entries: vec![], current: 0, extra: extra.to_vec() };
        lib.rescan();
        lib
    }

    /// Re-reads the folders, keeping the current selection when it still exists.
    pub fn rescan(&mut self) {
        let keep = self.entries.get(self.current).map(|e| (e.path().map(Path::to_path_buf), e.label()));
        let mut files = scan_dir(&user_dir());
        for p in &self.extra {
            if p.is_dir() {
                files.extend(scan_dir(p));
            } else {
                files.push(p.clone());
            }
        }
        let mut entries: Vec<Entry> = BUILTIN.iter().map(|(file, src)| Entry::Builtin { file, src }).collect();
        for path in files {
            if entries.iter().any(|e| e.path().is_some_and(|q| same_file(q, &path))) {
                continue;
            }
            // Unique labels: a file named like a built-in (or another file) gets " 2", " 3", …
            let base = base_label(&path);
            let label = (1..)
                .map(|n| if n == 1 { base.clone() } else { format!("{base} {n}") })
                .find(|l| !entries.iter().any(|e| e.label() == *l))
                .expect("infinite");
            entries.push(Entry::File { path, label });
        }
        self.entries = entries;
        self.current = keep
            .and_then(|(path, label)| {
                self.entries.iter().position(|e| match (&path, e.path()) {
                    (Some(p), Some(q)) => same_file(p, q),
                    (None, None) => e.label() == label,
                    _ => false,
                })
            })
            .unwrap_or(0);
    }

    /// Adds a dashboard saved outside the scanned folders, so it stays in the list.
    pub fn add_extra(&mut self, path: &Path) {
        if !self.entries.iter().any(|e| e.path().is_some_and(|p| same_file(p, path))) {
            self.extra.push(path.to_path_buf());
            self.rescan();
        }
    }

    /// Selects by label ("race", "race 2") or path. Returns false if not found.
    pub fn select(&mut self, name: &str) -> bool {
        let found =
            self.entries.iter().position(|e| e.label() == name || e.path().is_some_and(|p| p == Path::new(name)));
        if let Some(i) = found {
            self.current = i;
        }
        found.is_some()
    }

    pub fn select_path(&mut self, path: &Path) -> bool {
        let found = self.entries.iter().position(|e| e.path().is_some_and(|p| same_file(p, path)));
        if let Some(i) = found {
            self.current = i;
        }
        found.is_some()
    }

    pub fn current(&self) -> &Entry {
        &self.entries[self.current]
    }

    pub fn step(&mut self, delta: isize) -> &Entry {
        let n = self.entries.len() as isize;
        self.current = ((self.current as isize + delta).rem_euclid(n)) as usize;
        self.current()
    }

    /// Index of the first entry from the command line's `--dashboards` (shown first).
    pub fn first_extra(&self) -> Option<usize> {
        let first = self.extra.first()?;
        self.entries.iter().position(|e| e.path().is_some_and(|p| p.starts_with(first) || p == first))
    }

    pub fn is_listed(&self, path: &Path) -> bool {
        self.entries.iter().any(|e| e.path().is_some_and(|p| same_file(p, path)))
    }
}

/// A file-system-friendly name.
pub fn slug(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if s.is_empty() { "dashboard".into() } else { s }
}

/// A path in `dir` named `base` (+ `-2`, `-3`, … if taken) with extension `ext`
/// (empty = folder).
pub fn unique_path(dir: &Path, base: &str, ext: &str) -> PathBuf {
    (1..)
        .map(|n| {
            let name = if n == 1 { base.to_owned() } else { format!("{base}-{n}") };
            if ext.is_empty() { dir.join(name) } else { dir.join(format!("{name}.{ext}")) }
        })
        .find(|p| !p.exists())
        .expect("infinite")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert_eq!(slug("Grand Tourer (copy)"), "grand-tourer-copy");
        assert_eq!(slug("  "), "dashboard");
    }

    #[test]
    fn labels_are_unique() {
        let dir = std::env::temp_dir().join(format!("dc-lib-{}", std::process::id()));
        let sub = dir.join("race");
        std::fs::create_dir_all(&sub).unwrap();
        let a = dir.join("race.json5");
        std::fs::write(&a, "{}").unwrap();
        std::fs::write(sub.join("dashboard.json5"), "{}").unwrap();
        let lib = Library::new(std::slice::from_ref(&dir));
        assert!(lib.entries.iter().any(|e| e.path().is_none() && e.label() == "race"), "built-in keeps its name");
        let mut mine: Vec<String> =
            lib.entries.iter().filter(|e| e.path().is_some_and(|p| p.starts_with(&dir))).map(Entry::label).collect();
        mine.sort();
        assert_eq!(mine.len(), 2);
        assert!(mine.iter().all(|l| l.starts_with("race ") && l != "race"), "{mine:?}");
        let mut lib2 = Library::new(&[]);
        lib2.add_extra(&a);
        assert!(lib2.is_listed(&a));
        std::fs::remove_dir_all(dir).ok();
    }
}
