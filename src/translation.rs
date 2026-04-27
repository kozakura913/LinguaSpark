use crate::{AppError, AppState};
use isolang::Language;
use std::sync::Arc;

pub fn parse_language_code(code: &str) -> Result<Language, AppError> {
    Language::from_639_1(code.split('-').next().unwrap_or(code)).ok_or_else(|| {
        AppError::TranslationError(format!(
            "Invalid language code: '{}'. Please use ISO 639-1 format.",
            code
        ))
    })
}

fn get_iso_code(lang: &Language) -> Result<&'static str, AppError> {
    if lang.to_639_3() == "cmn" {
        // whichlang uses "cmn" for Chinese, but we want to return "zh" for ISO 639-1
        return Ok("zh");
    }
    lang.to_639_1().ok_or_else(|| {
        AppError::TranslationError(format!(
            "Language '{}' doesn't have an ISO 639-1 code",
            lang
        ))
    })
}

pub fn detect_language_code(text: &str) -> Result<&'static str, AppError> {
    get_iso_code(
        &Language::from_639_3(whichlang::detect_language(text).three_letter_code()).ok_or_else(
            || {
                AppError::TranslationError(format!(
                    "Failed to identify language for text: '{}'",
                    text
                ))
            },
        )?,
    )
}

pub async fn perform_translation(
    state: &Arc<AppState>,
    text: &str,
    from_lang: Option<String>,
    to_lang: &str,
) -> Result<(String, String, String), AppError> {
    let source_lang = match from_lang.as_deref() {
        None | Some("") | Some("auto") => Language::from_639_3(
            whichlang::detect_language(text).three_letter_code(),
        )
        .ok_or_else(|| {
            AppError::TranslationError(format!("Failed to detect language for text: '{}'", text))
        })?,
        Some(code) => parse_language_code(code)?,
    };

    let target_lang = parse_language_code(to_lang)?;

    let from_code = get_iso_code(&source_lang)?;
    let to_code = get_iso_code(&target_lang)?;

    // If source and target languages are the same, return the original text
    if from_code == to_code {
        return Ok((text.to_string(), from_code.to_string(), to_code.to_string()));
    }
    let loaded = if let Ok(models) = state.models.lock() {
        models.contains(&(source_lang, target_lang))
    } else {
        false
    };
    let interlanguage = if loaded {
        None
    } else {
        if state
            .downloader
            .available(from_code, to_code)
            .unwrap_or(false)
        {
            state
                .downloader
                .load_model(&state.translator, &state.models, from_code, to_code)
                .await?;
            None
        } else {
            //to en
            let interlanguage = "en";
            let to_inter = state
                .downloader
                .available(from_code, interlanguage)
                .unwrap_or(false);
            let from_inter = state
                .downloader
                .available(interlanguage, to_code)
                .unwrap_or(false);
            if !to_inter {
                return Err(AppError::TranslationError(format!(
                    "Translation from '{}' to '{}' is not supported",
                    from_code, interlanguage
                )));
            }
            if !from_inter {
                return Err(AppError::TranslationError(format!(
                    "Translation from '{}' to '{}' is not supported",
                    interlanguage, to_code
                )));
            }
            let download_to_inter=state
                .downloader
                .load_model(&state.translator, &state.models, from_code, interlanguage);
            let download_from_inter=state
                .downloader
                .load_model(&state.translator, &state.models, interlanguage, to_code);
            let (download_to_inter,download_from_inter)=tokio::join!(download_to_inter,download_from_inter);
            download_to_inter?;
            download_from_inter?;
            Some(interlanguage)
        }
    };

    let translated_text = if let Some(interlanguage) = interlanguage {
        let to_inter = state.translator.is_supported(from_code, interlanguage)?;
        let from_inter = state.translator.is_supported(interlanguage, to_code)?;
        if !to_inter {
            return Err(AppError::TranslationError(format!(
                "Translation from '{}' to '{}' is not supported",
                from_code, interlanguage
            )));
        }
        if !from_inter {
            return Err(AppError::TranslationError(format!(
                "Translation from '{}' to '{}' is not supported",
                interlanguage, to_code
            )));
        }
        let intertext = state.translator.translate(from_code, interlanguage, text)?;
        state
            .translator
            .translate(interlanguage, to_code, &intertext)?
    } else {
        if !state.translator.is_supported(from_code, to_code)? {
            return Err(AppError::TranslationError(format!(
                "Translation from '{}' to '{}' is not supported",
                from_code, to_code
            )));
        }
        state.translator.translate(from_code, to_code, text)?
    };

    Ok((translated_text, from_code.to_string(), to_code.to_string()))
}
