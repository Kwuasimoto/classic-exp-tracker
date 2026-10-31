//! The chat reader's files. Reading the chat needs a text finder and a reader for each script
//! players use (PaddleOCR PP-OCRv5, ONNX), which don't come with the installer: they're
//! downloaded together, only when the user clicks Download in Setup, from Hugging Face, each
//! file pinned to a repository revision and kept only once its SHA-256 matches the one here.
//! This module downloads and removes them; reading the chat comes later. Translating is done
//! by Gemini with the user's own key (gemini.rs).

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
    /// Where it comes from, for the log: the repository and its license.
    pub source: &'static str,
    pub license: &'static str,
}

/// Files installed together, in their own folder.
pub struct Part {
    pub id: &'static str,
    pub label: &'static str,
    pub files: &'static [Download],
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

pub const PARTS: [Part; 5] = [DETECTOR, READER_CJ, READER_KO, READER_LATIN, READER_TH];

/// The folder of v0.9.0's translator part (the Qwen2.5 1.5B model), deleted at start.
const OBSOLETE_TRANSLATOR: &str = "translator";

/// A stalled download fails after this long without data (reqwest's blocking timeout applies
/// to each read, not the whole download).
const READ_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, thiserror::Error)]
pub enum TranslatorError {
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
    #[error("download the chat reader in Setup first")]
    NotDownloaded,
}

/// A download in progress (or the last one, failed).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub done_bytes: u64,
    pub total_bytes: u64,
    /// Why the last download stopped, once it has; `None` while it runs.
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslatorStatus {
    /// The user turned it on (and the chat reader is downloaded). Off, nothing of it runs.
    pub enabled: bool,
    /// Every part is in place.
    pub downloaded: bool,
    /// What downloading would fetch now.
    pub download_bytes: u64,
    /// The download running, or the last one if it failed.
    pub job: Option<Job>,
    /// Where the files go.
    pub dir: String,
}

/// The chat reader's folder, the translator's on/off switch and its one download at a time.
pub struct Translator {
    dir: PathBuf,
    /// Reading the chat takes real processing power, so the translator runs only while the
    /// user has it on. Also in the database.
    enabled: AtomicBool,
    job: Mutex<Option<Job>>,
    cancel: AtomicBool,
}

impl Translator {
    /// `enabled` as saved; it stays off while the chat reader isn't downloaded.
    pub fn new(dir: PathBuf, enabled: bool) -> Self {
        // v0.9.0 could download a local translator model (1.1 GB); Gemini replaced it.
        let obsolete = dir.join(OBSOLETE_TRANSLATOR);
        if obsolete.exists() {
            match fs::remove_dir_all(&obsolete) {
                Ok(()) => info!("translator: deleted the old local translator model"),
                Err(e) => warn!("translator: couldn't delete the old local translator model at {}: {e}", obsolete.display()),
            }
        }
        let tr = Self { dir, enabled: AtomicBool::new(false), job: Mutex::new(None), cancel: AtomicBool::new(false) };
        tr.enabled.store(enabled && tr.downloaded(), Ordering::Release);
        tr
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    /// Turn it on (the chat reader must be downloaded) or off.
    pub fn set_enabled(&self, on: bool) -> Result<(), TranslatorError> {
        if on && !self.downloaded() {
            return Err(TranslatorError::NotDownloaded);
        }
        self.enabled.store(on, Ordering::Release);
        Ok(())
    }

    pub fn downloaded(&self) -> bool {
        self.missing().is_empty()
    }

    fn file_path(&self, part: &Part, file: &Download) -> PathBuf {
        self.dir.join(part.id).join(file.name)
    }

    /// A part is in place when each of its files is there at its size. A file is put in place
    /// only once its SHA-256 matched (`fetch`), so the size check catches one changed since.
    fn installed(&self, part: &Part) -> bool {
        part.files.iter().all(|f| fs::metadata(self.file_path(part, f)).is_ok_and(|m| m.is_file() && m.len() == f.size))
    }

    fn missing(&self) -> Vec<&'static Part> {
        PARTS.iter().filter(|p| !self.installed(p)).collect()
    }

    pub fn status(&self) -> TranslatorStatus {
        let missing = self.missing();
        TranslatorStatus {
            enabled: self.enabled(),
            downloaded: missing.is_empty(),
            download_bytes: missing.iter().flat_map(|p| p.files).map(|f| f.size).sum(),
            job: self.job.lock().expect("translator job lock poisoned").clone(),
            dir: self.dir.display().to_string(),
        }
    }

    /// Start downloading the parts that are missing (the user clicked Download), on their own
    /// thread; `status` reports the progress.
    pub fn download(self: &Arc<Self>) -> Result<(), TranslatorError> {
        let missing = self.missing();
        {
            let mut job = self.job.lock().expect("translator job lock poisoned");
            if job.as_ref().is_some_and(|j| j.error.is_none()) {
                return Err(TranslatorError::Busy);
            }
            let total_bytes = missing.iter().flat_map(|p| p.files).map(|f| f.size).sum();
            *job = Some(Job { done_bytes: 0, total_bytes, error: None });
        }
        self.cancel.store(false, Ordering::Release);
        let this = Arc::clone(self);
        std::thread::Builder::new().name("translator-download".into()).spawn(move || {
            let result = this.fetch_parts(&missing);
            let mut job = this.job.lock().expect("translator job lock poisoned");
            match result {
                Ok(()) => {
                    info!("translator: chat reader downloaded");
                    *job = None;
                }
                Err(e) => {
                    warn!("translator: downloading the chat reader stopped: {e}");
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

    /// Delete the chat reader's files.
    pub fn remove(&self) -> Result<(), TranslatorError> {
        if self.job.lock().expect("translator job lock poisoned").as_ref().is_some_and(|j| j.error.is_none()) {
            return Err(TranslatorError::Busy);
        }
        for part in &PARTS {
            let dir = self.dir.join(part.id);
            if dir.exists() {
                fs::remove_dir_all(&dir)?;
            }
        }
        info!("translator: chat reader removed");
        Ok(())
    }

    fn fetch_parts(&self, parts: &[&'static Part]) -> Result<(), TranslatorError> {
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
        info!("translator: downloading {} for the {} ({} bytes, {}, {}) from {}", file.name, part.label, file.size, file.source, file.license, file.url);
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
    fn a_file_is_kept_only_at_its_size_and_sha256_and_every_part_is_needed() {
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

        // One part missing: not downloaded, and only that part is left to fetch.
        let dir = std::env::temp_dir().join(format!("classic-exp-translator-test-{}", std::process::id()));
        let tr = Translator::new(dir.clone(), true);
        assert!(!tr.enabled(), "saved on, but nothing downloaded");
        for p in PARTS.iter().filter(|p| p.id != READER_TH.id) {
            fs::create_dir_all(dir.join(p.id)).unwrap();
            for f in p.files {
                File::create(tr.file_path(p, f)).unwrap().set_len(f.size).unwrap();
            }
        }
        assert!(!tr.status().downloaded);
        assert_eq!(tr.status().download_bytes, READER_TH.files.iter().map(|f| f.size).sum::<u64>());
        assert!(matches!(tr.set_enabled(true), Err(TranslatorError::NotDownloaded)));
        fs::create_dir_all(dir.join(READER_TH.id)).unwrap();
        for f in READER_TH.files {
            File::create(tr.file_path(&READER_TH, f)).unwrap().set_len(f.size).unwrap();
        }
        assert!(tr.status().downloaded);
        assert!(tr.set_enabled(true).is_ok());
        tr.remove().unwrap();
        assert!(!tr.status().downloaded);
        assert_eq!(tr.status().download_bytes, PARTS.iter().flat_map(|p| p.files).map(|f| f.size).sum::<u64>());
        fs::remove_dir_all(&dir).unwrap();
    }
}
