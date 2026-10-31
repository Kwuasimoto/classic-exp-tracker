//! Translating with Google's Gemini API and the user's own key (credential.rs): the app has no
//! key or server of its own. Requests go straight from this PC to Google, through Gemini's
//! OpenAI-compatible Chat Completions endpoint, and carry only the text being translated.
//!
//! Gemini's models are often too busy to answer (503) or over the key's per-model quota (429).
//! Each request goes to the strongest model when it can, then down the list: Flash-Lite is
//! faster but unsafe for trades on its own (in Korean it turned "50k" into "50만", 500k, and
//! "WTS" into buying). A model that's busy is left alone for a while (30 s, doubling each time
//! it's busy again, up to 15 min) and the next model in `MODELS` is asked instead, so a busy
//! model isn't asked again and again.
//!
//! Prices matter in chat, so a translation is used only if it has every amount of the message
//! as written; otherwise the next model is asked.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use log::{info, warn};
use reqwest::header::CONTENT_TYPE;
use serde::{Deserialize, Serialize};

/// In the order they're asked, strongest first. Gemini 3.5 Flash is left out: it took over 20 s
/// to answer, too slow for chat. `gemini-flash-lite-latest` is too: it's an alias, which can
/// name the same model as another here.
pub const MODELS: [&str; 3] = ["gemini-3-flash-preview", "gemini-3.5-flash-lite", "gemini-3.1-flash-lite"];

/// What replies can be translated into: the app's id for each, and its English name for the prompt.
pub const LANGUAGES: [(&str, &str); 6] = [
    ("zh-hans", "Simplified Chinese"),
    ("ko", "Korean"),
    ("ja", "Japanese"),
    ("es", "Spanish"),
    ("pt-br", "Brazilian Portuguese"),
    ("th", "Thai"),
];

pub fn language(id: &str) -> Option<&'static str> {
    LANGUAGES.iter().find(|(i, _)| *i == id).map(|(_, name)| *name)
}

const ENDPOINT: &str = "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions";
/// A model that hasn't answered by then counts as busy.
const TIMEOUT: Duration = Duration::from_secs(15);
const FIRST_COOLDOWN: Duration = Duration::from_secs(30);
const MAX_COOLDOWN: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, thiserror::Error)]
pub enum GeminiError {
    #[error("Google didn't accept the key: {0}")]
    BadKey(String),
    #[error("Gemini is busy; try again in {0} s")]
    Busy(u64),
    #[error("couldn't reach Google: {0}")]
    Network(reqwest::Error),
    #[error("Google refused the request ({status}): {message}")]
    Refused { status: u16, message: String },
    #[error("{model} sent a reply that isn't a translation")]
    BadReply { model: &'static str },
    #[error("no model kept the message's amounts as written; try rewording it")]
    Amounts,
}

/// A translation, and the model that made it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Translation {
    pub text: String,
    pub model: &'static str,
}

/// When a model may be asked again.
#[derive(Clone, Copy, Debug, Default)]
struct Cooldown {
    until: Option<Instant>,
    /// How long the next busy reply leaves it alone; zero until it's been busy.
    next: Duration,
}

#[derive(Debug, Default)]
struct Rotation([Cooldown; MODELS.len()]);

impl Rotation {
    /// The first model, of those not `tried` yet, that isn't cooling down.
    fn pick(&self, now: Instant, tried: &[bool; MODELS.len()]) -> Option<usize> {
        (0..MODELS.len()).find(|&i| !tried[i] && self.0[i].until.is_none_or(|t| t <= now))
    }

    fn busy(&mut self, i: usize, now: Instant) -> Duration {
        let c = &mut self.0[i];
        let wait = if c.next.is_zero() { FIRST_COOLDOWN } else { c.next };
        c.until = Some(now + wait);
        c.next = (wait * 2).min(MAX_COOLDOWN);
        wait
    }

    /// The model isn't offered (retired, or not to this key): left alone as long as any.
    fn gone(&mut self, i: usize, now: Instant) {
        self.0[i] = Cooldown { until: Some(now + MAX_COOLDOWN), next: MAX_COOLDOWN };
    }

    fn answered(&mut self, i: usize) {
        self.0[i] = Cooldown::default();
    }

    /// Until the first model can be asked again.
    fn wait(&self, now: Instant) -> Duration {
        self.0.iter().filter_map(|c| c.until).map(|t| t.saturating_duration_since(now)).min().unwrap_or_default()
    }
}

#[derive(Serialize)]
struct Request<'a> {
    model: &'a str,
    messages: [Message<'a>; 2],
    temperature: f32,
    /// Gemini 3's least thinking: as good for chat, and several times faster.
    reasoning_effort: &'a str,
}

#[derive(Serialize)]
struct Message<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct Response {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: Reply,
}

#[derive(Deserialize)]
struct Reply {
    content: Option<String>,
}

/// Gemini's error body: `{"error": {...}}`, or that inside a one-item array.
#[derive(Deserialize)]
#[serde(untagged)]
enum ErrorBody {
    One(ErrorWrap),
    Many(Vec<ErrorWrap>),
}

#[derive(Deserialize)]
struct ErrorWrap {
    error: ErrorDetail,
}

#[derive(Deserialize)]
struct ErrorDetail {
    message: String,
}

fn error_message(body: &str) -> String {
    let detail = match serde_json::from_str::<ErrorBody>(body) {
        Ok(ErrorBody::One(w)) => Some(w.error),
        Ok(ErrorBody::Many(v)) => v.into_iter().next().map(|w| w.error),
        Err(_) => None,
    };
    detail.map_or_else(|| body.chars().take(200).collect(), |d| d.message)
}

/// What one model's answer means for the request.
enum Outcome {
    Translated(String),
    /// Busy or over quota: ask the next model.
    Busy,
    /// Not offered: ask the next model.
    Gone,
    Failed(GeminiError),
}

/// Async: commands run on Tauri's async runtime, where reqwest's blocking client can't.
pub struct Gemini {
    client: reqwest::Client,
    rotation: Mutex<Rotation>,
}

impl Gemini {
    pub fn new() -> Result<Self, reqwest::Error> {
        let client = reqwest::Client::builder()
            .user_agent(concat!("ClassicEXPTracker/", env!("CARGO_PKG_VERSION"), " (+https://github.com/Kwuasimoto/classic-exp-tracker)"))
            .timeout(TIMEOUT)
            .build()?;
        Ok(Self { client, rotation: Mutex::default() })
    }

    /// Translate the user's chat message `text` into `language` (its English name), to type
    /// into the game.
    pub async fn reply(&self, key: &str, text: &str, language: &str) -> Result<Translation, GeminiError> {
        let system = format!(
            "Translate this MapleStory chat message into {language}, short and natural like in-game chat. Keep names, \
             abbreviations like GG, and amounts like 50k or 1.2m exactly as written. Keep who is buying and who is selling. \
             Reply with only the translation."
        );
        let amounts = amounts(text);
        self.complete(key, &system, text, |t| keeps(t, &amounts)).await
    }

    /// Ask the models in turn, as `Rotation` allows, until one answers with a translation that
    /// passes `check`.
    async fn complete(&self, key: &str, system: &str, user: &str, check: impl Fn(&str) -> bool) -> Result<Translation, GeminiError> {
        let mut tried = [false; MODELS.len()];
        let mut unfaithful = false;
        loop {
            let now = Instant::now();
            let picked = {
                let rotation = self.rotation.lock().expect("gemini rotation lock poisoned");
                rotation.pick(now, &tried).ok_or_else(|| rotation.wait(now))
            };
            let i = match picked {
                Ok(i) => i,
                // Every model that could answer did, without keeping the amounts.
                Err(_) if unfaithful => return Err(GeminiError::Amounts),
                Err(wait) => return Err(GeminiError::Busy(wait.as_secs().max(1))),
            };
            tried[i] = true;
            let model = MODELS[i];
            let started = Instant::now();
            let outcome = self.ask(key, model, system, user).await;
            let mut rotation = self.rotation.lock().expect("gemini rotation lock poisoned");
            match outcome {
                Outcome::Translated(text) if !check(&text) => {
                    rotation.answered(i);
                    unfaithful = true;
                    warn!("gemini: {model} changed an amount, asking the next model");
                }
                Outcome::Translated(text) => {
                    rotation.answered(i);
                    info!("gemini: {model} answered in {} ms", started.elapsed().as_millis());
                    return Ok(Translation { text, model });
                }
                Outcome::Busy => {
                    let wait = rotation.busy(i, Instant::now());
                    warn!("gemini: {model} busy, left alone for {} s", wait.as_secs());
                }
                Outcome::Gone => {
                    rotation.gone(i, Instant::now());
                    warn!("gemini: {model} isn't offered to this key, left alone for {} min", MAX_COOLDOWN.as_secs() / 60);
                }
                Outcome::Failed(e) => return Err(e),
            }
        }
    }

    async fn ask(&self, key: &str, model: &'static str, system: &str, user: &str) -> Outcome {
        let request = Request {
            model,
            messages: [Message { role: "system", content: system }, Message { role: "user", content: user }],
            temperature: 0.0,
            reasoning_effort: "minimal",
        };
        let body = serde_json::to_vec(&request).expect("the request serializes");
        let response = match self.client.post(ENDPOINT).bearer_auth(key).header(CONTENT_TYPE, "application/json").body(body).send().await {
            Ok(r) => r,
            Err(e) if e.is_timeout() => return Outcome::Busy,
            Err(e) => return Outcome::Failed(GeminiError::Network(e)),
        };
        let status = response.status().as_u16();
        match response.text().await {
            Ok(body) => classify(status, &body, model),
            Err(e) if e.is_timeout() => Outcome::Busy,
            Err(e) => Outcome::Failed(GeminiError::Network(e)),
        }
    }
}

/// The amounts in `text` (50k, 1.2m, 60, 50,000), lowercased and without thousands commas.
fn amounts(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let mut amount = String::new();
        while i < chars.len() {
            let c = chars[i];
            let between_digits = matches!(c, '.' | ',') && chars.get(i + 1).is_some_and(char::is_ascii_digit) && !amount.is_empty();
            if c.is_ascii_digit() || between_digits {
                if c != ',' {
                    amount.push(c);
                }
                i += 1;
            } else {
                break;
            }
        }
        // A k/m/b right after it, ending the word, is part of it ("50k", not "4kg").
        if let Some(&c) = chars.get(i)
            && matches!(c.to_ascii_lowercase(), 'k' | 'm' | 'b')
            && !chars.get(i + 1).is_some_and(char::is_ascii_alphabetic)
        {
            amount.push(c.to_ascii_lowercase());
            i += 1;
        }
        out.push(amount);
    }
    out
}

/// `translation` has every one of `amounts`, as written.
fn keeps(translation: &str, amounts: &[String]) -> bool {
    let t = translation.to_lowercase().replace(',', "");
    amounts.iter().all(|a| t.contains(a.as_str()))
}

fn classify(status: u16, body: &str, model: &'static str) -> Outcome {
    match status {
        200 => {
            let text = serde_json::from_str::<Response>(body)
                .ok()
                .and_then(|r| r.choices.into_iter().next())
                .and_then(|c| c.message.content)
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty());
            text.map_or(Outcome::Failed(GeminiError::BadReply { model }), Outcome::Translated)
        }
        429 | 500 | 502 | 503 | 504 => Outcome::Busy,
        404 => Outcome::Gone,
        401 | 403 => Outcome::Failed(GeminiError::BadKey(error_message(body))),
        _ => {
            let message = error_message(body);
            // A wrong key is a 400, told apart only by its message ("Please pass a valid API key").
            if status == 400 && message.contains("API key") {
                Outcome::Failed(GeminiError::BadKey(message))
            } else {
                Outcome::Failed(GeminiError::Refused { status, message })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_busy_model_is_skipped_for_a_doubling_while_and_the_first_comes_first_again() {
        let start = Instant::now();
        let none = [false; MODELS.len()];
        let mut r = Rotation::default();
        assert_eq!(r.pick(start, &none), Some(0));
        assert_eq!(r.busy(0, start), FIRST_COOLDOWN);
        assert_eq!(r.pick(start, &none), Some(1), "the busy one is skipped");
        assert_eq!(r.pick(start, &[false, true, false]), Some(2), "and so is one already tried");
        assert_eq!(r.pick(start + FIRST_COOLDOWN, &none), Some(0), "asked again once its time is up");
        assert_eq!(r.busy(0, start), FIRST_COOLDOWN * 2, "busy again: twice as long");
        r.answered(0);
        assert_eq!(r.busy(0, start), FIRST_COOLDOWN, "an answer resets it");
        for _ in 0..10 {
            r.busy(1, start);
        }
        assert_eq!(r.busy(1, start), MAX_COOLDOWN);
        r.gone(2, start);
        assert_eq!(r.pick(start, &none), None);
        assert_eq!(r.wait(start), FIRST_COOLDOWN, "until the soonest");
    }

    #[test]
    fn a_translation_must_keep_the_amounts_as_written() {
        let a = amounts("WTS gfa60 for 150k, or 1.2M and 50,000 mesos for 4kg");
        assert_eq!(a, ["60", "150k", "1.2m", "50000", "4"]);
        assert!(keeps("장공 60% 150k에 팝니다, 1.2m 50,000 4", &a));
        assert!(!keeps("장공 60% 150만에 팝니다, 1.2m 50,000 4", &a), "50만 isn't 150k");
        assert!(keeps("GG", &amounts("GG, thanks!")), "nothing to keep");
    }

    #[test]
    fn replies_are_told_apart_by_status_and_message() {
        let ok = r#"{"choices":[{"message":{"role":"assistant","content":" 谢谢老哥 \n"}}]}"#;
        assert!(matches!(classify(200, ok, MODELS[0]), Outcome::Translated(t) if t == "谢谢老哥"));
        assert!(matches!(classify(200, r#"{"choices":[]}"#, MODELS[0]), Outcome::Failed(GeminiError::BadReply { .. })));
        let bad_key = r#"[{"error":{"code":400,"message":"Please pass a valid API key","status":"INVALID_ARGUMENT"}}]"#;
        assert!(matches!(classify(400, bad_key, MODELS[0]), Outcome::Failed(GeminiError::BadKey(m)) if m == "Please pass a valid API key"));
        let bad_arg = r#"{"error":{"code":400,"message":"Request contains an invalid argument.","status":"INVALID_ARGUMENT"}}"#;
        assert!(matches!(classify(400, bad_arg, MODELS[0]), Outcome::Failed(GeminiError::Refused { status: 400, .. })));
        assert!(matches!(classify(503, "", MODELS[0]), Outcome::Busy));
        assert!(matches!(classify(429, "", MODELS[0]), Outcome::Busy));
        assert!(matches!(classify(404, "", MODELS[0]), Outcome::Gone));
    }
}
