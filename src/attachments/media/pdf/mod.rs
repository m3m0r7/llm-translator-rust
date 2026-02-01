use anyhow::{anyhow, Context, Result};
use std::fs;
use std::process::Command;
use tempfile::tempdir;

use crate::data;
use crate::providers::Provider;
use crate::{TranslateOptions, Translator};

use super::audio::command_exists;
use crate::attachments::cache::TranslationCache;
use crate::attachments::AttachmentTranslation;

use super::image::{translate_image_with_cache, ImageTranslateRequest};
use super::ocr::OcrDebugConfig;

pub(crate) async fn translate_pdf<P: Provider + Clone>(
    pdf_bytes: &[u8],
    ocr_languages: &str,
    translator: &Translator<P>,
    options: &TranslateOptions,
    debug: Option<OcrDebugConfig>,
) -> Result<AttachmentTranslation> {
    let pages = render_pdf_pages(pdf_bytes)?;
    if pages.is_empty() {
        return Err(anyhow!("no pages found in pdf"));
    }

    let mut cache = TranslationCache::new();
    let mut translated_images = Vec::new();
    for (index, page) in pages.into_iter().enumerate() {
        let debug_page = debug.as_ref().map(|config| config.for_page(index));
        let output = translate_image_with_cache(
            ImageTranslateRequest {
                image_bytes: &page,
                image_mime: "image/png",
                output_mime: "image/png",
                ocr_languages,
                allow_empty: true,
                debug: debug_page,
            },
            &mut cache,
            translator,
            options,
        )
        .await?;
        translated_images.push(output);
    }

    let pdf = images_to_pdf(&translated_images)?;
    Ok(cache.finish(data::PDF_MIME.to_string(), pdf))
}

fn render_pdf_pages(pdf_bytes: &[u8]) -> Result<Vec<Vec<u8>>> {
    let dir = tempdir().with_context(|| "failed to create temp dir for pdf")?;
    let input_path = dir.path().join("input.pdf");
    fs::write(&input_path, pdf_bytes).with_context(|| "failed to write temp pdf")?;

    if command_exists("mutool") {
        let output = Command::new("mutool")
            .arg("draw")
            .arg("-r")
            .arg("200")
            .arg("-o")
            .arg(dir.path().join("page-%03d.png"))
            .arg(&input_path)
            .output()
            .with_context(|| "failed to run mutool")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("mutool failed: {}", stderr.trim()));
        }
    } else if command_exists("pdftoppm") {
        let output = Command::new("pdftoppm")
            .arg("-png")
            .arg("-r")
            .arg("200")
            .arg(&input_path)
            .arg(dir.path().join("page"))
            .output()
            .with_context(|| "failed to run pdftoppm")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("pdftoppm failed: {}", stderr.trim()));
        }
    } else {
        return Err(anyhow!(
            "pdf rendering requires mutool or pdftoppm (install mupdf or poppler)"
        ));
    }

    let mut pages = Vec::new();
    let mut entries: Vec<_> = fs::read_dir(dir.path())
        .with_context(|| "failed to read temp pdf directory")?
        .filter_map(|entry| entry.ok())
        .collect();
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.starts_with("page-") || name.starts_with("page"))
            .unwrap_or(false)
            && path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("png"))
                .unwrap_or(false)
        {
            let bytes = fs::read(&path).with_context(|| "failed to read rendered pdf page")?;
            pages.push(bytes);
        }
    }
    Ok(pages)
}

fn images_to_pdf(pages: &[Vec<u8>]) -> Result<Vec<u8>> {
    use printpdf::{Image, ImageTransform, Mm, PdfDocument};

    let mut doc = None;
    let mut layers = Vec::new();

    for (idx, bytes) in pages.iter().enumerate() {
        let image = printpdf::image_crate::load_from_memory(bytes)
            .with_context(|| "failed to decode rendered pdf page")?;
        let width = image.width();
        let height = image.height();
        let width_mm = px_to_mm(width);
        let height_mm = px_to_mm(height);

        if idx == 0 {
            let (doc_handle, page, layer) =
                PdfDocument::new("translated", Mm(width_mm), Mm(height_mm), "Layer 1");
            doc = Some(doc_handle);
            layers.push((page, layer, image));
        } else if let Some(doc_handle) = doc.as_mut() {
            let (page, layer) =
                doc_handle.add_page(Mm(width_mm), Mm(height_mm), format!("Layer {}", idx + 1));
            layers.push((page, layer, image));
        }
    }

    let doc = doc.ok_or_else(|| anyhow!("no pages to render"))?;
    for (page, layer, image) in layers.into_iter() {
        let current_layer = doc.get_page(page).get_layer(layer);
        let pdf_image = Image::from_dynamic_image(&image);
        let transform = ImageTransform {
            translate_x: Some(Mm(0.0)),
            translate_y: Some(Mm(0.0)),
            rotate: None,
            scale_x: Some(1.0),
            scale_y: Some(1.0),
            dpi: Some(72.0),
        };
        pdf_image.add_to_layer(current_layer, transform);
    }

    let mut buffer = Vec::new();
    {
        let mut writer = std::io::BufWriter::new(&mut buffer);
        doc.save(&mut writer)
            .with_context(|| "failed to write pdf")?;
    }
    Ok(buffer)
}

fn px_to_mm(px: u32) -> f32 {
    let inches = px as f32 / 72.0;
    inches * 25.4
}
