use anyhow::{Context, Result};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Json;
use axum::Router;
use std::sync::Arc;

use crate::settings;

use super::models::{ErrorResponse, ServerRequest, ServerResponse};
use super::state::ServerState;
use super::translate::translate_request;

pub async fn run_server(settings: settings::Settings, addr: String) -> Result<()> {
    let state = Arc::new(ServerState {
        settings,
        registry: crate::languages::LanguageRegistry::load()?,
    });
    let app = Router::new()
        .route("/health", get(health))
        .route("/translate", post(translate))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| "failed to bind server address")?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> impl IntoResponse {
    (StatusCode::OK, Json(serde_json::json!({ "status": "ok" })))
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
        Err(err) => Err((
            err.status,
            Json(ErrorResponse {
                error: err.message,
            }),
        )),
    }
}
