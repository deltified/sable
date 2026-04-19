use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub type FileId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Span {
    pub file_id: FileId,
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(file_id: FileId, start: usize, end: usize) -> Self {
        Self {
            file_id,
            start,
            end,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SourceFile {
    pub id: FileId,
    pub path: PathBuf,
    pub text: String,
    pub line_starts: Vec<usize>,
}

impl SourceFile {
    fn new(id: FileId, path: PathBuf, text: String) -> Self {
        let mut line_starts = Vec::with_capacity(text.len() / 32 + 2);
        line_starts.push(0);
        for (idx, byte) in text.as_bytes().iter().enumerate() {
            if *byte == b'\n' {
                line_starts.push(idx + 1);
            }
        }

        Self {
            id,
            path,
            text,
            line_starts,
        }
    }

    pub fn line_col(&self, byte_offset: usize) -> (usize, usize) {
        let idx = self
            .line_starts
            .partition_point(|line_start| *line_start <= byte_offset)
            .saturating_sub(1);
        let line_start = self.line_starts[idx];
        (idx + 1, byte_offset.saturating_sub(line_start) + 1)
    }
}

#[derive(Debug, Default)]
pub struct SourceDb {
    files: Vec<SourceFile>,
    path_to_id: BTreeMap<PathBuf, FileId>,
}

impl SourceDb {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_file<P: AsRef<Path>>(&mut self, path: P) -> Result<FileId> {
        let canonical = normalize_path(path.as_ref())?;
        if let Some(id) = self.path_to_id.get(&canonical) {
            return Ok(*id);
        }

        let text = std::fs::read_to_string(&canonical)
            .with_context(|| format!("failed to read source file {}", canonical.display()))?;
        let id = self.files.len() as FileId;
        let file = SourceFile::new(id, canonical.clone(), text);
        self.files.push(file);
        self.path_to_id.insert(canonical, id);
        Ok(id)
    }

    pub fn file(&self, id: FileId) -> &SourceFile {
        &self.files[id as usize]
    }

    pub fn source(&self, id: FileId) -> &str {
        &self.file(id).text
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }
}

fn normalize_path(path: &Path) -> Result<PathBuf> {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return Ok(canonical);
    }

    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }

    let current = std::env::current_dir().context("failed to read current working directory")?;
    Ok(current.join(path))
}
