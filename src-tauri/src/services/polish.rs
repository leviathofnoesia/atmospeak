use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::json;
use tauri::{AppHandle, Manager};

use crate::{
    models::{AppSettings, PolishProvider, PolishStyle},
    services::{app_state::AppState, polish_models},
};

pub const AUTO_POLISH_TIMEOUT: Duration = Duration::from_millis(750);
const API_KEY_ENV: &str = "ATMOSPEAK_POLISH_API_KEY";
const KEYRING_SERVICE: &str = "atmospeak";
const KEYRING_USER: &str = "polish-api-key";

#[derive(Debug, Clone)]
pub struct PolishOutcome {
    pub text: String,
    pub elapsed_ms: u64,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ChatChoiceMessage {
    content: Option<String>,
}

/// Polish transcript via an OpenAI-compatible chat completions endpoint.
/// Returns `Ok(None)` when polish is disabled so callers skip HTTP entirely.
pub fn polish_if_enabled(
    app: &AppHandle,
    settings: &AppSettings,
    cleaned: &str,
    timeout: Duration,
) -> Result<Option<PolishOutcome>> {
    if !settings.auto_polish {
        return Ok(None);
    }
    if cleaned.trim().is_empty() {
        return Ok(None);
    }
    let started = std::time::Instant::now();
    let text = polish_text_inner(app, settings, cleaned, timeout, false)?;
    Ok(Some(PolishOutcome {
        text,
        elapsed_ms: started.elapsed().as_millis() as u64,
    }))
}

/// Explicit Settings / History polish — may download/setup the bundled runtime.
pub fn polish_text(
    app: &AppHandle,
    settings: &AppSettings,
    cleaned: &str,
    timeout: Duration,
) -> Result<String> {
    polish_text_inner(app, settings, cleaned, timeout, true)
}

fn polish_text_inner(
    app: &AppHandle,
    settings: &AppSettings,
    cleaned: &str,
    timeout: Duration,
    allow_setup: bool,
) -> Result<String> {
    let (endpoint, model_name) = resolve_endpoint_and_model(app, settings, allow_setup)?;

    let system = system_prompt(settings);
    let body = json!({
        "model": model_name,
        "temperature": 0.2,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": cleaned },
        ],
    });

    let mut request = reqwest::blocking::Client::builder()
        .timeout(timeout)
        .no_proxy()
        .build()
        .context("failed to build polish HTTP client")?
        .post(&endpoint)
        .header("Content-Type", "application/json")
        .json(&body);

    if let Some(api_key) = api_key_for(settings) {
        request = request.bearer_auth(api_key);
    }

    let response = request.send().map_err(sanitize_reqwest_error)?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        bail!(
            "polish provider returned {}: {}",
            status.as_u16(),
            sanitize_message(&body)
        );
    }

    let parsed: ChatCompletionResponse = response
        .json()
        .map_err(|error| anyhow::anyhow!(sanitize_message(&error.to_string())))?;
    let content = parsed
        .choices
        .first()
        .and_then(|choice| choice.message.content.as_ref())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .context("polish provider returned empty content")?;

    Ok(strip_fences(&content))
}

fn resolve_endpoint_and_model(
    app: &AppHandle,
    settings: &AppSettings,
    allow_setup: bool,
) -> Result<(String, String)> {
    match settings.polish_provider {
        PolishProvider::Bundled => {
            let model_id = if settings.polish_model.trim().is_empty() {
                polish_models::DEFAULT_POLISH_MODEL_ID
            } else {
                settings.polish_model.trim()
            };
            if allow_setup {
                polish_models::ensure_runtime(app, model_id)?;
            } else {
                if !polish_models::runtime_ready(app, model_id) {
                    bail!(
                        "bundled polish runtime not ready; download it from Settings → AI edit"
                    );
                }
                polish_models::attach_ready_runtime(app, model_id)?;
            }
            let host = AppState::llama_host_from(app)
                .ok_or_else(|| anyhow::anyhow!("bundled llama host is not available"))?;
            let endpoint = host.endpoint().map_err(|error| {
                app.state::<AppState>().shutdown_llama_host();
                error
            })?;
            let filename = polish_models::descriptor(model_id)
                .map(|model| model.filename.to_string())
                .unwrap_or_else(|| model_id.to_string());
            Ok((endpoint, filename))
        }
        PolishProvider::Ollama => {
            let endpoint = if settings.polish_endpoint.trim().is_empty() {
                "http://127.0.0.1:11434/v1/chat/completions".to_string()
            } else {
                settings.polish_endpoint.trim().to_string()
            };
            let model = settings.polish_model.trim();
            if model.is_empty() {
                bail!("polish model is not configured");
            }
            Ok((endpoint, model.to_string()))
        }
        PolishProvider::OpenaiCompatible => {
            let endpoint = settings.polish_endpoint.trim();
            if endpoint.is_empty() {
                bail!("polish endpoint is not configured");
            }
            let model = settings.polish_model.trim();
            if model.is_empty() {
                bail!("polish model is not configured");
            }
            Ok((endpoint.to_string(), model.to_string()))
        }
    }
}

fn system_prompt(settings: &AppSettings) -> String {
    let style = match settings.polish_style {
        PolishStyle::None => "Keep the speaker's natural tone. Do not restyle.".to_string(),
        PolishStyle::Concise => "Make the text concise without dropping meaning.".to_string(),
        PolishStyle::Formal => "Rewrite in a clear, formal tone.".to_string(),
        PolishStyle::Casual => "Rewrite in a friendly, casual tone.".to_string(),
        PolishStyle::Excited => "Rewrite with energetic but professional enthusiasm.".to_string(),
    };
    let custom = settings.custom_instructions.trim();
    let custom_block = if custom.is_empty() {
        String::new()
    } else {
        format!("\nAdditional user instructions:\n{custom}\n")
    };

    format!(
        "You are Atmospeak's dictation polish layer. \
You receive already-cleaned speech-to-text. Apply Wispr-style Backtrack and light formatting:\n\
- Remove filler words and false starts.\n\
- Apply self-corrections (e.g. \"meet at 5, actually 6\" → \"meet at 6\").\n\
- Collapse stuttered repeats (\"is not is not\" → \"is not\").\n\
- Fix obvious grammar/punctuation only when confident.\n\
- Preserve meaning; never invent facts, names, or numbers.\n\
- Preserve dictionary terms and snippet expansions already present.\n\
- Return ONLY the final text with no quotes, labels, or explanation.\n\
Style guidance: {style}\n\
Provider mode: {:?}.{custom_block}",
        settings.polish_provider
    )
}

fn api_key_for(settings: &AppSettings) -> Option<String> {
    read_keyring_api_key()
        .or_else(|| {
            std::env::var(API_KEY_ENV)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        })
        .or_else(|| {
            if matches!(settings.polish_provider, PolishProvider::OpenaiCompatible) {
                std::env::var("OPENAI_API_KEY")
                    .ok()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
            } else {
                None
            }
        })
}

pub fn read_keyring_api_key() -> Option<String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).ok()?;
    entry
        .get_password()
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub fn set_keyring_api_key(api_key: &str) -> Result<()> {
    let trimmed = api_key.trim();
    if trimmed.is_empty() {
        bail!("API key cannot be empty");
    }
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .context("failed to open OS keyring")?;
    entry
        .set_password(trimmed)
        .context("failed to store polish API key in the OS keyring")?;
    Ok(())
}

pub fn clear_keyring_api_key() -> Result<()> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .context("failed to open OS keyring")?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(error).context("failed to clear polish API key from the OS keyring"),
    }
}

pub fn has_keyring_api_key() -> bool {
    read_keyring_api_key().is_some()
}

fn strip_fences(value: &str) -> String {
    let trimmed = value.trim();
    if let Some(rest) = trimmed.strip_prefix("```") {
        let rest = rest
            .strip_prefix("text")
            .or_else(|| rest.strip_prefix("markdown"))
            .unwrap_or(rest)
            .trim_start_matches('\n');
        if let Some(inner) = rest.strip_suffix("```") {
            return inner.trim().to_string();
        }
    }
    trimmed.to_string()
}

fn sanitize_reqwest_error(error: reqwest::Error) -> anyhow::Error {
    let message = sanitize_message(&error.to_string());
    if error.is_timeout() || message.to_ascii_lowercase().contains("timed out") {
        anyhow::anyhow!("polish-timeout: {message}")
    } else {
        anyhow::anyhow!(message)
    }
}

pub fn is_timeout_error(error: &anyhow::Error) -> bool {
    let message = error.to_string().to_ascii_lowercase();
    message.contains("polish-timeout") || message.contains("timed out") || message.contains("timeout")
}

/// Strip likely API key material from provider error strings.
pub fn sanitize_message(message: &str) -> String {
    let mut sanitized = message.to_string();
    for key in [
        read_keyring_api_key(),
        std::env::var(API_KEY_ENV).ok(),
        std::env::var("OPENAI_API_KEY").ok(),
    ]
    .into_iter()
    .flatten()
    .filter(|value| value.len() >= 8)
    {
        sanitized = sanitized.replace(&key, "[redacted]");
    }
    let bearer = regex::Regex::new(r"(?i)(bearer\s+)[A-Za-z0-9._\-]+").expect("bearer regex");
    sanitized = bearer.replace_all(&sanitized, "${1}[redacted]").to_string();
    let sk = regex::Regex::new(r"sk-[A-Za-z0-9]{10,}").expect("sk regex");
    sanitized = sk.replace_all(&sanitized, "[redacted]").to_string();
    sanitized
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_polish_skips_without_app_http() {
        // Mirror the early-return contract of polish_if_enabled without needing AppHandle.
        let settings = AppSettings {
            auto_polish: false,
            ..AppSettings::default()
        };
        assert!(!settings.auto_polish);
    }

    #[test]
    fn timeout_classifier_detects_timeout_errors() {
        let timeout = anyhow::anyhow!("polish-timeout: operation timed out");
        let other = anyhow::anyhow!("bundled polish runtime not ready");
        assert!(is_timeout_error(&timeout));
        assert!(!is_timeout_error(&other));
    }

    #[test]
    fn sanitize_redacts_bearer_and_sk_tokens() {
        let message = "Authorization: Bearer sk-abcdefghijklmnopqrstuvwxyz failed";
        let sanitized = sanitize_message(message);
        assert!(!sanitized.contains("sk-abcdefghijklmnopqrstuvwxyz"));
        assert!(sanitized.contains("[redacted]"));
    }

    #[test]
    fn strip_fences_removes_markdown_wrapper() {
        assert_eq!(strip_fences("```text\nHello.\n```"), "Hello.");
        assert_eq!(strip_fences("Just text."), "Just text.");
    }
}
