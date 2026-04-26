use crate::{
    AppError, AppState,
    translation::{detect_language_code, perform_translation},
};
use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Deserialize)]
pub struct DetectLanguageRequest {
    q: String,
    api_key: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DetectLanguageResponse {
    language: String,
}

pub async fn detect_language(
    Json(request): Json<DetectLanguageRequest>,
) -> Result<Json<DetectLanguageResponse>, AppError> {
    if let Ok(api_key) = std::env::var(crate::ENV_API_KEY) {
        if let Some(client_key) = &request.api_key {
            if client_key != &api_key {
                return Err(AppError::Unauthorized);
            }
        } else {
            return Err(AppError::Unauthorized);
        }
    }
    Ok(Json(DetectLanguageResponse {
        language: detect_language_code(&request.q)?.to_owned(),
    }))
}

#[derive(Debug, Deserialize)]
pub struct TranslationRequest {
    q: String,
    source: Option<String>,
    #[serde(rename = "format")]
    _format: Option<String>, //input format not support
    target: String,
    api_key: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TranslationResponse {
    #[serde(rename = "translatedText")]
    translated_text: String,
    #[serde(rename = "detectedLanguage")]
    detected_language: DetectedLanguage,
}

#[derive(Debug, Serialize)]
pub struct DetectedLanguage {
    confidence: f32,
    language: String,
}
pub async fn translate(
    State(state): State<Arc<AppState>>,
    Json(request): Json<TranslationRequest>,
) -> Result<Json<TranslationResponse>, AppError> {
    if let Ok(api_key) = std::env::var(crate::ENV_API_KEY) {
        if let Some(client_key) = &request.api_key {
            if client_key != &api_key {
                return Err(AppError::Unauthorized);
            }
        } else {
            return Err(AppError::Unauthorized);
        }
    }
    let (translated_text, from_lang, _to_lang) =
        perform_translation(&state, &request.q, request.source, &request.target).await?;

    Ok(Json(TranslationResponse {
        translated_text,
        detected_language: DetectedLanguage {
            confidence: 50.0, //dummy
            language: from_lang,
        },
    }))
}
