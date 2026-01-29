use std::io::{self, IsTerminal, Read};

use anyhow::{anyhow, Result};
use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "llm-translator-rust",
    version,
    about = "Translate text using LLM tool calls"
)]
struct Cli {
    /// Target language (default: en)
    #[arg(short = 'l', long = "lang", default_value = "en")]
    lang: String,

    /// Model name or provider:model (e.g. openai:MODEL_ID)
    #[arg(short = 'm', long = "model")]
    model: Option<String>,

    /// API key (overrides environment variables)
    #[arg(short = 'k', long = "key")]
    key: Option<String>,

    /// Formality/style key (from settings [formally])
    #[arg(short = 'f', long = "formal", default_value = "formal")]
    formal: String,

    /// Source language (ISO 639-1/2/3). Use "auto" to detect.
    #[arg(short = 'L', long = "source-lang", default_value = "auto")]
    source_lang: String,

    /// Enable slang keywords in the output
    #[arg(short = 's', long = "slang")]
    slang: bool,

    /// File to translate (image/doc/docx/pptx/xlsx/pdf/txt)
    #[arg(short = 'd', long = "data")]
    data: Option<String>,

    /// Mime type for --data (auto, image/*, pdf, doc, docx, docs, pptx, xlsx, txt)
    #[arg(short = 'M', long = "data-mime")]
    data_mime: Option<String>,

    /// Show enabled translation languages and exit
    #[arg(long = "show-enabled-languages")]
    show_enabled_languages: bool,

    /// Show enabled style keys and exit
    #[arg(long = "show-enabled-styles")]
    show_enabled_styles: bool,

    /// Show cached model list (provider:model per line) and exit
    #[arg(long = "show-models-list")]
    show_models_list: bool,

    /// Show dictionary info (part of speech/inflections) for the input
    #[arg(long = "pos")]
    pos: bool,

    /// Show translation histories and exit
    #[arg(long = "show-histories")]
    show_histories: bool,

    /// Append token usage to output
    #[arg(long = "with-using-tokens")]
    with_using_tokens: bool,

    /// Append model name to output
    #[arg(long = "with-using-model")]
    with_using_model: bool,

    /// Read extra settings from a local TOML file
    #[arg(short = 'r', long = "read-settings")]
    read_settings: Option<String>,

    /// Output OCR debug overlays for attachments
    #[arg(long = "debug-ocr")]
    debug_ocr: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let needs_input = !(cli.show_enabled_languages
        || cli.show_enabled_styles
        || cli.show_models_list
        || cli.show_histories);
    let stdin_bytes = if needs_input {
        if cli.data.is_some() && io::stdin().is_terminal() {
            None
        } else {
            let mut buffer = Vec::new();
            io::stdin().read_to_end(&mut buffer)?;
            Some(buffer)
        }
    } else {
        None
    };

    let mut input = None;
    let mut stdin_attachment = None;
    if let Some(bytes) = stdin_bytes {
        if cli.data.is_some() {
            if !bytes.is_empty() {
                let text = String::from_utf8(bytes).map_err(|_| {
                    anyhow!("stdin must be UTF-8 text when using --data (binary detected)")
                })?;
                input = Some(text);
            }
        } else if !bytes.is_empty() {
            let mime_hint = cli.data_mime.as_deref();
            let mime_forced = mime_hint
                .map(|value| !value.trim().eq_ignore_ascii_case("auto"))
                .unwrap_or(false);
            if mime_forced {
                stdin_attachment = Some(llm_translator_rust::data::load_attachment_from_bytes(
                    bytes, mime_hint, None,
                )?);
            } else if let Ok(text) = std::str::from_utf8(&bytes) {
                if let Some(mime) = llm_translator_rust::data::sniff_mime(&bytes) {
                    if mime != llm_translator_rust::data::TEXT_MIME {
                        stdin_attachment =
                            Some(llm_translator_rust::data::load_attachment_from_bytes(
                                bytes,
                                Some("auto"),
                                None,
                            )?);
                    } else {
                        input = Some(text.to_string());
                    }
                } else {
                    input = Some(text.to_string());
                }
            } else {
                stdin_attachment = Some(
                    llm_translator_rust::data::load_attachment_from_bytes(
                        bytes,
                        Some("auto"),
                        None,
                    )
                    .map_err(|err| anyhow!("stdin appears to be binary; {}", err))?,
                );
            }
        }
    }

    let output = llm_translator_rust::run(
        llm_translator_rust::Config {
            lang: cli.lang,
            model: cli.model,
            key: cli.key,
            formal: cli.formal,
            source_lang: cli.source_lang,
            slang: cli.slang,
            data: cli.data,
            data_mime: cli.data_mime,
            data_attachment: stdin_attachment,
            settings_path: cli.read_settings,
            show_enabled_languages: cli.show_enabled_languages,
            show_enabled_styles: cli.show_enabled_styles,
            show_models_list: cli.show_models_list,
            pos: cli.pos,
            show_histories: cli.show_histories,
            with_using_tokens: cli.with_using_tokens,
            with_using_model: cli.with_using_model,
            debug_ocr: cli.debug_ocr,
        },
        input,
    )
    .await?;

    println!("{}", output);
    Ok(())
}
