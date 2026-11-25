use std::{collections::BTreeMap, path::Path, sync::LazyLock};

use serde::{Deserialize, Serialize};

pub const AUTO_LANGUAGE: &str = "auto";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechModel {
    pub id: String,
    pub label: String,
    pub filename: String,
    pub bytes: u64,
    pub sha256: String,
    pub url: String,
    pub details: SpeechModelDetails,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechModelDetails {
    pub description: String,
    pub languages: Vec<String>,
    pub supports_streaming: bool,
    pub supports_language_detection: bool,
    pub recommended: bool,
    pub source_url: String,
    pub benchmark: Option<SpeechBenchmark>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechBenchmark {
    pub accuracy: BTreeMap<String, SpeechAccuracy>,
    pub cpu_seconds: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechAccuracy {
    pub grade: String,
    pub error_percent: f64,
}

impl SpeechModel {
    pub fn supports_language(&self, language: &str) -> bool {
        if language == AUTO_LANGUAGE {
            return self.details.supports_language_detection;
        }
        self.details
            .languages
            .iter()
            .any(|available| available == language)
    }

    pub fn language_or_default<'a>(&'a self, preferred: &'a str) -> &'a str {
        if self.supports_language(preferred) {
            preferred
        } else if self.supports_language(AUTO_LANGUAGE) {
            AUTO_LANGUAGE
        } else {
            &self.details.languages[0]
        }
    }
}

#[derive(Deserialize)]
struct Catalog {
    models: Vec<SpeechModel>,
}

static CATALOG: LazyLock<Catalog> = LazyLock::new(|| {
    serde_json::from_str(include_str!("speech_catalog.json"))
        .expect("generated Handy voice model catalog")
});

pub fn models() -> &'static [SpeechModel] {
    &CATALOG.models
}

pub fn by_id(id: &str) -> Option<&'static SpeechModel> {
    models().iter().find(|model| model.id == id)
}

pub fn by_path(path: &Path) -> Option<&'static SpeechModel> {
    let filename = path.file_name()?.to_str()?;
    models().iter().find(|model| model.filename == filename)
}
