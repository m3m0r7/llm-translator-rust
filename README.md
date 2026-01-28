# llm-translator-rust

English | [日本語](README.ja.md)

A small CLI translator that uses LLM tool calls and always reads from stdin.

## Contents

- [Overview](#overview)
- [Installation](#installation)
- [Quickstart](#quickstart)
- [Alias (`t`) for CLI usage](#alias-t-for-cli-usage)
- [Usage](#usage)
- [Model selection & cache](#model-selection--cache)
- [Settings](#settings)
- [Language Packs](#language-packs)
- [Environment variables](#environment-variables)
- [Options](#options)
- [Notes](#notes)

## Overview

- Reads input from stdin and prints the translated text.
- Uses tool-calling JSON only (no free-form output).
- Providers: OpenAI, Gemini, Claude.
- Model list is fetched via each provider's Models API and cached for 24 hours.

## Installation

Choose one of the following:

### 1) Download from GitHub Releases

Release artifacts are available on the Releases page:
[GitHub Releases](https://github.com/m3m0r7/llm-translator-rust/releases/latest)

Each asset is named `llm-translator-rust-<os>-<arch>` (e.g. `llm-translator-rust-macos-aarch64`).

### 2) Install with cargo (global)

```bash
cargo install --git https://github.com/m3m0r7/llm-translator-rust --locked
```

### 3) Build from source (git clone)

```bash
git clone https://github.com/m3m0r7/llm-translator-rust
cd llm-translator-rust
make install
```

Binary will be available at:

```
./target/release/llm-translator-rust
```
Notes:
- macOS/Linux default: `/usr/local/bin` (use `sudo make install` if needed)
- Windows (MSYS/Git Bash): `%USERPROFILE%/.cargo/bin`
- `make install` also copies `settings.toml` to `~/.llm-translator-rust/settings.toml` if it does not exist.

## Quickstart

```bash
export OPENAI_API_KEY="..."
./target/release/llm-translator-rust <<< "ねこ"
```

## Alias (`t`) for CLI usage

```bash
alias t="/path/to/llm-translator-rust/target/release/llm-translator-rust"

echo ねこ | t
```

## Usage

```bash
echo Cat | llm-translator-rust

echo Cat | llm-translator-rust -l en

echo Cat | llm-translator-rust --source-lang en -l ja

# Output examples
echo Cat | llm-translator-rust
# Cat

echo Cat | llm-translator-rust -l en
# Cat

echo Cat | llm-translator-rust -l kor
# 고양이

echo Cat | llm-translator-rust -l zho-hans
# 猫

echo Cat | llm-translator-rust -l zho-hant
# 貓

echo Cat | llm-translator-rust -l ja --formal academic
# 猫

echo Awesome | llm-translator-rust -l ja --slang
# ヤバい

# File translation
cat foobar.txt | llm-translator-rust -l en

# File attachment translation (image/doc/docx/pptx/xlsx/pdf/txt)
llm-translator-rust --data ./slides.pptx --data-mime pptx -l en
llm-translator-rust --data ./scan.png -l ja

# Attachment via stdin (auto-detect or with --data-mime)
cat ./scan.png | llm-translator-rust -l ja
cat ./report.pdf | llm-translator-rust --data-mime pdf -l en

# Image/PDF attachments are re-rendered with numbered overlays (path is returned).
# The image height is extended and a footer list is added:
# (N) original (reading): translated
# - reading is a Latin-script pronunciation for non-Latin text (e.g., romaji/pinyin).
# - identical translations share the same number.
# When using --data with a file path, a sibling file named *_translated.<ext> is also written.
```

## Image translation example

Original:

![Original image](docs/image.png)

Translated:

![Translated image](docs/image_translated.png)

## Model selection & cache

- Default provider priority: OpenAI → Gemini → Claude (first API key found).
- `-m/--model` accepts:
  - Provider only: `openai`, `gemini`, `claude` (uses provider defaults below, if available)
  - Provider + model: `openai:MODEL_ID`
  - When specifying a model, always include the provider prefix.
- Defaults use provider defaults below; if unavailable, the first chat-capable model is used.
- Source/target languages use ISO 639-1 or ISO 639-2/3 codes (e.g., `ja`, `en`, `jpn`, `eng`). Source can be `auto`.
- For Chinese variants, use `zho-hans` (Simplified) or `zho-hant` (Traditional).
- Language validation uses the ISO 639 list from Wikipedia: https://en.wikipedia.org/wiki/List_of_ISO_639_language_codes
- Model list is fetched from each provider’s Models API and cached for 24 hours.
- Cache path:
  - `~/.llm-translator/.cache/meta.json` (fallback: `./.llm-translator/.cache/meta.json`)
- `--show-models-list` prints the cached list as `provider:model` per line.
- When `--model` is omitted, `lastUsingModel` in `meta.json` is preferred (falls back to default resolution if missing or invalid).
- Histories are stored in `meta.json`. Dest files are written to `~/.llm-translator-rust/.cache/dest/<md5>`.
- Image/PDF attachments use OCR (tesseract), normalize OCR text with LLMs, and re-render a numbered overlay plus a footer list.
- Office files (docx/xlsx/pptx) are rewritten by translating text nodes in the XML.
- Output mime matches the input mime (e.g. png stays png, pdf stays pdf).
- OCR languages are inferred from `--source-lang` and `--lang`.
- Use `tesseract --list-langs` to see installed OCR language codes.
- PDF OCR requires a PDF renderer (`mutool` or `pdftoppm` from poppler).
- PDF output is rasterized (text is no longer selectable).

Provider defaults:
- OpenAI: `openai:gpt-5.1`
- Gemini: `gemini:gemini-2.5-flash`
- Claude: `claude:claude-sonnet-4-5-20250929`

Provider model APIs:
- [OpenAI Models API](https://platform.openai.com/docs/api-reference/models)
- [Gemini Models API](https://ai.google.dev/api/models)
- [Anthropic Models API](https://docs.anthropic.com/en/api/models)

## Settings

Settings files are loaded with the following precedence (highest first):

1. `~/.llm-translator-rust/settings.local.toml`
2. `~/.llm-translator-rust/settings.toml`
3. `./settings.local.toml`
4. `./settings.toml`

You can also pass `-r/--read-settings` to load an additional local TOML file (highest priority).

`settings.toml` uses the following format:
`system.languages` should be ISO 639-3 codes.

```toml
[system]
languages = ["jpn", "eng", ...]
histories = 10

[formally]
casual = "Use casual, natural everyday speech."
formal = "Use polite, formal register suitable for professional contexts."
...

[ocr]
text_color = "#c40000"
stroke_color = "#c40000"
fill_color = "#ffffff"
normalize = true
# font_size = 18
# font_family = "Hiragino Sans"
# font_path = "/System/Library/Fonts/Hiragino Sans W3.ttc"
```

## Language Packs

Language packs live in `src/languages/<iso-639-3>.toml`. The first entry in `system.languages` is used for label display in `--show-enabled-languages`.

Example (Japanese):

```toml
[translate.iso_country_lang.jpn]
jpn = "日本語"
eng = "英語"

```

## Environment variables

- OpenAI: `OPENAI_API_KEY`
- Gemini: `GEMINI_API_KEY` or `GOOGLE_API_KEY`
- Claude: `ANTHROPIC_API_KEY`

`-k/--key` overrides environment variables.

## Options

| Flag | Long | Description | Default |
| --- | --- | --- | --- |
| `-l` | `--lang` | Target language | `en` |
| `-m` | `--model` | Provider/model selector | (auto) |
| `-k` | `--key` | API key override | (env) |
| `-f` | `--formal` | Formality key (from `settings.toml` `[formally]`) | `formal` |
| `-L` | `--source-lang` | Source language (ISO 639-1/2/3 or `auto`) | `auto` |
| `-s` | `--slang` | Include slang keywords when appropriate | `false` |
| `-d` | `--data` | File attachment (image/doc/docx/pptx/xlsx/pdf/txt) |  |
| `-M` | `--data-mime` | Mime type for `--data` (or stdin) (`auto`, `image/*`, `pdf`, `doc`, `docx`, `docs`, `pptx`, `xlsx`, `txt`, `png`, `jpeg`, `gif`) | `auto` |
|  | `--show-enabled-languages` | Show enabled translation languages |  |
|  | `--show-enabled-styles` | Show enabled style keys |  |
|  | `--show-models-list` | Show cached model list (provider:model) |  |
|  | `--show-histories` | Show translation histories |  |
|  | `--with-using-tokens` | Append token usage to output |  |
|  | `--with-using-model` | Append model name to output |  |
|  | `--debug-ocr` | Output OCR debug overlays/JSON for attachments |  |
| `-r` | `--read-settings` | Read extra settings TOML file |  |
| `-h` | `--help` | Show help |  |

## Notes

- API errors (including insufficient quota) are surfaced with provider error messages.
- Use `-h/--help` to see the latest options.

## Formality values (default settings)

- `casual`: casual everyday tone
- `formal`: polite formal tone
- `loose`: relaxed, loose phrasing
- `academic`: academic, paper-like phrasing
- `gal`: playful gyaru/gal tone
- `yankee`: rough delinquent style
- `otaku`: otaku-friendly diction and nuance
- `elderly`: gentle, elder register
- `aristocrat`: refined aristocratic tone
- `samurai`: archaic samurai-style wording
- `braille`: output in Unicode Braille patterns
- `morse`: output in International Morse code
- `engineer`: precise technical tone
