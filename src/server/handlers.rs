use anyhow::{Context, Result};
use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, Method, Request, Response, StatusCode};
use axum::middleware::Next;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use std::sync::Arc;

use crate::settings;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

use super::models::{ErrorResponse, ServerRequest, ServerResponse};
use super::state::ServerState;
use super::translate::translate_request;
use crate::model_registry;
use std::collections::HashMap;
use std::path::PathBuf;

pub async fn run_server(settings: settings::Settings, addr: String) -> Result<()> {
    let state = Arc::new(ServerState {
        settings,
        registry: crate::languages::LanguageRegistry::load()?,
    });
    let app = Router::new()
        .route("/health", get(health))
        .route("/translate", post(translate))
        .route("/histories", get(histories))
        .route("/history-content", get(history_content))
        .route("/trend", get(trend))
        .route("/settings", get(settings_info))
        .with_state(state)
        .layer(axum::middleware::from_fn(cors_middleware));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| "failed to bind server address")?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({ "status": "ok" })))
}

async fn cors_middleware(req: Request<Body>, next: Next) -> Result<Response<Body>, StatusCode> {
    if req.method() == Method::OPTIONS {
        let mut response = Response::new(Body::empty());
        *response.status_mut() = StatusCode::NO_CONTENT;
        apply_cors_headers(response.headers_mut());
        return Ok(response);
    }
    let mut response = next.run(req).await;
    apply_cors_headers(response.headers_mut());
    Ok(response)
}

fn apply_cors_headers(headers: &mut HeaderMap) {
    headers.insert("access-control-allow-origin", HeaderValue::from_static("*"));
    headers.insert(
        "access-control-allow-methods",
        HeaderValue::from_static("GET,POST,OPTIONS"),
    );
    headers.insert(
        "access-control-allow-headers",
        HeaderValue::from_static("content-type,authorization"),
    );
}

async fn translate(
    State(state): State<Arc<ServerState>>,
    Json(payload): Json<ServerRequest>,
) -> Result<Json<ServerResponse>, (StatusCode, Json<ErrorResponse>)> {
    let state = state.clone();
    let handle = tokio::runtime::Handle::current();
    let result = tokio::task::spawn_blocking(move || {
        handle.block_on(translate_request(state.as_ref(), payload))
    })
    .await
    .map_err(|err| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("server task failed: {}", err),
            }),
        )
    })?;

    match result {
        Ok(response) => Ok(Json(response)),
        Err(err) => Err((err.status, Json(ErrorResponse { error: err.message }))),
    }
}

async fn histories(
    State(state): State<Arc<ServerState>>,
) -> Result<Json<Vec<model_registry::HistoryEntry>>, (StatusCode, Json<ErrorResponse>)> {
    let _ = state;
    let histories = model_registry::get_histories().map_err(|err| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: err.to_string(),
            }),
        )
    })?;
    Ok(Json(histories))
}

#[derive(serde::Deserialize)]
struct HistoryContentQuery {
    path: String,
}

#[derive(serde::Serialize)]
struct HistoryContentResponse {
    data_base64: String,
    mime: String,
}

async fn history_content(
    State(state): State<Arc<ServerState>>,
    Query(query): Query<HistoryContentQuery>,
) -> Result<Json<HistoryContentResponse>, (StatusCode, Json<ErrorResponse>)> {
    let _ = state;
    let raw_path = query.path.trim();
    if raw_path.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "path is required".to_string(),
            }),
        ));
    }
    let path = PathBuf::from(raw_path);
    let history_dir = crate::build_env::history_dest_dir();
    let canonical_history = std::fs::canonicalize(&history_dir).unwrap_or(history_dir);
    let canonical_path = std::fs::canonicalize(&path).map_err(|err| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("failed to read history content: {}", err),
            }),
        )
    })?;
    if !canonical_path.starts_with(&canonical_history) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse {
                error: "history path is not allowed".to_string(),
            }),
        ));
    }
    let bytes = std::fs::read(&canonical_path).map_err(|err| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: format!("failed to read history content: {}", err),
            }),
        )
    })?;
    let data_base64 = BASE64.encode(&bytes);
    let mime = model_registry::get_histories()
        .ok()
        .and_then(|items| {
            items
                .into_iter()
                .find(|item| item.dest == raw_path)
                .map(|item| item.mime)
        })
        .unwrap_or_else(|| "application/octet-stream".to_string());
    Ok(Json(HistoryContentResponse { data_base64, mime }))
}

async fn trend(
    State(state): State<Arc<ServerState>>,
) -> Result<Json<model_registry::TrendMeta>, (StatusCode, Json<ErrorResponse>)> {
    let _ = state;
    let trend = model_registry::get_trend().map_err(|err| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: err.to_string(),
            }),
        )
    })?;
    Ok(Json(trend))
}

#[derive(serde::Serialize)]
struct SettingsInfo {
    formal_keys: Vec<String>,
    languages: Vec<LanguageOption>,
    default_formal: String,
    default_lang: String,
    default_source_lang: String,
    labels: HashMap<String, String>,
    models: Vec<String>,
    last_model: Option<String>,
}

#[derive(serde::Serialize)]
struct LanguageOption {
    value: String,
    label: String,
}

#[derive(serde::Deserialize)]
struct SettingsQuery {
    ui_lang: Option<String>,
}

async fn settings_info(
    State(state): State<Arc<ServerState>>,
    Query(query): Query<SettingsQuery>,
) -> Json<SettingsInfo> {
    let mut formal_keys = state.settings.formally.keys().cloned().collect::<Vec<_>>();
    formal_keys.sort();
    let language_codes = state.settings.system_languages.clone();
    let ui_lang = query.ui_lang.as_deref().unwrap_or("eng").trim().to_string();
    let labels = crate::languages::load_client_labels(&ui_lang);
    let models = model_registry::get_cached_models().unwrap_or_default();
    let last_model = model_registry::get_last_using_model().unwrap_or(None);
    let languages = language_codes
        .iter()
        .map(|code| {
            let label = crate::languages::language_autonym(code)
                .or_else(|| state.registry.iso_name(code))
                .unwrap_or_else(|| code.clone());
            LanguageOption {
                value: code.clone(),
                label,
            }
        })
        .collect::<Vec<_>>();
    Json(SettingsInfo {
        formal_keys,
        languages,
        default_formal: "formal".to_string(),
        default_lang: "en".to_string(),
        default_source_lang: "auto".to_string(),
        labels,
        models,
        last_model,
    })
}
