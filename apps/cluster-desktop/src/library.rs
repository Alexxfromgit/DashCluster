//! The set of dashboards the user can cycle through: built-ins, the user's dashboards
//! folder, and any extra files passed on the command line.

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
    /// images next to it.
    File(PathBuf),
}

impl Entry {
    pub fn label(&self) -> String {
        match self {
            Entry::Builtin { file, .. } => file.trim_end_matches(".json5").to_owned(),
            Entry::File(p) => {
                let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                if stem == "dashboard" {
                    p.parent().and_then(Path::file_name).map_or(stem, |n| n.to_string_lossy().into_owned())
                } else {
                    stem
                }
            }
        }
    }

    pub fn path(&self) -> Option<&Path> {
        match self {
            Entry::File(p) => Some(p),
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
            Entry::File(p) => std::fs::read_to_string(p).map_err(|e| anyhow::anyhow!("{}: {e}", p.display())),
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

fn scan_dir(dir: &Path) -> Vec<Entry> {
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
    out.into_iter().map(Entry::File).collect()
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
        let keep = self.entries.get(self.current).cloned();
        let mut entries: Vec<Entry> = BUILTIN.iter().map(|(file, src)| Entry::Builtin { file, src }).collect();
        entries.extend(scan_dir(&user_dir()));
        for p in &self.extra {
            if p.is_dir() {
                entries.extend(scan_dir(p));
            } else {
                entries.push(Entry::File(p.clone()));
            }
        }
        let mut seen = std::collections::HashSet::new();
        entries.retain(|e| {
            seen.insert(
                e.path()
                    .map(|p| p.canonicalize().unwrap_or_else(|_| p.to_path_buf()))
                    .map_or_else(|| e.label(), |p| p.display().to_string()),
            )
        });
        self.entries = entries;
        self.current = keep.and_then(|k| self.entries.iter().position(|e| *e == k)).unwrap_or(0);
    }

    /// Selects by built-in name ("race"), label, or path. Returns false if not found.
    pub fn select(&mut self, name: &str) -> bool {
        let found =
            self.entries.iter().position(|e| e.label() == name || e.path().is_some_and(|p| p == Path::new(name)));
        if let Some(i) = found {
            self.current = i;
        }
        found.is_some()
    }

    pub fn select_path(&mut self, path: &Path) -> bool {
        let want = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let found = self
            .entries
            .iter()
            .position(|e| e.path().is_some_and(|p| p.canonicalize().unwrap_or_else(|_| p.to_path_buf()) == want));
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

    /// Index of the first non-built-in entry from `extra` (for "show my file first").
    pub fn first_extra(&self) -> Option<usize> {
        let first = self.extra.first()?;
        self.entries.iter().position(|e| e.path().is_some_and(|p| p.starts_with(first) || p == first))
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
}
