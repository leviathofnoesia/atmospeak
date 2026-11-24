use crate::models::{DictionaryEntry, Snippet};
use crate::services::cleanup;

/// Text policy frozen at capture start, shared by preview and final dictation.
#[derive(Debug, Clone)]
pub struct DictationTextContext {
    pub cleanup_enabled: bool,
    pub dictionary: Vec<DictionaryEntry>,
    pub snippets: Vec<Snippet>,
}

impl DictationTextContext {
    pub fn prepare_text(&self, text: &str) -> String {
        if self.cleanup_enabled {
            cleanup::clean_text(text, &self.dictionary, &self.snippets)
        } else {
            text.trim().to_string()
        }
    }
}
