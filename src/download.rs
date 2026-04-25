use isolang::Language;
use linguaspark_sys::Translator;
use std::{collections::HashSet, io, path::PathBuf};

use tracing::{error, info};

use crate::{AppError, translation};

#[derive(Clone)]
pub(crate) struct ModelDownloader {
    models_dir: PathBuf,
    models: Option<serde_json::Value>,
    base_url: Option<String>,
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
        }
    }
    async fn download_model(&self, from_lang: String, to_lang: String) -> Option<()> {
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
        info!("download_model all files found");
        let language_pair = format!("{}{}", from_lang, to_lang);
        let mut path = self.models_dir.clone();
        path.push(&language_pair);
        if let Err(e) = tokio::fs::create_dir_all(&path).await {
            error!("download_model mkdir {:?}", e);
        }
        let s2t = self.download(&language_pair, "model.s2t.bin", s2t);
        let model = self.download(&language_pair, "model.intgemm8.bin", model);
        let vocab = async {
            if files.contains_key("vocab") {
                info!("download_model dl vocab");
                let vocab = files.get("vocab")?.as_object()?.get("path")?.as_str()?; //vocab.spm
                self.download(&language_pair, "vocab.spm", vocab).await
            } else {
                let srcvocab = files.get("srcVocab")?.as_object()?.get("path")?.as_str()?; //srcvocab.spm
                let trgvocab = files.get("trgVocab")?.as_object()?.get("path")?.as_str()?; //trgvocab.spm
                let srcvocab = self.download(&language_pair, "srcvocab.spm", srcvocab);
                let trgvocab = self.download(&language_pair, "trgvocab.spm", trgvocab);
                let (srcvocab, trgvocab) = tokio::join!(srcvocab, trgvocab);
                info!("download_model dl srcvocab");
                srcvocab?;
                info!("download_model dl trgvocab");
                trgvocab
            }
        };
        let (s2t, model, vocab) = tokio::join!(s2t, model, vocab);
        s2t?;
        model?;
        vocab?;
        info!("download_model dl all");
        Some(())
    }
    async fn download(&self, language_pair: &str, name: &str, remote_path: &str) -> Option<()> {
        let mut path = self.models_dir.clone();
        path.push(language_pair);
        path.push(name);
        if tokio::fs::try_exists(&path).await.ok()? {
            return Some(());
        }
        let req =
            reqwest::Client::new().get(format!("{}/{}", self.base_url.as_ref()?, remote_path));
        let resp = req.send().await.ok()?;
        use futures_util::stream::TryStreamExt;
        let byte_stream = resp
            .bytes_stream()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e));
        let reader = tokio_util::io::StreamReader::new(byte_stream);
        let mut decoder = async_compression::tokio::bufread::GzipDecoder::new(reader);
        let total_len = match tokio::fs::File::create(path).await {
            Ok(mut file) => match tokio::io::copy(&mut decoder, &mut file).await {
                Err(e) => {
                    error!("download_model write {:?}", e);
                    return None;
                }
                Ok(len) => len,
            },
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
        if self.download_model(from_lang_s, to_lang_s).await.is_none() {
            return Err(AppError::TranslationError(
                "model download failed".to_string(),
            ));
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
