use std::io::{self, Read};

use anyhow::Result;
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
    #[arg(
        short = 'c',
        long = "source-lang",
        alias = "countery-language",
        default_value = "auto"
    )]
    source_lang: String,

    /// Enable slang keywords in the output
    #[arg(short = 's', long = "slang")]
    slang: bool,

    /// Show enabled translation languages and exit
    #[arg(long = "show-enabled-languages")]
    show_enabled_languages: bool,

    /// Show enabled style keys and exit
    #[arg(long = "show-enabled-styles")]
    show_enabled_styles: bool,

    /// Show cached model list (provider:model per line) and exit
    #[arg(long = "show-models-list")]
    show_models_list: bool,

    /// Append token usage to output
    #[arg(long = "with-using-tokens")]
    with_using_tokens: bool,

    /// Append model name to output
    #[arg(long = "with-using-model")]
    with_using_model: bool,

    /// Read extra settings from a local TOML file
    #[arg(short = 'r', long = "read-settings")]
    read_settings: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let needs_input =
        !(cli.show_enabled_languages || cli.show_enabled_styles || cli.show_models_list);
    let input = if needs_input {
        let mut buffer = String::new();
        io::stdin().read_to_string(&mut buffer)?;
        Some(buffer)
    } else {
        None
    };

    let output = llm_translator_rust::run(
        llm_translator_rust::Config {
            lang: cli.lang,
            model: cli.model,
            key: cli.key,
            formal: cli.formal,
            source_lang: cli.source_lang,
            slang: cli.slang,
            settings_path: cli.read_settings,
            show_enabled_languages: cli.show_enabled_languages,
            show_enabled_styles: cli.show_enabled_styles,
            show_models_list: cli.show_models_list,
            with_using_tokens: cli.with_using_tokens,
            with_using_model: cli.with_using_model,
        },
        input,
    )
    .await?;

    println!("{}", output);
    Ok(())
}
