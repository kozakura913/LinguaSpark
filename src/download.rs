use isolang::Language;
use linguaspark_sys::Translator;
use std::{
    collections::{HashMap, HashSet},
    io,
    path::PathBuf,
    sync::Arc,
};

use tracing::{error, info};

use crate::{AppError, translation};

#[derive(Clone)]
pub(crate) struct ModelDownloader {
    models_dir: PathBuf,
    models: Option<serde_json::Value>,
    base_url: Option<String>,
    lock: Arc<tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
}
impl ModelDownloader {
    pub(crate) async fn new(models_dir: PathBuf) -> Self {
        let mut json_path = models_dir.clone();
        json_path.push("models.json");
        let models = match std::fs::read(&json_path) {
            Ok(b) => serde_json::from_slice::<serde_json::Value>(&b).ok(),
            Err(_) => None,
        };
        let models=async move{
            let req=reqwest::Client::new().get("https://storage.googleapis.com/moz-fx-translations-data--303e-prod-translations-data/db/models.json");
            let Ok(resp)=req.send().await else{
                return models;
            };
            let Ok(text)=resp.text().await else{
                return models;
            };
            let Ok(dl_models)=serde_json::from_str::<serde_json::Value>(&text) else{
                return models;
            };
            let Some(root)=dl_models.as_object()else{
                return models;
            };
            let Some(serde_json::Value::String(generated))=root.get("generated")else{
                return models;
            };
            if generated.len()<2{
                return models;
            }
            let _=tokio::fs::write(json_path, text.as_bytes()).await;
            return Some(dl_models);
        }.await;
        let root = models.as_ref().map(|v| v.as_object()).unwrap_or_default();
        let base_url = root.map(|v| v.get("baseUrl")).unwrap_or_default();
        let base_url = base_url.map(|v| v.as_str()).unwrap_or_default();
        let base_url = base_url.map(|v| v.to_string());
        Self {
            models_dir,
            models,
            base_url,
            lock: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        }
    }
    pub fn available(&self, from_lang: &str, to_lang: &str) -> Option<bool> {
        let root = self.models.as_ref()?.as_object()?;
        let models = root
            .get("models")?
            .as_object()?
            .get(&format!("{}-{}", from_lang, to_lang))?
            .as_array()?;
        Some(!models.is_empty())
    }
    async fn download_model(&self, from_lang: String, to_lang: String) -> Option<()> {
        let language_pair = format!("{}{}", from_lang, to_lang);
        let lock = {
            let mut map = self.lock.lock().await;
            if let Some(lock) = map.get(&language_pair) {
                lock.clone()
            } else {
                let lock = Arc::new(tokio::sync::Mutex::new(()));
                map.insert(language_pair.clone(), lock.clone());
                lock
            }
        };
        let _ = lock.lock();
        let root = self.models.as_ref()?.as_object()?;
        info!("download_model root found");
        let models = root
            .get("models")?
            .as_object()?
            .get(&format!("{}-{}", from_lang, to_lang))?
            .as_array()?;
        info!("download_model target lang {} models", models.len());
        let model = models.get(0)?;
        let files = model.get("files")?.as_object()?;
        info!("download_model files found");
        let s2t = files
            .get("lexicalShortlist")?
            .as_object()?
            .get("path")?
            .as_str()?; //model.s2t.bin
        let model = files.get("model")?.as_object()?.get("path")?.as_str()?; //model.intgemm8.bin
        info!("download_model all files found metadata");
        let mut path = self.models_dir.clone();
        path.push(&"tmp");
        path.push(&language_pair);
        if let Err(e) = tokio::fs::create_dir_all(&path).await {
            error!("download_model mkdir {:?}", e);
        }
        if let Ok(mut list) = tokio::fs::read_dir(&path).await {
            while let Ok(Some(f)) = list.next_entry().await {
                let _ = tokio::fs::remove_dir_all(f.path()).await;
            }
        }
        let mut path = self.models_dir.clone();
        path.push(&language_pair);
        if let Err(e) = tokio::fs::create_dir_all(&path).await {
            error!("download_model mkdir {:?}", e);
        }
        let s2t = self.download(&language_pair, "model.s2t.bin", s2t);
        let model = self.download(&language_pair, "model.intgemm8.bin", model);
        let vocab = async {
            if files.contains_key("vocab") {
                info!("model dl vocab...");
                let vocab = files.get("vocab")?.as_object()?.get("path")?.as_str()?; //vocab.spm
                self.download(&language_pair, "vocab.spm", vocab).await
            } else {
                let srcvocab = files.get("srcVocab")?.as_object()?.get("path")?.as_str()?; //srcvocab.spm
                let trgvocab = files.get("trgVocab")?.as_object()?.get("path")?.as_str()?; //trgvocab.spm
                let srcvocab = self.download(&language_pair, "srcvocab.spm", srcvocab);
                let trgvocab = self.download(&language_pair, "trgvocab.spm", trgvocab);
                let (srcvocab, trgvocab) = tokio::join!(srcvocab, trgvocab);
                info!("model dl srcvocab...");
                srcvocab?;
                info!("model dl trgvocab...");
                trgvocab
            }
        };
        let (s2t, model, vocab) = tokio::join!(s2t, model, vocab);
        s2t?;
        model?;
        vocab?;
        let mut path = self.models_dir.clone();
        path.push(&"tmp");
        path.push(&language_pair);
        if let Err(e) = tokio::fs::remove_dir(&path).await {
            error!("download_model rm temp dir {:?}", e);
        }
        info!("download model all ok");
        Some(())
    }
    async fn download(&self, language_pair: &str, name: &str, remote_path: &str) -> Option<()> {
        let mut target_path = self.models_dir.clone();
        target_path.push(language_pair);
        target_path.push(name);
        if tokio::fs::try_exists(&target_path).await.ok()? {
            return Some(());
        }
        let mut temp_path = self.models_dir.clone();
        temp_path.push("tmp");
        temp_path.push(language_pair);
        temp_path.push(name);
        if self
            .download0(&temp_path, name, remote_path)
            .await
            .is_none()
        {
            error!("download failed {} {}", language_pair, name);
            let _ = tokio::fs::remove_file(temp_path).await;
            None
        } else {
            tokio::fs::rename(&temp_path, &target_path)
                .await
                .map_err(|e| {
                    error!(
                        "model download failed move '{}' to '{}' {:?}",
                        temp_path.to_string_lossy(),
                        target_path.to_string_lossy(),
                        e
                    )
                })
                .ok()
        }
    }
    async fn download0(&self, path: &PathBuf, name: &str, remote_path: &str) -> Option<()> {
        let total_len = match tokio::fs::File::create(&path).await {
            Ok(mut file) => {
                let url = format!("{}/{}", self.base_url.as_ref()?, remote_path);
                info!("download {} -> {}", url, &path.to_string_lossy());
                let req = reqwest::Client::new().get(url);
                let resp = req
                    .send()
                    .await
                    .map_err(|e| error!("model download failed {} {:?}", name, e))
                    .ok()?;
                use futures_util::stream::TryStreamExt;
                let byte_stream = resp
                    .bytes_stream()
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e));
                let reader = tokio_util::io::StreamReader::new(byte_stream);
                let mut decoder = async_compression::tokio::bufread::GzipDecoder::new(reader);
                match tokio::io::copy(&mut decoder, &mut file).await {
                    Err(e) => {
                        error!("download_model write {:?}", e);
                        return None;
                    }
                    Ok(len) => len,
                }
            }
            Err(e) => {
                error!("download_model write {:?}", e);
                return None;
            }
        };
        info!("download_model {} {}bytes", name, total_len);
        Some(())
    }
    pub(crate) async fn load_model(
        &self,
        translator: &Translator,
        models: &std::sync::Mutex<HashSet<(Language, Language)>>,
        from_lang_s: &str,
        to_lang_s: &str,
    ) -> Result<(), AppError> {
        let language_pair = format!("{}{}", from_lang_s, to_lang_s);
        let from_lang = translation::parse_language_code(from_lang_s)?;
        let to_lang = translation::parse_language_code(to_lang_s)?;
        {
            let models = models
                .lock()
                .map_err(|e| AppError::ConfigError(format!("{:?}", e)))?;
            if models.contains(&(from_lang, to_lang)) {
                return Ok(());
            }
        }
        let from_lang_s = from_lang_s.to_string();
        let to_lang_s = to_lang_s.to_string();
        let cloned_self = self.clone();
        let download_job = tokio::runtime::Handle::current()
            .spawn(async move { cloned_self.download_model(from_lang_s, to_lang_s).await });
        match download_job.await {
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => {
                return Err(AppError::TranslationError(
                    "model download failed".to_string(),
                ));
            }
        }
        let mut models = models
            .lock()
            .map_err(|e| AppError::ConfigError(format!("{:?}", e)))?;
        if models.contains(&(from_lang, to_lang)) {
            return Ok(());
        }
        let mut model_dir_path = self.models_dir.clone();
        model_dir_path.push(&language_pair);
        info!("Looking for models in {}", model_dir_path.display());
        translator.load_model(&language_pair, model_dir_path)?;
        models.insert((from_lang, to_lang));

        info!("Loaded model for language pair '{}'", language_pair);
        Ok(())
    }
}
