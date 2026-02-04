use anyhow::{Context, Result};
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, Method, Request, Response, StatusCode};
use axum::middleware::Next;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use std::sync::Arc;

use crate::settings;

use super::models::{ErrorResponse, ServerRequest, ServerResponse};
use super::state::ServerState;
use super::translate::translate_request;
use crate::model_registry;

pub async fn run_server(settings: settings::Settings, addr: String) -> Result<()> {
    let state = Arc::new(ServerState {
        settings,
        registry: crate::languages::LanguageRegistry::load()?,
    });
    let app = Router::new()
        .route("/health", get(health))
        .route("/translate", post(translate))
        .route("/histories", get(histories))
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
    languages: Vec<String>,
    default_formal: String,
    default_lang: String,
    default_source_lang: String,
}

async fn settings_info(State(state): State<Arc<ServerState>>) -> Json<SettingsInfo> {
    let mut formal_keys = state.settings.formally.keys().cloned().collect::<Vec<_>>();
    formal_keys.sort();
    let languages = state.settings.system_languages.clone();
    Json(SettingsInfo {
        formal_keys,
        languages,
        default_formal: "formal".to_string(),
        default_lang: "en".to_string(),
        default_source_lang: "auto".to_string(),
    })
}
