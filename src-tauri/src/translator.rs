//! The chat translator's setup. None of it comes with the installer: each language needs a
//! text finder and a reader for its script (PaddleOCR PP-OCRv5, ONNX) and the translator (the
//! Qwen2.5 1.5B Instruct language model, GGUF). They're downloaded only when the user installs a
//! language, from Hugging Face, each file pinned to a repository revision and kept only once
//! its SHA-256 matches the one here. Parts shared by languages are downloaded once. This module
//! installs and removes them; reading and translating the chat come later.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use log::{info, warn};
use serde::Serialize;
use sha2::{Digest, Sha256};

/// A file to download, pinned: `url` names a repository revision, and the file is kept only
/// if it's `size` bytes with this SHA-256.
pub struct Download {
    pub name: &'static str,
    pub url: &'static str,
    pub size: u64,
    pub sha256: &'static str,
    /// Where it comes from, for the page: the repository and its license.
    pub source: &'static str,
    pub license: &'static str,
}

/// Files installed together, in their own folder.
pub struct Part {
    pub id: &'static str,
    pub label: &'static str,
    pub files: &'static [Download],
}

pub struct Language {
    pub id: &'static str,
    pub label: &'static str,
    /// The ids of the parts it needs.
    pub parts: &'static [&'static str],
}

const DETECTOR: Part = Part {
    id: "ocr-detector",
    label: "Text finder (all languages)",
    files: &[
        Download {
            name: "inference.onnx",
            url: "https://huggingface.co/PaddlePaddle/PP-OCRv5_mobile_det_onnx/resolve/e6f4fa85f00e168c862bc462aebca69eef9b3d3d/inference.onnx",
            size: 4_826_518,
            sha256: "a431985659dc921974177a95adcfbb90fd9e51989a5e04d70d0b75f597b6e61d",
            source: "PaddlePaddle/PP-OCRv5_mobile_det_onnx",
            license: "Apache-2.0",
        },
        Download {
            name: "inference.yml",
            url: "https://huggingface.co/PaddlePaddle/PP-OCRv5_mobile_det_onnx/resolve/e6f4fa85f00e168c862bc462aebca69eef9b3d3d/inference.yml",
            size: 903,
            sha256: "98069072e1b6b37d727fd9d9f11725faa46d6ea0de012f2ed26caea011c37699",
            source: "PaddlePaddle/PP-OCRv5_mobile_det_onnx",
            license: "Apache-2.0",
        },
    ],
};

/// PP-OCRv5's main reader: Simplified and Traditional Chinese, Japanese and English.
const READER_CJ: Part = Part {
    id: "ocr-reader-cj",
    label: "Reader for Chinese and Japanese",
    files: &[
        Download {
            name: "inference.onnx",
            url: "https://huggingface.co/PaddlePaddle/PP-OCRv5_mobile_rec_onnx/resolve/ed152b8b495f84de93cda5709d768548a9127622/inference.onnx",
            size: 16_534_782,
            sha256: "da72dc72ca4dc220df0dfde68c1dedc31c58d3e76a25871122e5056227d50092",
            source: "PaddlePaddle/PP-OCRv5_mobile_rec_onnx",
            license: "Apache-2.0",
        },
        Download {
            name: "inference.yml",
            url: "https://huggingface.co/PaddlePaddle/PP-OCRv5_mobile_rec_onnx/resolve/ed152b8b495f84de93cda5709d768548a9127622/inference.yml",
            size: 148_345,
            sha256: "5dfeb2777f6d0db8177d8128a8acfcf6e6276dc4ac73ea3bf0dc06d6a5e85d8e",
            source: "PaddlePaddle/PP-OCRv5_mobile_rec_onnx",
            license: "Apache-2.0",
        },
    ],
};

const READER_KO: Part = Part {
    id: "ocr-reader-ko",
    label: "Reader for Korean",
    files: &[
        Download {
            name: "inference.onnx",
            url: "https://huggingface.co/PaddlePaddle/korean_PP-OCRv5_mobile_rec_onnx/resolve/5c6f574b8e2230adf4287b33e736d71b9fabd28e/inference.onnx",
            size: 13_418_787,
            sha256: "92f0b7785e64fc9090106a241cf4c1eb97472824558272751b88a2a4476d3a08",
            source: "PaddlePaddle/korean_PP-OCRv5_mobile_rec_onnx",
            license: "Apache-2.0",
        },
        Download {
            name: "inference.yml",
            url: "https://huggingface.co/PaddlePaddle/korean_PP-OCRv5_mobile_rec_onnx/resolve/5c6f574b8e2230adf4287b33e736d71b9fabd28e/inference.yml",
            size: 96_039,
            sha256: "f757fa1c40e99edcf27e9cce879b93eb2a51fa46f5ef39095689b8c37dd75998",
            source: "PaddlePaddle/korean_PP-OCRv5_mobile_rec_onnx",
            license: "Apache-2.0",
        },
    ],
};

/// PP-OCRv5's Latin reader: Spanish and Portuguese (with ñ, ç, ã and the accents), among others.
/// It lacks most of Vietnamese's letters (ạ, ả, ấ...), so Vietnamese isn't offered with it.
const READER_LATIN: Part = Part {
    id: "ocr-reader-latin",
    label: "Reader for Spanish and Portuguese",
    files: &[
        Download {
            name: "inference.onnx",
            url: "https://huggingface.co/PaddlePaddle/latin_PP-OCRv5_mobile_rec_onnx/resolve/89d3a50e2c27e2e7cceeab0e944c25c807d5db4f/inference.onnx",
            size: 8_042_023,
            sha256: "7888113072263cb471b93f66dd5e2ad70548dc526fa1ace760d0d973dd121498",
            source: "PaddlePaddle/latin_PP-OCRv5_mobile_rec_onnx",
            license: "Apache-2.0",
        },
        Download {
            name: "inference.yml",
            url: "https://huggingface.co/PaddlePaddle/latin_PP-OCRv5_mobile_rec_onnx/resolve/89d3a50e2c27e2e7cceeab0e944c25c807d5db4f/inference.yml",
            size: 6_817,
            sha256: "0bbe984570f597af3638e50bdf2e8276f3ab26a61966096538b3b0d1849f5c84",
            source: "PaddlePaddle/latin_PP-OCRv5_mobile_rec_onnx",
            license: "Apache-2.0",
        },
    ],
};

const READER_TH: Part = Part {
    id: "ocr-reader-th",
    label: "Reader for Thai",
    files: &[
        Download {
            name: "inference.onnx",
            url: "https://huggingface.co/PaddlePaddle/th_PP-OCRv5_mobile_rec_onnx/resolve/1d4adbbafb1034a2fd6618498575b81ea7b69f69/inference.onnx",
            size: 7_891_015,
            sha256: "27618be66018f8598ac0a526a593f9f1cebf794e7eded93428e8fb016e537f5f",
            source: "PaddlePaddle/th_PP-OCRv5_mobile_rec_onnx",
            license: "Apache-2.0",
        },
        Download {
            name: "inference.yml",
            url: "https://huggingface.co/PaddlePaddle/th_PP-OCRv5_mobile_rec_onnx/resolve/1d4adbbafb1034a2fd6618498575b81ea7b69f69/inference.yml",
            size: 4_667,
            sha256: "f6ba7fefc38ca1ff398ddafa75d67d16e0b3757c4e6c833adffee98a981766c9",
            source: "PaddlePaddle/th_PP-OCRv5_mobile_rec_onnx",
            license: "Apache-2.0",
        },
    ],
};

const TRANSLATOR: Part = Part {
    id: "translator",
    label: "Translator (all languages)",
    files: &[Download {
        name: "qwen2.5-1.5b-instruct-q4_k_m.gguf",
        url: "https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct-GGUF/resolve/91cad51170dc346986eccefdc2dd33a9da36ead9/qwen2.5-1.5b-instruct-q4_k_m.gguf",
        size: 1_117_320_736,
        sha256: "6a1a2eb6d15622bf3c96857206351ba97e1af16c30d7a74ee38970e434e9407e",
        source: "Qwen/Qwen2.5-1.5B-Instruct-GGUF",
        license: "Apache-2.0",
    }],
};

pub const PARTS: [Part; 6] = [DETECTOR, READER_CJ, READER_KO, READER_LATIN, READER_TH, TRANSLATOR];

pub const LANGUAGES: [Language; 6] = [
    Language { id: "zh-hans", label: "Chinese (Simplified)", parts: &[DETECTOR.id, READER_CJ.id, TRANSLATOR.id] },
    Language { id: "ko", label: "Korean", parts: &[DETECTOR.id, READER_KO.id, TRANSLATOR.id] },
    Language { id: "ja", label: "Japanese", parts: &[DETECTOR.id, READER_CJ.id, TRANSLATOR.id] },
    Language { id: "es", label: "Spanish", parts: &[DETECTOR.id, READER_LATIN.id, TRANSLATOR.id] },
    Language { id: "pt-br", label: "Portuguese (Brazil)", parts: &[DETECTOR.id, READER_LATIN.id, TRANSLATOR.id] },
    Language { id: "th", label: "Thai", parts: &[DETECTOR.id, READER_TH.id, TRANSLATOR.id] },
];

/// A stalled download fails after this long without data (reqwest's blocking timeout applies
/// to each read, not the whole download).
const READ_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, thiserror::Error)]
pub enum TranslatorError {
    #[error("no language {0:?}")]
    UnknownLanguage(String),
    #[error("a download is already running")]
    Busy,
    #[error("download failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("couldn't write the download: {0}")]
    Io(#[from] std::io::Error),
    #[error("{name} was {got} bytes, expected {expected}; it was deleted")]
    Size { name: String, got: u64, expected: u64 },
    #[error("{name} didn't match its SHA-256 (got {got}); it was deleted")]
    Checksum { name: String, got: String },
    #[error("cancelled")]
    Cancelled,
    #[error("install a language first")]
    NothingInstalled,
}

fn part(id: &str) -> &'static Part {
    PARTS.iter().find(|p| p.id == id).expect("languages name only parts in PARTS")
}

/// Every part `a` needs, `b` needs too.
fn within(a: &Language, b: &Language) -> bool {
    a.parts.iter().all(|p| b.parts.contains(p))
}

fn language(id: &str) -> Result<&'static Language, TranslatorError> {
    LANGUAGES.iter().find(|l| l.id == id).ok_or_else(|| TranslatorError::UnknownLanguage(id.to_string()))
}

/// A download in progress (or the last one, failed).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub language: String,
    pub done_bytes: u64,
    pub total_bytes: u64,
    /// Why the last download stopped, once it has; `None` while it runs.
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageStatus {
    pub id: &'static str,
    pub label: &'static str,
    pub installed: bool,
    /// What installing it would download now (parts another language installed are counted once).
    pub download_bytes: u64,
    /// The ids of other languages made of the same files: installed and removed with it.
    pub shares_with: Vec<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    /// The part's id (the UI names it in its language) and English label.
    pub part_id: &'static str,
    pub part: &'static str,
    pub name: &'static str,
    pub url: &'static str,
    pub size: u64,
    pub sha256: &'static str,
    pub source: &'static str,
    pub license: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslatorStatus {
    /// The user turned it on (and a language is installed). Off, nothing of it runs.
    pub enabled: bool,
    pub languages: Vec<LanguageStatus>,
    /// The download running, or the last one if it failed.
    pub job: Option<Job>,
    /// Where the files go.
    pub dir: String,
    /// Every file a language can need, as pinned here.
    pub files: Vec<FileInfo>,
}

/// The translator's folder, its on/off switch and its one download at a time.
pub struct Translator {
    dir: PathBuf,
    /// Reading and translating the chat take real processing power and memory, so the
    /// translator runs only while the user has it on. Also in the database.
    enabled: AtomicBool,
    job: Mutex<Option<Job>>,
    cancel: AtomicBool,
}

impl Translator {
    /// `enabled` as saved; it stays off while no language is installed.
    pub fn new(dir: PathBuf, enabled: bool) -> Self {
        let tr = Self { dir, enabled: AtomicBool::new(false), job: Mutex::new(None), cancel: AtomicBool::new(false) };
        tr.enabled.store(enabled && tr.any_installed(), Ordering::Release);
        tr
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    /// Turn it on (a language must be installed) or off.
    pub fn set_enabled(&self, on: bool) -> Result<(), TranslatorError> {
        if on && !self.any_installed() {
            return Err(TranslatorError::NothingInstalled);
        }
        self.enabled.store(on, Ordering::Release);
        Ok(())
    }

    pub fn any_installed(&self) -> bool {
        LANGUAGES.iter().any(|l| self.missing(l).is_empty())
    }

    fn file_path(&self, part: &Part, file: &Download) -> PathBuf {
        self.dir.join(part.id).join(file.name)
    }

    /// A part is installed when each of its files is there at its size. A file is put in place
    /// only once its SHA-256 matched (`fetch`), so the size check catches one changed since.
    fn installed(&self, part: &Part) -> bool {
        part.files.iter().all(|f| fs::metadata(self.file_path(part, f)).is_ok_and(|m| m.is_file() && m.len() == f.size))
    }

    fn missing(&self, language: &Language) -> Vec<&'static Part> {
        language.parts.iter().map(|id| part(id)).filter(|p| !self.installed(p)).collect()
    }

    pub fn status(&self) -> TranslatorStatus {
        let languages = LANGUAGES
            .iter()
            .map(|l| {
                let missing = self.missing(l);
                LanguageStatus {
                    id: l.id,
                    label: l.label,
                    installed: missing.is_empty(),
                    download_bytes: missing.iter().flat_map(|p| p.files).map(|f| f.size).sum(),
                    shares_with: LANGUAGES.iter().filter(|o| o.id != l.id && within(o, l)).map(|o| o.id).collect(),
                }
            })
            .collect();
        let files = PARTS
            .iter()
            .flat_map(|p| {
                p.files.iter().map(|f| FileInfo { part_id: p.id, part: p.label, name: f.name, url: f.url, size: f.size, sha256: f.sha256, source: f.source, license: f.license })
            })
            .collect();
        TranslatorStatus {
            enabled: self.enabled(),
            languages,
            job: self.job.lock().expect("translator job lock poisoned").clone(),
            dir: self.dir.display().to_string(),
            files,
        }
    }

    /// Start installing `language` on its own thread; `status` reports its progress.
    pub fn install(self: &Arc<Self>, language_id: &str) -> Result<(), TranslatorError> {
        let language = language(language_id)?;
        let missing = self.missing(language);
        {
            let mut job = self.job.lock().expect("translator job lock poisoned");
            if job.as_ref().is_some_and(|j| j.error.is_none()) {
                return Err(TranslatorError::Busy);
            }
            let total_bytes = missing.iter().flat_map(|p| p.files).map(|f| f.size).sum();
            *job = Some(Job { language: language.id.to_string(), done_bytes: 0, total_bytes, error: None });
        }
        self.cancel.store(false, Ordering::Release);
        let this = Arc::clone(self);
        std::thread::Builder::new().name("translator-download".into()).spawn(move || {
            let result = this.download(&missing);
            let mut job = this.job.lock().expect("translator job lock poisoned");
            match result {
                Ok(()) => {
                    info!("translator: {} installed", language.label);
                    *job = None;
                }
                Err(e) => {
                    warn!("translator: installing {} stopped: {e}", language.label);
                    if let Some(job) = job.as_mut() {
                        job.error = Some(e.to_string());
                    }
                }
            }
        })?;
        Ok(())
    }

    /// Stop the running download; its partial file is deleted.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }

    /// Forget a failed download's message.
    pub fn dismiss(&self) {
        let mut job = self.job.lock().expect("translator job lock poisoned");
        if job.as_ref().is_some_and(|j| j.error.is_some()) {
            *job = None;
        }
    }

    /// Delete `language`'s parts that another installed language still needs, other than one
    /// made of no more than its files (Chinese and Japanese share theirs): that one goes too.
    pub fn remove(&self, language_id: &str) -> Result<(), TranslatorError> {
        let language = language(language_id)?;
        if self.job.lock().expect("translator job lock poisoned").as_ref().is_some_and(|j| j.error.is_none()) {
            return Err(TranslatorError::Busy);
        }
        let kept: Vec<&str> = LANGUAGES
            .iter()
            .filter(|l| !within(l, language) && self.missing(l).is_empty())
            .flat_map(|l| l.parts.iter().copied())
            .collect();
        for id in language.parts.iter().filter(|id| !kept.contains(id)) {
            let dir = self.dir.join(id);
            if dir.exists() {
                fs::remove_dir_all(&dir)?;
            }
        }
        info!("translator: {} removed", language.label);
        Ok(())
    }

    fn download(&self, parts: &[&'static Part]) -> Result<(), TranslatorError> {
        let client = reqwest::blocking::Client::builder()
            .user_agent(concat!("ClassicEXPTracker/", env!("CARGO_PKG_VERSION"), " (+https://github.com/Kwuasimoto/classic-exp-tracker)"))
            .timeout(READ_TIMEOUT)
            .build()?;
        for part in parts {
            fs::create_dir_all(self.dir.join(part.id))?;
            for file in part.files {
                if fs::metadata(self.file_path(part, file)).is_ok_and(|m| m.len() == file.size) {
                    self.progress(file.size);
                    continue;
                }
                self.fetch(&client, part, file)?;
            }
        }
        Ok(())
    }

    /// Download `file` to a `.part` file, then put it in place only if its size and SHA-256
    /// match; anything else deletes it.
    fn fetch(&self, client: &reqwest::blocking::Client, part: &Part, file: &Download) -> Result<(), TranslatorError> {
        let target = self.file_path(part, file);
        let partial = target.with_extension(format!("{}.part", target.extension().and_then(|e| e.to_str()).unwrap_or("")));
        info!("translator: downloading {} ({} bytes) from {}", file.name, file.size, file.url);
        let result = self.fetch_to(client, file, &partial);
        match result {
            Ok(()) => Ok(fs::rename(&partial, &target)?),
            Err(e) => {
                if partial.exists()
                    && let Err(rm) = fs::remove_file(&partial)
                {
                    warn!("translator: couldn't delete {}: {rm}", partial.display());
                }
                Err(e)
            }
        }
    }

    fn fetch_to(&self, client: &reqwest::blocking::Client, file: &Download, partial: &Path) -> Result<(), TranslatorError> {
        let mut response = client.get(file.url).send()?.error_for_status()?;
        let mut out = File::create(partial)?;
        let mut check = Check::default();
        let mut buf = vec![0u8; 256 * 1024];
        loop {
            if self.cancel.load(Ordering::Acquire) {
                return Err(TranslatorError::Cancelled);
            }
            let n = response.read(&mut buf)?;
            if n == 0 {
                break;
            }
            out.write_all(&buf[..n])?;
            check.update(&buf[..n]);
            self.progress(n as u64);
            if check.len > file.size {
                break;
            }
        }
        out.sync_all()?;
        check.finish(file)
    }

    fn progress(&self, bytes: u64) {
        if let Some(job) = self.job.lock().expect("translator job lock poisoned").as_mut() {
            job.done_bytes += bytes;
        }
    }
}

/// A download's size and SHA-256 as it arrives.
#[derive(Default)]
struct Check {
    hasher: Sha256,
    len: u64,
}

impl Check {
    fn update(&mut self, bytes: &[u8]) {
        self.hasher.update(bytes);
        self.len += bytes.len() as u64;
    }

    fn finish(self, file: &Download) -> Result<(), TranslatorError> {
        if self.len != file.size {
            return Err(TranslatorError::Size { name: file.name.to_string(), got: self.len, expected: file.size });
        }
        let got: String = self.hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if got != file.sha256 {
            return Err(TranslatorError::Checksum { name: file.name.to_string(), got });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_kept_only_at_its_size_and_sha256_and_shared_parts_outlive_a_removal() {
        // SHA-256 of "abc" (FIPS 180-2 example).
        let file = Download { name: "x", url: "", size: 3, sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad", source: "", license: "" };
        let check = |bytes: &[u8]| {
            let mut c = Check::default();
            c.update(bytes);
            c.finish(&file)
        };
        assert!(check(b"abc").is_ok());
        assert!(matches!(check(b"abd"), Err(TranslatorError::Checksum { .. })));
        assert!(matches!(check(b"abcd"), Err(TranslatorError::Size { .. })));

        // Every part in place at its size: all three languages installed.
        let dir = std::env::temp_dir().join(format!("classic-exp-translator-test-{}", std::process::id()));
        let tr = Translator::new(dir.clone(), false);
        for p in &PARTS {
            fs::create_dir_all(dir.join(p.id)).unwrap();
            for f in p.files {
                File::create(tr.file_path(p, f)).unwrap().set_len(f.size).unwrap();
            }
        }
        let installed = |tr: &Translator| tr.status().languages.iter().map(|l| (l.id, l.installed)).collect::<Vec<_>>();
        assert_eq!(installed(&tr), [("zh-hans", true), ("ko", true), ("ja", true), ("es", true), ("pt-br", true), ("th", true)]);
        assert_eq!(tr.status().languages[2].shares_with, ["zh-hans"]);
        assert_eq!(tr.status().languages[3].shares_with, ["pt-br"]);
        // Japanese and Chinese share their files, so they go together; Korean keeps the shared
        // text finder and translator.
        tr.remove("ja").unwrap();
        assert_eq!(installed(&tr), [("zh-hans", false), ("ko", true), ("ja", false), ("es", true), ("pt-br", true), ("th", true)]);
        assert_eq!(tr.status().languages[0].download_bytes, READER_CJ.files.iter().map(|f| f.size).sum::<u64>(), "only the reader is missing");
        for id in ["ko", "es", "th"] {
            tr.remove(id).unwrap();
        }
        assert!(installed(&tr).iter().all(|(_, on)| !on), "{:?}", installed(&tr));
        fs::remove_dir_all(&dir).unwrap();
    }
}
