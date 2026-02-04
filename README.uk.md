# llm-translator-rust

UK English | [English](README.md) | [日本語](README.ja.md) | [中文](README.cn.md) | [Français](README.fr.md) | [Deutsch](README.ge.md) | [Italiano](README.it.md) | [한국어](README.kr.md) | [Русский](README.ru.md)

A proper little CLI translator that leans on LLM tool‑calls and always reads from stdin — no faff, no fluff.

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
- [Server mode](#server-mode)
- [MCP mode](#mcp-mode)
- [FFI (C ABI)](#ffi-c-abi)
- [Notes](#notes)

## Overview

- Reads input from stdin and spits out the translation, clean and tidy.
- Tool‑calling JSON only — no free‑form waffle.
- Providers: OpenAI, Gemini, Claude.
- Model list is fetched via each provider’s Models API and cached for 24 hours.

## Installation

Pick your poison:

### 1) Download from GitHub Releases

Release artefacts live here:
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
make
sudo make install
```

Binary ends up at:

```
./target/release/llm-translator-rust
```

Notes:
- macOS/Linux default: `$XDG_RUNTIME_DIR` if set, otherwise `/usr/local/bin` (use `sudo make install` if needed)
- Windows (MSYS/Git Bash): `%USERPROFILE%/.cargo/bin`
- make writes build/build_env.toml and embeds it into the binary (portable builds don't need the file at runtime).
- You can override paths via env vars passed to `make`, e.g.
  `DATA_DIRECTORY=$XDG_DATA_HOME/llm-translator-rust CONFIG_DIRECTORY=$XDG_CONFIG_HOME/llm-translator-rust RUNTIME_DIRECTORY=$XDG_RUNTIME_DIR BIN_DIRECTORY=target/release BUILD_ENV_PATH=build/build_env.toml make`
- `make install` copies `settings.toml` to the settings path (XDG config by default) if it does not exist.
- `make install` copies headers from `ext/` into `$XDG_DATA_HOME/llm-translator-rust` if they do not exist.

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

# Dictionary (part of speech/inflections)
echo 猫 | llm-translator-rust --pos -l en
echo play | llm-translator-rust --pos noun,verb -l en

# File translation
cat foobar.txt | llm-translator-rust -l en

# File attachment translation (image/doc/docx/pptx/xlsx/pdf/txt/md/html/json/yaml/po/xml/js/ts/tsx/mermaid/audio)
llm-translator-rust --data ./slides.pptx --data-mime pptx -l en
llm-translator-rust --data ./scan.png -l ja
llm-translator-rust --data ./voice.mp3 -l en

# Attachment via stdin (auto-detect or with --data-mime)
cat ./scan.png | llm-translator-rust -l ja
cat ./report.pdf | llm-translator-rust --data-mime pdf -l en

# Image/PDF attachments are re-rendered with numbered overlays (path is returned).
# The image height is extended and a footer list is added:
# (N) original (reading): translated
# - reading is a Latin-script pronunciation for non-Latin text (e.g., romaji/pinyin).
# - identical translations share the same number.
# When using --data with a file path (and without --overwrite), a sibling file is written.
# The suffix comes from settings.toml [system].translated_suffix (default: _translated).
# When --data points to a directory, a sibling output directory is created with the same suffix.
```

## Directory translation

When `--data` points to a directory, the CLI walks it recursively and translates each supported file.
The relative directory structure is preserved in the output directory.

```bash
llm-translator-rust --data ./docs -l ja
# Output: ./docs_translated (default suffix; configurable via settings.toml)
```

Notes:
- `--data-mime` applies to every file in the directory; leave it as `auto` for mixed file types.
- Files that can’t be read or whose mime can’t be detected are reported as failures; unsupported types are skipped.
- Use `--force` to treat unknown/low‑confidence detections as text.
- Directory translation runs concurrently (default 3 threads). Use `--directory-translation-threads` or `settings.toml`.
- Exclude files with `--ignore-translation-file` or an ignore file (default: `.llm-translation-rust-ignore`).
  Patterns follow `.gitignore` rules (`*`, `**`, `!`, comments).
- Ignore rules apply only when `--data` is a directory.
- Use `--out` to choose the output directory.
- If a directory translation fails, the original file is copied to the output directory.

## Overwrite mode (--overwrite)

`--overwrite` writes results in place for files or directories passed via `--data`.
Each file is backed up to `$XDG_DATA_HOME/llm-translator-rust/backup` before writing.
Retention is controlled by `settings.toml` `[system].backup_ttl_days` (default: 30).

```bash
llm-translator-rust --data ./docs --overwrite -l ja
llm-translator-rust --data ./slide.pdf --overwrite -l en
```

## Output path (--out)

`--out` sets the output path for file or directory translations.
It can’t be used with `--overwrite`.

```bash
llm-translator-rust --data ./docs -l ja --out ./outdir
llm-translator-rust --data ./slide.pdf -l en --out ./translated.pdf
```

## Dictionary (--pos)

`--pos` повертає словникові відомості для введеного терміна. Можна фільтрувати за частиною мови (через кому); англійські назви POS приймаються й за можливості зіставляються з мовою джерела.

Usage:

```
echo 猫 | llm-translator-rust --pos -l en
echo play | llm-translator-rust --pos noun,verb -l en
```

Example output (labels follow the source language):

```
訳語: cat
読み: キャット
品詞: 名詞
属性: 動物, ペット
別訳: kitty (キティ), tomcat (トムキャット), feline (フィーライン)

複数形: cats
三人称単数: cats
過去形: -
現在分詞: -

用法: 一般的な猫を指す最も基本的な言葉。ペットや動物全般として広く使われる。
用例:
- I have a cat. (私は猫を飼っています。)
- The black cat is sleeping. (黒い猫が眠っています。)
- Many people love cats. (多くの人が猫を愛しています。)
```

## Correction (--correction)

`--correction` proofreads the input and calls out corrections in the source language.

Usage:

```
echo "This is pen" | llm-translator-rust --correction --source-lang en
```

Example output:

```
This is a pen
        -

Correction reasons:
- English requires a/an before a countable noun
```

- Labels are localised to the source language.
- `Reading` is the translation’s pronunciation in the source language’s usual script.
- `Alternatives` lists other plausible translations with readings.
- `Usage` and example source sentences are in the source language.
- Examples include the translation or one of the alternatives.

## Audio translation

Audio files are transcribed with `whisper-rs`, translated by the LLM, then re‑synthesised.

- Supported audio: mp3, wav, m4a, flac, ogg
- Requires `ffmpeg`
- Requires a Whisper model (auto‑downloaded on first run)
- TTS uses macOS `say` or Linux `espeak`

Choose a model:

```
llm-translator-rust --show-whisper-models
llm-translator-rust --whisper-model small -d ./voice.mp3 -l en
```

You can also set `LLM_TRANSLATOR_WHISPER_MODEL` to a model name or file path.
`settings.toml` `[whisper] model` or `--whisper-model` takes priority.

## Dependencies

macOS (Homebrew):

```
brew install tesseract ffmpeg
```

Ubuntu/Debian:

```
sudo apt-get install tesseract-ocr ffmpeg espeak
```

Windows (Chocolatey):

```
choco install tesseract ffmpeg
```

## Image translation example

Original:

![Original image](docs/image.png)

Translated:

![Translated image](docs/image_translated.png)

## Model selection & cache

- Default provider priority: OpenAI → Gemini → Claude (first API key found).
- `-m/--model` accepts:
  - Provider only: `openai`, `gemini`, `claude`
  - Provider + model: `openai:MODEL_ID`
  - Always include the provider prefix when specifying a model.
- Defaults use provider defaults below; if unavailable, the first chat‑capable model is used.
- Source/target languages use ISO 639-1 or ISO 639-2/3 codes (e.g., `ja`, `en`, `jpn`, `eng`). Source can be `auto`.
- Chinese variants: `zho-hans` (Simplified) or `zho-hant` (Traditional).
- Language validation uses Wikipedia’s ISO 639 list: https://en.wikipedia.org/wiki/List_of_ISO_639_language_codes
- Model list is fetched from each provider’s Models API and cached for 24 hours.
- Cache path:
  - `~/.llm-translator/.cache/meta.json` (fallback: `./.llm-translator/.cache/meta.json`)
- `--show-models-list` prints the cached list as `provider:model` per line.
- `--show-whisper-models` prints available whisper model names.
- `--pos` returns dictionary‑style details.
- `--correction` returns proofreading corrections and reasons in the source language.
- `--whisper-model` selects the whisper model name or path.
- When `--model` is omitted, `lastUsingModel` in `meta.json` is preferred.
- Histories are stored in `meta.json`. Dest files go to `$XDG_DATA_HOME/llm-translator-rust/.cache/dest/<md5>`.
- Image/PDF attachments use OCR (tesseract), normalise OCR text with LLMs, and re‑render a numbered overlay + footer list.
- Office files (docx/xlsx/pptx) are rewritten by translating text nodes in XML.
- Output mime matches the input mime.
- OCR languages are inferred from `--source-lang` and `--lang`.
- Use `tesseract --list-langs` to see installed OCR language codes.
- PDF OCR requires a PDF renderer (`mutool` or `pdftoppm` from poppler).
- PDF output is rasterised (text is no longer selectable).

Provider defaults:
- OpenAI: `openai:gpt-5.2`
- Gemini: `gemini:gemini-2.5-flash`
- Claude: `claude:claude-sonnet-4-5-20250929`

Provider model APIs:
- [OpenAI Models API](https://platform.openai.com/docs/api-reference/models)
- [Gemini Models API](https://ai.google.dev/api/models)
- [Anthropic Models API](https://docs.anthropic.com/en/api/models)

## Settings

Settings files are loaded in this order (highest priority first):

1. `$XDG_CONFIG_HOME/llm-translator-rust/settings.local.toml (fallback: ~/.config/llm-translator-rust/settings.local.toml)`
2. `$XDG_CONFIG_HOME/llm-translator-rust/settings.toml` (fallback: `~/.config/llm-translator-rust/settings.toml`)
3. `./settings.local.toml`
4. `./settings.toml`

Use `-r/--read-settings` to load an additional local TOML file (highest priority).

`settings.toml` format:
`system.languages` should be ISO 639-3 codes.

```toml
[system]
languages = ["jpn", "eng", ...]
histories = 10
directory_translation_threads = 3
translation_ignore_file = ".llm-translation-rust-ignore"

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

Language packs live in `src/languages/<iso-639-3>.toml`.
The first entry in `system.languages` is used for label display in `--show-enabled-languages`.

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
| `-d` | `--data` | File attachment (image/doc/docx/pptx/xlsx/pdf/txt/md/html/json/yaml/po/xml/js/ts/tsx/mermaid/audio) |  |
| `-M` | `--data-mime` | Mime type for `--data` (or stdin) | `auto` |
|  | `--with-commentout` | Translate comment-out text (HTML/YAML/PO) |  |
|  | `--show-enabled-languages` | Show enabled translation languages |  |
|  | `--show-enabled-styles` | Show enabled style keys |  |
|  | `--show-models-list` | Show cached model list (provider:model) |  |
|  | `--show-whisper-models` | Show available whisper model names |  |
|  | `--pos [noun,verb]` | Dictionary output (part of speech/inflections) |  |
|  | `--correction` | Proofread input text and point out corrections |  |
|  | `--details` | Detailed translations across all formal styles |  |
|  | `--report` | Generate a translation report (html/xml/json) |  |
|  | `--report-out` | Report output path |  |
|  | `--show-histories` | Show translation histories |  |
|  | `--with-using-tokens` | Append token usage to output |  |
|  | `--with-using-model` | Append model name to output |  |
|  | `--force` | Force translation when mime detection is uncertain (treat as text) |  |
|  | `--debug-ocr` | Output OCR debug overlays/JSON for attachments |  |
|  | `--whisper-model` | Whisper model name or path |  |
|  | `--overwrite` | Overwrite input files in place (backups stored in `$XDG_DATA_HOME/llm-translator-rust/backup`) |  |
|  | `--directory-translation-threads` | Directory translation concurrency |  |
|  | `--ignore-translation-file` | Ignore patterns for directory translation (gitignore-like) |  |
| `-o` | `--out` | Output path for translated file or directory |  |
|  | `--verbose` | Verbose logging |  |
| `-i` | `--interactive` | Interactive mode |  |
| `-r` | `--read-settings` | Read extra settings TOML file |  |
|  | `--server` | Start HTTP server (`ADDR` defaults to settings or `0.0.0.0:11223`) |  |
|  | `--mcp` | Start MCP server over stdio |  |
| `-h` | `--help` | Show help |  |

## Server mode

Start the HTTP server:

```bash
llm-translator-rust --server
llm-translator-rust --server 0.0.0.0:11223
```

Server settings are configurable in `settings.toml` under `[server]`:

```toml
[server]
host = "0.0.0.0"
port = 11223
tmp_dir = "/tmp/llm-translator-rust"
```

Requests are JSON `POST /translate` (either `text` or `data` path):

```json
{
  "text": "Hello",
  "lang": "ja"
}
```

```json
{
  "data": "/path/to/file-or-dir",
  "data_mime": "auto",
  "lang": "ja",
  "force_translation": false
}
```

Correction request:

```json
{
  "text": "This is pen",
  "correction": true,
  "source_lang": "en"
}
```

Response (text):

```json
{
  "contents": [
    {
      "mime": "text/plain",
      "format": "raw",
      "original": "Hello",
      "translated": "こんにちは"
    }
  ]
}
```

Correction response (text):

```json
{
  "contents": [
    {
      "mime": "text/plain",
      "format": "raw",
      "original": "This is pen",
      "translated": "This is a pen",
      "correction": {
        "markers": "        -",
        "reasons": ["English requires a/an before a countable noun"],
        "source_language": "en"
      }
    }
  ]
}
```

Response (binary):

```json
{
  "contents": [
    {
      "mime": "image/png",
      "format": "path",
      "translated": "/tmp/llm-translator-rust/llm-translator-xxxx.png"
    }
  ]
}
```

When `data` is a directory, multiple entries are returned in `contents`.

## MCP mode

Start the MCP server over stdio:

```bash
llm-translator-rust --mcp
```

Tools:
- `translate`
- `translate_details`
- `correction`
- `pos`

## FFI (C ABI)

- C header is at `ext/llm_translator_rust.h`.
- Functions return heap strings; free them with `llm_ext_free_string`.
- When a call fails, retrieve a message with `llm_ext_last_error_message`.

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
