use std::io::{self, BufRead, IsTerminal, Read};

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

    /// File to translate (image/doc/docx/pptx/xlsx/pdf/txt/audio)
    #[arg(short = 'd', long = "data")]
    data: Option<String>,

    /// Mime type for --data (auto, image/*, pdf, doc, docx, docs, pptx, xlsx, txt, mp3, wav, m4a, flac, ogg)
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

    /// Show available whisper model names and exit
    #[arg(long = "show-whisper-models")]
    show_whisper_models: bool,

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

    /// Enable verbose logging
    #[arg(long = "verbose")]
    verbose: bool,

    /// Interactive mode
    #[arg(short = 'i', long = "interactive")]
    interactive: bool,

    /// Whisper model name or path (audio transcription)
    #[arg(long = "whisper-model")]
    whisper_model: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    llm_translator_rust::logging::init(cli.verbose)?;
    if cli.interactive {
        return run_interactive(cli).await;
    }

    let needs_input = !(cli.show_enabled_languages
        || cli.show_enabled_styles
        || cli.show_models_list
        || cli.show_whisper_models
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
            show_whisper_models: cli.show_whisper_models,
            pos: cli.pos,
            show_histories: cli.show_histories,
            with_using_tokens: cli.with_using_tokens,
            with_using_model: cli.with_using_model,
            debug_ocr: cli.debug_ocr,
            verbose: cli.verbose,
            whisper_model: cli.whisper_model,
        },
        input,
    )
    .await?;

    println!("{}", output);
    Ok(())
}

struct InteractiveState {
    config: llm_translator_rust::Config,
}

impl InteractiveState {
    fn new(cli: &Cli) -> Self {
        Self {
            config: llm_translator_rust::Config {
                lang: cli.lang.clone(),
                model: cli.model.clone(),
                key: cli.key.clone(),
                formal: cli.formal.clone(),
                source_lang: cli.source_lang.clone(),
                slang: cli.slang,
                data: cli.data.clone(),
                data_mime: cli.data_mime.clone(),
                data_attachment: None,
                settings_path: cli.read_settings.clone(),
                show_enabled_languages: false,
                show_enabled_styles: false,
                show_models_list: false,
                show_whisper_models: false,
                pos: cli.pos,
                show_histories: false,
                with_using_tokens: cli.with_using_tokens,
                with_using_model: cli.with_using_model,
                debug_ocr: cli.debug_ocr,
                verbose: cli.verbose,
                whisper_model: cli.whisper_model.clone(),
            },
        }
    }

    fn config_for_run(&self) -> llm_translator_rust::Config {
        let mut config = self.config.clone();
        config.show_enabled_languages = false;
        config.show_enabled_styles = false;
        config.show_models_list = false;
        config.show_whisper_models = false;
        config.show_histories = false;
        config
    }
}

async fn run_interactive(cli: Cli) -> Result<()> {
    use std::io::Write;

    let mut state = InteractiveState::new(&cli);
    println!("Interactive mode. Use /quit or /exit to finish.");
    println!("Type /help to see available commands.");

    let mut line = String::new();
    let stdin = io::stdin();
    let mut stdin_lock = stdin.lock();
    loop {
        line.clear();
        print!("> ");
        io::stdout().flush()?;
        if stdin_lock.read_line(&mut line)? == 0 {
            break;
        }
        let input = line.trim();
        if input.is_empty() {
            continue;
        }
        if input.starts_with('/') {
            if handle_interactive_command(input, &mut state).await? {
                break;
            }
            continue;
        }

        let output =
            llm_translator_rust::run(state.config_for_run(), Some(input.to_string())).await?;
        println!("{}", output);
    }
    Ok(())
}

async fn handle_interactive_command(input: &str, state: &mut InteractiveState) -> Result<bool> {
    let trimmed = input.trim();
    if matches!(trimmed, "/quit" | "/exit") {
        return Ok(true);
    }
    if trimmed == "/help" {
        print_interactive_help();
        return Ok(false);
    }
    if trimmed == "/show-models-list" {
        let mut config = state.config_for_run();
        config.show_models_list = true;
        let output = llm_translator_rust::run(config, None).await?;
        println!("{}", output);
        return Ok(false);
    }
    if trimmed == "/show-whisper-models" {
        let mut config = state.config_for_run();
        config.show_whisper_models = true;
        let output = llm_translator_rust::run(config, None).await?;
        println!("{}", output);
        return Ok(false);
    }
    if trimmed == "/show-histories" {
        let mut config = state.config_for_run();
        config.show_histories = true;
        let output = llm_translator_rust::run(config, None).await?;
        println!("{}", output);
        return Ok(false);
    }
    if trimmed == "/show-enabled-languages" {
        let mut config = state.config_for_run();
        config.show_enabled_languages = true;
        let output = llm_translator_rust::run(config, None).await?;
        println!("{}", output);
        return Ok(false);
    }
    if trimmed == "/show-enabled-styles" {
        let mut config = state.config_for_run();
        config.show_enabled_styles = true;
        let output = llm_translator_rust::run(config, None).await?;
        println!("{}", output);
        return Ok(false);
    }
    if trimmed == "/run" {
        let output = llm_translator_rust::run(state.config_for_run(), Some(String::new())).await?;
        println!("{}", output);
        return Ok(false);
    }

    if let Some(arg) = trimmed.strip_prefix("/model") {
        let value = arg.trim();
        if value.is_empty() {
            println!(
                "model: {}",
                state.config.model.as_deref().unwrap_or("(auto)")
            );
        } else {
            state.config.model = Some(value.to_string());
            println!("model set to {}", value);
        }
        return Ok(false);
    }
    if let Some(arg) = trimmed.strip_prefix("/whisper-model") {
        let value = arg.trim();
        if value.is_empty() {
            println!(
                "whisper-model: {}",
                state.config.whisper_model.as_deref().unwrap_or("(auto)")
            );
        } else {
            state.config.whisper_model = Some(value.to_string());
            println!("whisper-model set to {}", value);
        }
        return Ok(false);
    }
    if let Some(arg) = trimmed.strip_prefix("/lang") {
        let value = arg.trim();
        if value.is_empty() {
            println!("lang: {}", state.config.lang);
        } else {
            state.config.lang = value.to_string();
            println!("lang set to {}", value);
        }
        return Ok(false);
    }
    if let Some(arg) = trimmed.strip_prefix("/source-lang") {
        let value = arg.trim();
        if value.is_empty() {
            println!("source-lang: {}", state.config.source_lang);
        } else {
            state.config.source_lang = value.to_string();
            println!("source-lang set to {}", value);
        }
        return Ok(false);
    }
    if let Some(arg) = trimmed.strip_prefix("/formal") {
        let value = arg.trim();
        if value.is_empty() {
            println!("formal: {}", state.config.formal);
        } else {
            state.config.formal = value.to_string();
            println!("formal set to {}", value);
        }
        return Ok(false);
    }
    if let Some(arg) = trimmed.strip_prefix("/slang") {
        state.config.slang = parse_toggle(arg, state.config.slang)?;
        println!("slang: {}", state.config.slang);
        return Ok(false);
    }
    if let Some(arg) = trimmed.strip_prefix("/pos") {
        state.config.pos = parse_toggle(arg, state.config.pos)?;
        println!("pos: {}", state.config.pos);
        return Ok(false);
    }
    if let Some(arg) = trimmed.strip_prefix("/with-using-model") {
        state.config.with_using_model = parse_toggle(arg, state.config.with_using_model)?;
        println!("with-using-model: {}", state.config.with_using_model);
        return Ok(false);
    }
    if let Some(arg) = trimmed.strip_prefix("/with-using-tokens") {
        state.config.with_using_tokens = parse_toggle(arg, state.config.with_using_tokens)?;
        println!("with-using-tokens: {}", state.config.with_using_tokens);
        return Ok(false);
    }
    if let Some(arg) = trimmed.strip_prefix("/data") {
        let value = arg.trim();
        if value.is_empty() {
            println!("data: {}", state.config.data.as_deref().unwrap_or("(none)"));
        } else if value == "clear" {
            state.config.data = None;
            println!("data cleared");
        } else {
            state.config.data = Some(value.to_string());
            println!("data set to {}", value);
        }
        return Ok(false);
    }
    if let Some(arg) = trimmed.strip_prefix("/data-mime") {
        let value = arg.trim();
        if value.is_empty() {
            println!(
                "data-mime: {}",
                state.config.data_mime.as_deref().unwrap_or("(auto)")
            );
        } else if value == "clear" {
            state.config.data_mime = None;
            println!("data-mime cleared");
        } else {
            state.config.data_mime = Some(value.to_string());
            println!("data-mime set to {}", value);
        }
        return Ok(false);
    }
    if let Some(arg) = trimmed.strip_prefix("/key") {
        let value = arg.trim();
        if value.is_empty() {
            println!(
                "key: {}",
                state
                    .config
                    .key
                    .as_deref()
                    .map(|_| "(set)")
                    .unwrap_or("(none)")
            );
        } else {
            state.config.key = Some(value.to_string());
            println!("key set");
        }
        return Ok(false);
    }

    eprintln!("unknown command: {}", trimmed);
    Ok(false)
}

fn parse_toggle(arg: &str, current: bool) -> Result<bool> {
    let value = arg.trim();
    if value.is_empty() {
        return Ok(!current);
    }
    match value.to_lowercase().as_str() {
        "on" | "true" | "1" => Ok(true),
        "off" | "false" | "0" => Ok(false),
        _ => Err(anyhow!("expected on/off/true/false/1/0")),
    }
}

fn print_interactive_help() {
    println!("Commands:");
    println!("  /quit, /exit                 Exit interactive mode");
    println!("  /show-models-list            Show cached models");
    println!("  /show-whisper-models          Show whisper model names");
    println!("  /show-histories              Show translation histories");
    println!("  /show-enabled-languages      Show enabled languages");
    println!("  /show-enabled-styles         Show enabled styles");
    println!("  /run                         Run translation with empty input");
    println!("  /model <provider:model>      Set model (or show current)");
    println!("  /whisper-model <name|path>   Set whisper model (or show current)");
    println!("  /lang <code>                 Set target language");
    println!("  /source-lang <code>          Set source language");
    println!("  /formal <key>                Set formality key");
    println!("  /slang [on|off]              Toggle slang");
    println!("  /pos [on|off]                Toggle dictionary mode");
    println!("  /with-using-model [on|off]   Toggle model suffix output");
    println!("  /with-using-tokens [on|off]  Toggle token usage output");
    println!("  /data <path|clear>           Set attachment file");
    println!("  /data-mime <mime|clear>      Set attachment mime");
    println!("  /key <api-key>               Set API key");
}
