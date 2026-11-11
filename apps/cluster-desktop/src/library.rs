//! The set of dashboards the user can cycle through: built-ins plus files on disk.

use cluster_layout::{BUILTIN, Compiled};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub enum Entry {
    Builtin { file: &'static str, src: &'static str },
    File(PathBuf),
}

impl Entry {
    pub fn label(&self) -> String {
        match self {
            Entry::Builtin { file, .. } => file.trim_end_matches(".json5").to_owned(),
            Entry::File(p) => {
                p.file_stem().map_or_else(|| p.display().to_string(), |s| s.to_string_lossy().into_owned())
            }
        }
    }

    pub fn path(&self) -> Option<&Path> {
        match self {
            Entry::File(p) => Some(p),
            Entry::Builtin { .. } => None,
        }
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
        match self {
            Entry::Builtin { src, .. } => Ok((Compiled::from_json5(src)?, None)),
            Entry::File(p) => {
                let text = std::fs::read_to_string(p).map_err(|e| anyhow::anyhow!("{}: {e}", p.display()))?;
                let c = Compiled::from_json5(&text).map_err(|e| anyhow::anyhow!("{}: {e}", p.display()))?;
                Ok((c, p.parent().map(Path::to_path_buf)))
            }
        }
    }
}

pub struct Library {
    pub entries: Vec<Entry>,
    pub current: usize,
}

impl Library {
    /// Built-ins first, then each `extra` path (a .json5 file, or a folder of them).
    pub fn new(extra: &[PathBuf]) -> Self {
        let mut entries: Vec<Entry> = BUILTIN.iter().map(|(file, src)| Entry::Builtin { file, src }).collect();
        for p in extra {
            if p.is_dir() {
                let mut files: Vec<PathBuf> = std::fs::read_dir(p)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|e| e == "json5" || e == "json"))
                    .collect();
                files.sort();
                entries.extend(files.into_iter().map(Entry::File));
            } else {
                entries.push(Entry::File(p.clone()));
            }
        }
        Library { entries, current: 0 }
    }

    /// Selects by built-in name ("race"), file stem, or path. Returns false if not found.
    pub fn select(&mut self, name: &str) -> bool {
        let found =
            self.entries.iter().position(|e| e.label() == name || e.path().is_some_and(|p| p == Path::new(name)));
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
}
