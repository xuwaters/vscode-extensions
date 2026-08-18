//! The virtual file system: an open-document overlay over a [`FileProvider`].
//!
//! A file open in the editor is served from its in-memory [`Source`], kept
//! incrementally current through `Source::edit`, and is never read from disk.
//! Closed files fall through to the provider, cached for the duration of one
//! compile and dropped at the next [`Vfs::reset`] — the same lifecycle
//! `typst-cli`'s watch loop uses.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Mutex;

use typst::diag::{FileError, FileResult};
use typst::foundations::Bytes;
use typst::syntax::{FileId, Source};

use crate::ports::FileProvider;

/// Open documents plus a per-compile cache of everything read from the host.
pub struct Vfs<F> {
    provider: F,
    /// Documents the editor has open. Authoritative over the provider.
    open: HashMap<FileId, Source>,
    /// Sources read from the host during this compile.
    sources: Mutex<HashMap<FileId, FileResult<Source>>>,
    /// Raw bytes read from the host during this compile.
    files: Mutex<HashMap<FileId, FileResult<Bytes>>>,
}

impl<F: FileProvider> Vfs<F> {
    /// Create a VFS over the given provider.
    pub fn new(provider: F) -> Self {
        Self {
            provider,
            open: HashMap::new(),
            sources: Mutex::new(HashMap::new()),
            files: Mutex::new(HashMap::new()),
        }
    }

    /// The underlying provider, for directory listings and the like.
    pub fn provider(&self) -> &F {
        &self.provider
    }

    /// Register a document as open, with the editor's copy of its text.
    pub fn open(&mut self, id: FileId, text: String) {
        self.open.insert(id, Source::new(id, text));
        self.invalidate(id);
    }

    /// Drop the overlay for a document, so it is read from the host again.
    pub fn close(&mut self, id: FileId) {
        self.open.remove(&id);
        self.invalidate(id);
    }

    /// Whether the editor has this document open.
    pub fn is_open(&self, id: FileId) -> bool {
        self.open.contains_key(&id)
    }

    /// Apply an incremental edit to an open document.
    ///
    /// Returns `false` if the document is not open, or if the range does not
    /// lie on character boundaries — the caller should then resynchronize with
    /// a full [`Vfs::replace`].
    pub fn edit(&mut self, id: FileId, range: Range<usize>, with: &str) -> bool {
        let Some(source) = self.open.get_mut(&id) else { return false };
        let text = source.text();
        if range.start > range.end
            || range.end > text.len()
            || !text.is_char_boundary(range.start)
            || !text.is_char_boundary(range.end)
        {
            return false;
        }
        source.edit(range, with);
        true
    }

    /// Replace an open document's text wholesale.
    pub fn replace(&mut self, id: FileId, text: &str) -> bool {
        let Some(source) = self.open.get_mut(&id) else { return false };
        source.replace(text);
        true
    }

    /// The in-memory source for an open document.
    pub fn opened(&self, id: FileId) -> Option<&Source> {
        self.open.get(&id)
    }

    /// Every open document's id.
    pub fn open_ids(&self) -> impl Iterator<Item = FileId> + '_ {
        self.open.keys().copied()
    }

    /// Read a source file, preferring the overlay.
    pub fn source(&self, id: FileId) -> FileResult<Source> {
        if let Some(source) = self.open.get(&id) {
            return Ok(source.clone());
        }

        if let Some(hit) = self.sources.lock().unwrap().get(&id) {
            return hit.clone();
        }

        let result = self.read_source(id);
        self.sources.lock().unwrap().insert(id, result.clone());
        result
    }

    /// Read a file's raw bytes, preferring the overlay.
    pub fn file(&self, id: FileId) -> FileResult<Bytes> {
        if let Some(source) = self.open.get(&id) {
            return Ok(Bytes::from_string(source.text().to_string()));
        }

        if let Some(hit) = self.files.lock().unwrap().get(&id) {
            return hit.clone();
        }

        let path = id.get();
        let result = self.provider.read(path.root(), path.vpath());
        self.files.lock().unwrap().insert(id, result.clone());
        result
    }

    /// List a directory through the provider.
    pub fn list(&self, id: FileId) -> Vec<String> {
        let path = id.get();
        self.provider.list(path.root(), path.vpath())
    }

    /// Every file id this session has touched — open documents plus everything
    /// read during the last compile. Feeds `IdeWorld::files` for path
    /// completions.
    pub fn known_ids(&self) -> Vec<FileId> {
        let mut ids: Vec<FileId> = self.open.keys().copied().collect();
        for id in self.sources.lock().unwrap().keys() {
            if !ids.contains(id) {
                ids.push(*id);
            }
        }
        for id in self.files.lock().unwrap().keys() {
            if !ids.contains(id) {
                ids.push(*id);
            }
        }
        ids
    }

    /// Drop everything read from the host, keeping open documents.
    ///
    /// Called once at the start of each compile so a file changed on disk is
    /// picked up, exactly as `typst-cli` does between watch iterations.
    pub fn reset(&self) {
        self.sources.lock().unwrap().clear();
        self.files.lock().unwrap().clear();
    }

    /// Forget one file's cached reads.
    pub fn invalidate(&self, id: FileId) {
        self.sources.lock().unwrap().remove(&id);
        self.files.lock().unwrap().remove(&id);
    }

    fn read_source(&self, id: FileId) -> FileResult<Source> {
        let path = id.get();
        let bytes = self.provider.read(path.root(), path.vpath())?;
        let text = std::str::from_utf8(&bytes).map_err(|_| FileError::InvalidUtf8)?;
        Ok(Source::new(id, text.into()))
    }
}
