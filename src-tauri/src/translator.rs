//! The chat reader's files: a reader for each script players use (PaddleOCR PP-OCRv5, ONNX
//! models from Hugging Face) and the engine that runs them (Microsoft's ONNX Runtime). None
//! come with the installer: they're downloaded together, only when the user clicks Download in
//! Setup, each file pinned to a revision and kept only once its SHA-256 matches the one here.
//! This module downloads and removes them; read::chat_ocr runs them. Translating is done by
//! Gemini with the user's own key (gemini.rs).

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
    /// A file inside the download (a zip) that is kept instead of it, checked on its own.
    pub extract: Option<Extract>,
}

/// The one file taken out of a zip download; the zip itself is deleted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Extract {
    /// Its path inside the zip.
    pub path: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

impl Download {
    /// What ends up on disk: the extracted file's size, or the download's.
    fn kept_size(&self) -> u64 {
        self.extract.map_or(self.size, |e| e.size)
    }
}

/// Files installed together, in their own folder.
pub struct Part {
    pub id: &'static str,
    pub label: &'static str,
    /// The project's own page and where the files come from, linked in Setup so the user can
    /// look both up (open_page opens only these).
    pub homepage: &'static str,
    pub source_page: &'static str,
    pub files: &'static [Download],
}

/// ONNX Runtime, which runs the readers. Microsoft publishes the same signed DLL in its GitHub
/// release (an 83 MB zip of developer files) and in its 15 MB Python package; this takes it
/// from the package, the smaller download.
pub const ENGINE: Part = Part {
    id: "ocr-engine",
    label: "OCR engine (ONNX Runtime by Microsoft)",
    homepage: "https://onnxruntime.ai",
    source_page: "https://pypi.org/project/onnxruntime/1.31.1/",
    files: &[Download {
        name: "onnxruntime.dll",
        url: "https://files.pythonhosted.org/packages/59/bf/a1d282b4c753ba3e9b31f4c11abd2bc2390513dc5e3792ca3b78e42401c3/onnxruntime-1.31.1-cp313-cp313-win_amd64.whl",
        size: 14_888_304,
        sha256: "0558f29d5f6ac09957390c9c5495931485ee8ae7f0ce781ccee86a0ae5c2348b",
        source: "pypi.org/project/onnxruntime 1.31.1 (Microsoft)",
        license: "MIT",
        extract: Some(Extract {
            path: "onnxruntime/capi/onnxruntime.dll",
            size: 19_288_888,
            sha256: "68d0d5f502be4509e2a16f932627a95137d748b2fae519027f80125c0b1f550b",
        }),
    }],
};

/// PP-OCRv5's main reader: Simplified and Traditional Chinese, Japanese and English.
const READER_CJ: Part = Part {
    id: "ocr-reader-cj",
    label: "Reader for Chinese and Japanese",
    homepage: "https://www.paddleocr.ai",
    source_page: "https://huggingface.co/PaddlePaddle/PP-OCRv5_mobile_rec_onnx",
    files: &[
        Download {
            name: "inference.onnx",
            url: "https://huggingface.co/PaddlePaddle/PP-OCRv5_mobile_rec_onnx/resolve/ed152b8b495f84de93cda5709d768548a9127622/inference.onnx",
            size: 16_534_782,
            sha256: "da72dc72ca4dc220df0dfde68c1dedc31c58d3e76a25871122e5056227d50092",
            source: "PaddlePaddle/PP-OCRv5_mobile_rec_onnx",
            license: "Apache-2.0",
            extract: None,
        },
        Download {
            name: "inference.yml",
            url: "https://huggingface.co/PaddlePaddle/PP-OCRv5_mobile_rec_onnx/resolve/ed152b8b495f84de93cda5709d768548a9127622/inference.yml",
            size: 148_345,
            sha256: "5dfeb2777f6d0db8177d8128a8acfcf6e6276dc4ac73ea3bf0dc06d6a5e85d8e",
            source: "PaddlePaddle/PP-OCRv5_mobile_rec_onnx",
            license: "Apache-2.0",
            extract: None,
        },
    ],
};

const READER_KO: Part = Part {
    id: "ocr-reader-ko",
    label: "Reader for Korean",
    homepage: "https://www.paddleocr.ai",
    source_page: "https://huggingface.co/PaddlePaddle/korean_PP-OCRv5_mobile_rec_onnx",
    files: &[
        Download {
            name: "inference.onnx",
            url: "https://huggingface.co/PaddlePaddle/korean_PP-OCRv5_mobile_rec_onnx/resolve/5c6f574b8e2230adf4287b33e736d71b9fabd28e/inference.onnx",
            size: 13_418_787,
            sha256: "92f0b7785e64fc9090106a241cf4c1eb97472824558272751b88a2a4476d3a08",
            source: "PaddlePaddle/korean_PP-OCRv5_mobile_rec_onnx",
            license: "Apache-2.0",
            extract: None,
        },
        Download {
            name: "inference.yml",
            url: "https://huggingface.co/PaddlePaddle/korean_PP-OCRv5_mobile_rec_onnx/resolve/5c6f574b8e2230adf4287b33e736d71b9fabd28e/inference.yml",
            size: 96_039,
            sha256: "f757fa1c40e99edcf27e9cce879b93eb2a51fa46f5ef39095689b8c37dd75998",
            source: "PaddlePaddle/korean_PP-OCRv5_mobile_rec_onnx",
            license: "Apache-2.0",
            extract: None,
        },
    ],
};

/// PP-OCRv5's Latin reader: Spanish and Portuguese (with ñ, ç, ã and the accents), among others.
/// It lacks most of Vietnamese's letters (ạ, ả, ấ...), so Vietnamese isn't offered with it.
pub const READER_LATIN: Part = Part {
    id: "ocr-reader-latin",
    label: "Reader for Spanish and Portuguese",
    homepage: "https://www.paddleocr.ai",
    source_page: "https://huggingface.co/PaddlePaddle/latin_PP-OCRv5_mobile_rec_onnx",
    files: &[
        Download {
            name: "inference.onnx",
            url: "https://huggingface.co/PaddlePaddle/latin_PP-OCRv5_mobile_rec_onnx/resolve/89d3a50e2c27e2e7cceeab0e944c25c807d5db4f/inference.onnx",
            size: 8_042_023,
            sha256: "7888113072263cb471b93f66dd5e2ad70548dc526fa1ace760d0d973dd121498",
            source: "PaddlePaddle/latin_PP-OCRv5_mobile_rec_onnx",
            license: "Apache-2.0",
            extract: None,
        },
        Download {
            name: "inference.yml",
            url: "https://huggingface.co/PaddlePaddle/latin_PP-OCRv5_mobile_rec_onnx/resolve/89d3a50e2c27e2e7cceeab0e944c25c807d5db4f/inference.yml",
            size: 6_817,
            sha256: "0bbe984570f597af3638e50bdf2e8276f3ab26a61966096538b3b0d1849f5c84",
            source: "PaddlePaddle/latin_PP-OCRv5_mobile_rec_onnx",
            license: "Apache-2.0",
            extract: None,
        },
    ],
};

const READER_TH: Part = Part {
    id: "ocr-reader-th",
    label: "Reader for Thai",
    homepage: "https://www.paddleocr.ai",
    source_page: "https://huggingface.co/PaddlePaddle/th_PP-OCRv5_mobile_rec_onnx",
    files: &[
        Download {
            name: "inference.onnx",
            url: "https://huggingface.co/PaddlePaddle/th_PP-OCRv5_mobile_rec_onnx/resolve/1d4adbbafb1034a2fd6618498575b81ea7b69f69/inference.onnx",
            size: 7_891_015,
            sha256: "27618be66018f8598ac0a526a593f9f1cebf794e7eded93428e8fb016e537f5f",
            source: "PaddlePaddle/th_PP-OCRv5_mobile_rec_onnx",
            license: "Apache-2.0",
            extract: None,
        },
        Download {
            name: "inference.yml",
            url: "https://huggingface.co/PaddlePaddle/th_PP-OCRv5_mobile_rec_onnx/resolve/1d4adbbafb1034a2fd6618498575b81ea7b69f69/inference.yml",
            size: 4_667,
            sha256: "f6ba7fefc38ca1ff398ddafa75d67d16e0b3757c4e6c833adffee98a981766c9",
            source: "PaddlePaddle/th_PP-OCRv5_mobile_rec_onnx",
            license: "Apache-2.0",
            extract: None,
        },
    ],
};

pub const PARTS: [Part; 5] = [ENGINE, READER_CJ, READER_KO, READER_LATIN, READER_TH];

/// Folders of parts no longer used, deleted at start: v0.9.0's translator (the Qwen2.5 1.5B
/// model), and the text finder until v0.11 (read::chat finds the lines itself).
const OBSOLETE: [(&str, &str); 2] = [("translator", "the old local translator model"), ("ocr-detector", "the unused text finder")];

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
    #[error("couldn't open the download: {0}")]
    Zip(#[from] zip::result::ZipError),
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
    /// Each part and its pinned files, for Setup to show the user what's downloaded and from where.
    pub parts: Vec<PartStatus>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PartStatus {
    pub id: &'static str,
    pub homepage: &'static str,
    pub source_page: &'static str,
    pub installed: bool,
    pub files: Vec<FileStatus>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileStatus {
    pub name: &'static str,
    pub url: &'static str,
    pub size: u64,
    pub sha256: &'static str,
    pub license: &'static str,
    pub extract: Option<Extract>,
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
        for (folder, what) in OBSOLETE {
            let obsolete = dir.join(folder);
            if obsolete.exists() {
                match fs::remove_dir_all(&obsolete) {
                    Ok(()) => info!("translator: deleted {what}"),
                    Err(e) => warn!("translator: couldn't delete {what} at {}: {e}", obsolete.display()),
                }
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

    /// A part's page (`source` false) or its files' page, if `id` names a part: the only
    /// addresses Setup can open.
    pub fn page(id: &str, source: bool) -> Option<&'static str> {
        PARTS.iter().find(|p| p.id == id).map(|p| if source { p.source_page } else { p.homepage })
    }

    /// Where a part's files are, once downloaded.
    pub fn part_dir(&self, part: &Part) -> PathBuf {
        self.dir.join(part.id)
    }

    fn file_path(&self, part: &Part, file: &Download) -> PathBuf {
        self.dir.join(part.id).join(file.name)
    }

    /// A part is in place when each of its files is there at its size. A file is put in place
    /// only once its SHA-256 matched (`fetch`), so the size check catches one changed since.
    fn installed(&self, part: &Part) -> bool {
        part.files.iter().all(|f| fs::metadata(self.file_path(part, f)).is_ok_and(|m| m.is_file() && m.len() == f.kept_size()))
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
            parts: PARTS
                .iter()
                .map(|p| PartStatus {
                    id: p.id,
                    homepage: p.homepage,
                    source_page: p.source_page,
                    installed: !missing.iter().any(|m| m.id == p.id),
                    files: p
                        .files
                        .iter()
                        .map(|f| FileStatus { name: f.name, url: f.url, size: f.size, sha256: f.sha256, license: f.license, extract: f.extract })
                        .collect(),
                })
                .collect(),
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
                if fs::metadata(self.file_path(part, file)).is_ok_and(|m| m.len() == file.kept_size()) {
                    self.progress(file.size);
                    continue;
                }
                self.fetch(&client, part, file)?;
            }
        }
        Ok(())
    }

    /// Download `file` to a `.part` file, then put it in place only if its size and SHA-256
    /// match; anything else deletes it. A zip download is checked the same way, then only its
    /// `extract` file is kept, once that matches its own size and SHA-256.
    fn fetch(&self, client: &reqwest::blocking::Client, part: &Part, file: &Download) -> Result<(), TranslatorError> {
        let target = self.file_path(part, file);
        let partial = target.with_extension(format!("{}.part", target.extension().and_then(|e| e.to_str()).unwrap_or("")));
        let download = partial.with_extension("download.part");
        info!("translator: downloading {} for the {} ({} bytes, {}, {}) from {}", file.name, part.label, file.size, file.source, file.license, file.url);
        let result = match file.extract {
            None => self.fetch_to(client, file, &partial),
            Some(extract) => self.fetch_to(client, file, &download).and_then(|()| {
                info!("translator: taking {} out of the download", extract.path);
                extract_from(&download, extract, file.name, &partial)
            }),
        };
        for leftover in [&download].into_iter().chain(result.is_err().then_some(&partial)) {
            if leftover.exists()
                && let Err(rm) = fs::remove_file(leftover)
            {
                warn!("translator: couldn't delete {}: {rm}", leftover.display());
            }
        }
        result?;
        Ok(fs::rename(&partial, &target)?)
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
        check.finish(file.name, file.size, file.sha256)
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

    fn finish(self, name: &str, size: u64, sha256: &str) -> Result<(), TranslatorError> {
        if self.len != size {
            return Err(TranslatorError::Size { name: name.to_string(), got: self.len, expected: size });
        }
        let got: String = self.hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if got != sha256 {
            return Err(TranslatorError::Checksum { name: name.to_string(), got });
        }
        Ok(())
    }
}

/// Copy `extract` out of the (already checked) zip at `zip_path` to `out`, checking its size
/// and SHA-256; `name` is what the error calls it.
fn extract_from(zip_path: &Path, extract: Extract, name: &str, out: &Path) -> Result<(), TranslatorError> {
    let mut archive = zip::ZipArchive::new(File::open(zip_path)?)?;
    let mut entry = archive.by_name(extract.path)?;
    let mut file = File::create(out)?;
    let mut check = Check::default();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = entry.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        check.update(&buf[..n]);
        if check.len > extract.size {
            break;
        }
    }
    file.sync_all()?;
    check.finish(name, extract.size, extract.sha256)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_is_kept_only_at_its_size_and_sha256_and_every_part_is_needed() {
        // SHA-256 of "abc" (FIPS 180-2 example).
        let file = Download { name: "x", url: "", size: 3, sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad", source: "", license: "", extract: None };
        let check = |bytes: &[u8]| {
            let mut c = Check::default();
            c.update(bytes);
            c.finish(file.name, file.size, file.sha256)
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
                File::create(tr.file_path(p, f)).unwrap().set_len(f.kept_size()).unwrap();
            }
        }
        assert!(!tr.status().downloaded);
        assert_eq!(tr.status().download_bytes, READER_TH.files.iter().map(|f| f.size).sum::<u64>());
        assert!(matches!(tr.set_enabled(true), Err(TranslatorError::NotDownloaded)));
        fs::create_dir_all(dir.join(READER_TH.id)).unwrap();
        for f in READER_TH.files {
            File::create(tr.file_path(&READER_TH, f)).unwrap().set_len(f.kept_size()).unwrap();
        }
        assert!(tr.status().downloaded);
        assert!(tr.set_enabled(true).is_ok());
        tr.remove().unwrap();
        assert!(!tr.status().downloaded);
        assert_eq!(tr.status().download_bytes, PARTS.iter().flat_map(|p| p.files).map(|f| f.size).sum::<u64>());
        fs::remove_dir_all(&dir).unwrap();
    }
}
