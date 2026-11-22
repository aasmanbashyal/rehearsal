use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio::io::AsyncWriteExt;

const VERSION: &str = "0.4.2";
const ENGINE: Option<(&str, &str, &str)> = if cfg!(all(windows, target_arch = "x86_64")) {
    Some((
        "windows-x86_64-cpu-vulkan",
        "transcribe.dll",
        "6094102cf57b404b050707ceb258b12ce647b92e801b5d46077214e5b030c8aa",
    ))
} else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
    Some((
        "linux-x86_64-cpu-vulkan",
        "libtranscribe.so",
        "4fe87db8919ea34e461db7987713b580f54e6f3ed610457c5d8fb8eac1a15046",
    ))
} else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
    Some(("linux-aarch64-cpu-vulkan", "libtranscribe.so",
        "5e9cc7b244949fc686d1272e796e965411e80cc768533f1704c3c53e4c7d695e"))
} else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
    Some(("macos-arm64-metal", "libtranscribe.dylib",
        "75ea3a705f3edec09942fdbb1b2b5509d7ecd75bda55158a47a3ab1e2d1105b3"))
} else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
    Some(("macos-x86_64-cpu", "libtranscribe.dylib",
        "ca414e5b28706a2f2ecea1479b3e1c5f47d0f11f43aba86e289ae5c10b1ca40c"))
} else {
    None
};
const CATALOG: &str =
    "https://raw.githubusercontent.com/cjpais/Handy/main/src-tauri/src/catalog/catalog.json";

#[derive(Clone, Deserialize, Serialize)]
pub struct Model {
    pub id: String,
    pub name: String,
    size: u64,
    word_error_rate: Option<f64>,
    sha: String,
    revision: String,
    repo: String,
    filename: String,
}

#[derive(Serialize)]
struct Download {
    id: String,
    stage: &'static str,
    loaded: u64,
    total: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

pub struct Models {
    root: PathBuf,
    entries: RwLock<Vec<Model>>,
    download: Mutex<Option<Download>>,
    refresh: tokio::sync::Mutex<()>,
    client: reqwest::Client,
}

impl Models {
    pub fn new(root: PathBuf, client: reqwest::Client) -> Self {
        let mut entries = built_in();
        // Browser installations previously cached the upstream catalog as-is.
        // Read it before the Rust cache so downloaded models keep their names.
        if let Ok(bytes) = std::fs::read(root.join("handy-catalog.json"))
            && let Ok(catalog) = serde_json::from_slice::<Value>(&bytes)
            && catalog["schema"] == 2
            && let Some(rows) = catalog["models"].as_array()
        {
            for model in rows.iter().filter_map(|row| from_catalog(row, &catalog)) {
                merge(&mut entries, model, &root);
            }
        }
        if let Ok(bytes) = std::fs::read(root.join("model-catalog.json"))
            && let Ok(cached) = serde_json::from_slice::<Vec<Model>>(&bytes)
        {
            for model in cached.into_iter().filter(valid) {
                merge(&mut entries, model, &root);
            }
        }
        Self {
            root,
            entries: RwLock::new(entries),
            download: Mutex::new(None),
            refresh: Default::default(),
            client,
        }
    }

    pub fn engine(&self) -> Option<PathBuf> {
        let path = std::env::var_os("TRANSCRIBE_LIB")
            .map(PathBuf::from)
            .or_else(|| {
                let (platform, library, _) = ENGINE?;
                Some(
                    self.root
                        .join(format!("engine-{VERSION}"))
                        .join(format!("transcribe-native-{platform}"))
                        .join(library),
                )
            })?;
        path.is_file().then_some(path)
    }

    pub fn file(&self, model: &Model) -> PathBuf {
        self.root.join("models").join(&model.filename)
    }

    pub fn installed(&self, model: &Model) -> bool {
        valid_file(&self.root, model)
    }

    pub fn model(&self, id: &str) -> Option<Model> {
        self.entries
            .read()
            .unwrap()
            .iter()
            .find(|model| model.id == id)
            .cloned()
    }

    pub fn status(&self) -> Value {
        let ready = self.engine().is_some();
        let models: Vec<_> = self.entries.read().unwrap().iter().map(|model| json!({
            "id": model.id, "name": model.name, "size": model.size, "wordErrorRate": model.word_error_rate,
            "installed": self.installed(model), "recommended": model.id == "parakeet-110m",
        })).collect();
        json!({ "engineReady": ready, "canInstall": Self::can_install() || ready,
            "download": *self.download.lock().unwrap(), "models": models })
    }

    pub fn can_install() -> bool {
        ENGINE.is_some()
    }

    pub fn start_install(self: &Arc<Self>, model: Model) -> Result<()> {
        let mut download = self.download.lock().unwrap();
        if download
            .as_ref()
            .is_some_and(|state| state.stage != "error")
        {
            bail!("Another download is in progress.");
        }
        *download = Some(Download {
            id: model.id.clone(),
            stage: "starting",
            loaded: 0,
            total: 0,
            message: None,
        });
        let models = Arc::clone(self);
        tokio::spawn(async move {
            let result = models.install(&model).await;
            *models.download.lock().unwrap() = result.err().map(|error| Download {
                id: model.id,
                stage: "error",
                loaded: 0,
                total: 0,
                message: Some(error.to_string()),
            });
        });
        Ok(())
    }

    async fn install(&self, model: &Model) -> Result<()> {
        // The archive, model downloads and published engines share this profile.
        // Hold the OS lock from the readiness checks through the final renames.
        let _install_lock = lock_install(&self.root).await?;
        if self.engine().is_none() {
            let (platform, library, checksum) =
                ENGINE.context("Install a compatible transcribe.cpp engine first.")?;
            let name = format!("transcribe-native-{VERSION}-{platform}.tar.gz");
            let url = format!(
                "https://github.com/handy-computer/transcribe.cpp/releases/download/v{VERSION}/{name}"
            );
            let work = tempfile::tempdir_in(&self.root)?;
            let archive = work.path().join(name);
            self.fetch(&url, &archive, checksum, "engine").await?;
            let target = self.root.join(format!("engine-{VERSION}"));
            tokio::task::spawn_blocking(move || -> Result<()> {
                let reader = flate2::read::GzDecoder::new(std::fs::File::open(archive)?);
                tar::Archive::new(reader).unpack(work.path())?;
                let engine = work.path().join(format!("transcribe-native-{platform}"));
                if !engine.join(library).is_file() {
                    bail!("No speech engine found in the archive.");
                }
                // Each platform's engine has its own subfolder. Move only this
                // one, so a data folder that already holds another platform's
                // engine (a universal macOS app run on Intel and Apple silicon,
                // or data migrated between Macs) still accepts this install.
                std::fs::create_dir_all(&target)?;
                let installed = target.join(format!("transcribe-native-{platform}"));
                if installed.exists() {
                    if installed.join(library).is_file() {
                        return Ok(());
                    }
                    std::fs::remove_dir_all(&installed)?;
                }
                std::fs::rename(&engine, installed)?;
                Ok(())
            })
            .await??;
        }
        if !self.installed(model) {
            let url = format!(
                "https://huggingface.co/{}/resolve/{}/{}",
                model.repo, model.revision, model.filename
            );
            self.fetch(&url, &self.file(model), &model.sha, "model")
                .await?;
        }
        Ok(())
    }

    async fn fetch(
        &self,
        url: &str,
        target: &Path,
        expected: &str,
        stage: &'static str,
    ) -> Result<()> {
        tokio::fs::create_dir_all(target.parent().context("Missing download folder.")?).await?;
        let temporary = target.with_extension(format!(
            "{}.partial",
            target.extension().unwrap_or_default().to_string_lossy()
        ));
        self.progress(stage, 0, 0);
        let result = async {
            let response = self
                .client
                .get(url)
                .timeout(Duration::from_secs(20 * 60))
                .send()
                .await?
                .error_for_status()?;
            let total = response.content_length().unwrap_or(0);
            let mut body = response.bytes_stream();
            let mut hash = Sha256::new();
            let mut file = tokio::fs::File::create(&temporary).await?;
            let mut loaded = 0_usize;
            while let Some(bytes) = body.next().await {
                let bytes = bytes?;
                loaded += bytes.len();
                if loaded > 2 * 1024 * 1024 * 1024 {
                    bail!("The download is larger than expected.");
                }
                hash.update(&bytes);
                file.write_all(&bytes).await?;
                self.progress(stage, loaded as u64, total);
            }
            file.flush().await?;
            drop(file);
            if format!("{:x}", hash.finalize()) != expected {
                bail!("The download did not match the published checksum.");
            }
            tokio::fs::rename(&temporary, target).await?;
            Ok(())
        }
        .await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(temporary).await;
        }
        result
    }

    fn progress(&self, stage: &'static str, loaded: u64, total: u64) {
        if let Some(status) = self.download.lock().unwrap().as_mut() {
            status.stage = stage;
            status.loaded = loaded;
            status.total = total;
        }
    }

    pub async fn refresh_catalog(&self) -> bool {
        let _refresh = self.refresh.lock().await;
        let file = self.root.join("model-catalog.json");
        if let Ok(metadata) = tokio::fs::metadata(&file).await
            && metadata
                .modified()
                .ok()
                .and_then(|time| time.elapsed().ok())
                .is_some_and(|elapsed| elapsed.as_secs() < 86_400)
        {
            return true;
        }
        let Ok(response) = self
            .client
            .get(CATALOG)
            .timeout(Duration::from_secs(7))
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
        else {
            return false;
        };
        let Ok(data) = response.json::<Value>().await else {
            return false;
        };
        if data["schema"] != 2 {
            return false;
        }
        let Some(rows) = data["models"].as_array() else {
            return false;
        };
        let available: Vec<_> = rows
            .iter()
            .filter_map(|entry| from_catalog(entry, &data))
            .collect();
        if available.is_empty() {
            return false;
        }
        let entries = {
            let previous = self.entries.read().unwrap();
            let mut entries = built_in();
            for old in previous.iter().filter(|entry| self.installed(entry)) {
                merge(&mut entries, old.clone(), &self.root);
            }
            for model in available {
                merge(&mut entries, model, &self.root);
            }
            entries
        };
        *self.entries.write().unwrap() = entries.clone();
        if let Ok(json) = serde_json::to_vec(&entries) {
            let _ = tokio::fs::write(file, json).await;
        }
        true
    }
}

async fn lock_install(root: &Path) -> Result<std::fs::File> {
    tokio::fs::create_dir_all(root).await?;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("model-install.lock"))?;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) => {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            Err(std::fs::TryLockError::Error(error)) => return Err(error.into()),
        }
    }
}

fn valid_file(root: &Path, model: &Model) -> bool {
    root.join("models")
        .join(&model.filename)
        .metadata()
        .is_ok_and(|file| file.len() == model.size)
}

#[cfg(test)]
mod install_tests {
    use super::*;

    #[tokio::test]
    async fn overlapping_installs_recheck_the_engine_and_model_after_the_lock() -> Result<()> {
        let Some((platform, library, _)) = ENGINE else { return Ok(()) };
        let root = tempfile::tempdir()?;
        let first = lock_install(root.path()).await?;
        let models = Models::new(root.path().to_owned(), reqwest::Client::new());
        let mut model = built_in().remove(0);
        model.size = 4;
        let second = models.install(&model);
        tokio::pin!(second);
        assert!(tokio::time::timeout(Duration::from_millis(50), &mut second).await.is_err());

        let engine = root.path().join(format!("engine-{VERSION}/transcribe-native-{platform}"));
        std::fs::create_dir_all(&engine)?;
        std::fs::write(engine.join(library), b"first engine")?;
        std::fs::create_dir_all(root.path().join("models"))?;
        std::fs::write(models.file(&model), b"data")?;
        drop(first);
        tokio::time::timeout(Duration::from_secs(5), second).await??;
        assert_eq!(std::fs::read(engine.join(library))?, b"first engine");
        Ok(())
    }
}

fn merge(entries: &mut Vec<Model>, model: Model, root: &Path) {
    if let Some(old) = entries.iter_mut().find(|entry| entry.id == model.id) {
        if old.sha == model.sha || !valid_file(root, old) {
            *old = model;
        }
    } else {
        entries.push(model);
    }
}

fn from_catalog(entry: &Value, catalog: &Value) -> Option<Model> {
    if !matches!(entry["family"].as_str(), Some("parakeet" | "whisper"))
        || entry["role"] != "asr"
        || entry["curated"] != true
        || entry["capabilities"]["timestamps"] != true
        || !entry["languages"]
            .as_array()?
            .iter()
            .any(|lang| lang == "en")
    {
        return None;
    }
    let version: Vec<u16> = entry["minEngine"]
        .as_str()?
        .split('.')
        .map(str::parse)
        .collect::<std::result::Result<_, _>>()
        .ok()?;
    if version.len() != 3 || version.as_slice() > &[0, 4, 2] {
        return None;
    }
    let id = entry["id"].as_str()?;
    let preferred = match id {
        "parakeet-tdt_ctc-110m" => "parakeet-110m",
        "parakeet-unified-en-0.6b" => "parakeet-unified",
        "parakeet-110m" | "parakeet-unified" => return None,
        other => other,
    };
    let result = &catalog["benchmarks"];
    let word_error_rate = if result["quant"] == "Q8_0"
        && result["accuracy"]["dataset"] == "fleurs"
        && result["accuracy"]["split"] == "test"
    {
        catalog["results"][id]["accuracy"]["en"]["error"]
            .as_f64()
            .filter(|number| number.is_finite() && *number >= 0.0)
    } else {
        None
    };
    let file = &entry["files"]["Q8_0"];
    let model = Model {
        id: preferred.into(),
        name: entry["name"].as_str()?.into(),
        size: file["size"].as_u64()?,
        word_error_rate,
        sha: file["sha256"].as_str()?.into(),
        revision: entry["revision"].as_str()?.into(),
        repo: entry["repository"].as_str()?.into(),
        filename: file["filename"].as_str()?.into(),
    };
    valid(&model).then_some(model)
}

fn valid(model: &Model) -> bool {
    let safe = |part: &str| {
        !part.is_empty()
            && !part.starts_with('.')
            && part
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b".-_".contains(&byte))
    };
    let hex = |code: &str, length| {
        code.len() == length
            && code
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    };
    safe(&model.id)
        && model.id.len() <= 72
        && !model.name.trim().is_empty()
        && model.name.chars().count() <= 120
        && (1..=2_147_483_648).contains(&model.size)
        && hex(&model.sha, 64)
        && hex(&model.revision, 40)
        && model
            .repo
            .strip_prefix("handy-computer/")
            .is_some_and(|part| safe(part) && part.ends_with("-gguf"))
        && safe(&model.filename)
        && model.filename.ends_with("-Q8_0.gguf")
}

fn built_in() -> Vec<Model> {
    [
        (
            "parakeet-110m",
            "Parakeet TDT-CTC 110M",
            "parakeet-tdt_ctc-110m",
            135373280,
            Some(6.11),
            "7dd44c74a331d788a4e5f8b16913b3feb29ced22cf5613aad0e0f6cd30516296",
            "9d66d34f9e1594075c5dd72c90c0f4c321b29f21",
        ),
        (
            "parakeet-unified",
            "Parakeet Unified English 0.6B",
            "parakeet-unified-en-0.6b",
            731357568,
            Some(3.99),
            "4b50b6dd862bf6e346929aaf4f5eaacec003bfa3f56462d6c874b41ef2f38795",
            "7e948f21b7bdbac698d3318db9d350f1096f3b6c",
        ),
        (
            "whisper-tiny.en",
            "Whisper Tiny English",
            "whisper-tiny.en",
            45904544,
            Some(10.72),
            "e8c9b73c06344307d8b346e07fbe93dd88d894627854bcff31523f1ce44394fa",
            "becb8bcb804405dc97b380a523d9975888820986",
        ),
        (
            "whisper-small.en",
            "Whisper Small English",
            "whisper-small.en",
            269674144,
            None,
            "9614e6b7fda2d26018e4f268aece8ca25a83296ea0b534169a585b740bfd71ef",
            "41b0f75fd44415ba127a5356c5ba9ed450c1debd",
        ),
    ]
    .into_iter()
    .map(|(id, name, stem, size, wer, sha, revision)| Model {
        id: id.into(),
        name: name.into(),
        size,
        word_error_rate: wer,
        sha: sha.into(),
        revision: revision.into(),
        repo: format!("handy-computer/{stem}-gguf"),
        filename: format!("{stem}-Q8_0.gguf"),
    })
    .collect()
}
