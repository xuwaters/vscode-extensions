//! Lazy font loading over a [`FontProvider`].
//!
//! The `FontBook` is built from metadata alone, so it can describe every font
//! on the machine while holding none of their bytes. Each face gets a slot; the
//! first time a document selects it, the slot calls
//! [`FontProvider::data`](crate::ports::FontProvider::data) and constructs the
//! [`Font`]. A machine with 400 MB of installed fonts contributes ~2 KB of
//! metadata per face and zero bytes of glyph data until something is typeset in
//! it.

use std::sync::OnceLock;

use typst::text::{Font, FontBook};

use crate::ports::FontProvider;

/// One lazily-materialized font per known face.
pub struct FontSlots<T> {
    provider: T,
    slots: Vec<OnceLock<Option<Font>>>,
}

impl<T: FontProvider> FontSlots<T> {
    /// Create slots for every face the provider knows about.
    pub fn new(provider: T) -> Self {
        let slots = provider.faces().iter().map(|_| OnceLock::new()).collect();
        Self { provider, slots }
    }

    /// Build the book that describes these faces, without loading any of them.
    pub fn book(&self) -> FontBook {
        FontBook::from_infos(self.provider.faces().iter().map(|face| face.info.clone()))
    }

    /// How many faces are known.
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Whether no fonts are available at all.
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// The provider, for host-side queries.
    pub fn provider(&self) -> &T {
        &self.provider
    }

    /// Load a face, or return the already-loaded one.
    ///
    /// The index may be out of bounds: typst calls `World::font` with indices
    /// from an outdated font book during incremental validation, which is
    /// documented upstream and must not panic.
    pub fn font(&self, index: usize) -> Option<Font> {
        let slot = self.slots.get(index)?;
        slot.get_or_init(|| {
            let face = self.provider.faces().get(index)?;
            let data = self.provider.data(index)?;
            Font::new(data, face.index)
        })
        .clone()
    }
}
