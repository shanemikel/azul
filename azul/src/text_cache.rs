use std::sync::atomic::{Ordering, AtomicUsize};
use azul_css::{FontId, StyleFontSize};
use {
    FastHashMap,
    text_layout::Words,
    app_resources::AppResources,
};

/// Cache for accessing large amounts of text
#[derive(Debug, Default, Clone)]
pub struct TextCache {
    /// Mapping from the TextID to the actual, UTF-8 String
    ///
    /// This is stored outside of the actual glyph calculation, because usually you don't
    /// need the string, except for rebuilding a cached string (for example, when the font is changed)
    pub string_cache: FastHashMap<TextId, String>,
    /// Caches the layout of the strings / words.
    ///
    /// TextId -> FontId (to look up by font)
    /// FontId -> StyleFontSize (to categorize by size within a font)
    /// StyleFontSize -> layouted words (to cache the glyph widths on a per-font-size basis)
    pub layouted_strings_cache: FastHashMap<TextId, FastHashMap<FontId, FastHashMap<StyleFontSize, Words>>>,
}

impl TextCache {

    /// Add a new, large text to the resources
    pub fn add_text<S: Into<String>>(&mut self, text: S) -> TextId {
        let id = new_text_id();
        self.string_cache.insert(id, text.into());
        id
    }

    /// Removes a string from the string cache, but not the layouted text cache
    pub fn delete_string(&mut self, id: TextId) {
        self.string_cache.remove(&id);
    }

    /// Removes a string from the layouted text cache, but not the string cache
    pub fn delete_layouted_text(&mut self, id: TextId) {
        self.layouted_strings_cache.remove(&id);
    }

    /// Delete a text from both the string cache and the layouted text cache
    pub fn delete_text(&mut self, id: TextId) {
        self.delete_string(id);
        self.delete_layouted_text(id);
    }

    pub fn clear_all_texts(&mut self) {
        self.string_cache.clear();
        self.layouted_strings_cache.clear();
    }
}


/// This is used for caching large strings (in the `push_text` function)
/// In the cached version, you can lookup the text as well as the dimensions of
/// the words in the `AppResources`. For the `Uncached` version, you'll have to re-
/// calculate it on every frame.
///
/// TODO: It should be possible to switch this over to a `&'a str`, but currently
/// this leads to unsolvable borrowing issues.
#[derive(Debug)]
pub(crate) enum TextInfo {
    Cached(TextId),
    Uncached(String),
}

impl TextInfo {
    /// Returns if the inner text is empty.
    ///
    /// Returns true if the TextInfo::Cached TextId does not exist
    /// (since in that case, it is "empty", so to speak)
    pub(crate) fn is_empty_text(&self, app_resources: &AppResources)
    -> bool
    {
        use self::TextInfo::*;

        match self {
            Cached(text_id) => {
                match app_resources.text_cache.string_cache.get(text_id) {
                    Some(s) => s.is_empty(),
                    None => true,
                }
            }
            Uncached(s) => s.is_empty(),
        }
    }
}
