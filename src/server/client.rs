use anyhow::{Context, Result};
use axum::Router;
use axum::response::Html;
use axum::routing::get;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tera::{Context as TeraContext, Tera};

pub async fn run_client(addr: String, api_base: String) -> Result<()> {
    let html = Arc::new(render_client_html(&api_base)?);
    let app = Router::new().route(
        "/",
        get({
            let html = html.clone();
            move || {
                let html = html.clone();
                async move { Html((*html).clone()) }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| "failed to bind client address")?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn render_client_html(api_base: &str) -> Result<String> {
    let template = load_client_template("client.html.tera")?;
    let mut context = TeraContext::new();
    context.insert("api_base_json", &serde_json::to_string(api_base)?);
    Tera::one_off(&template, &context, false).with_context(|| "failed to render client template")
}

fn load_client_template(name: &str) -> Result<String> {
    let path = client_template_path(name)?;
    fs::read_to_string(&path)
        .with_context(|| format!("failed to read client template: {}", path.display()))
}

fn client_template_path(name: &str) -> Result<PathBuf> {
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("translations")
        .join("templates")
        .join(name))
}
