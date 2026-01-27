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
```

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
  - `$HOME/.cache/llm-translator-rust` (fallback: `./.cache/llm-translator-rust`)
- `--show-models-list` prints the cached list as `provider:model` per line.

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

[formally]
casual = "Use casual, natural everyday speech."
formal = "Use polite, formal register suitable for professional contexts."
...
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
| `-c` | `--source-lang` | Source language (ISO 639-1/2/3 or `auto`) | `auto` |
|  | `--countery-language` | Alias for `--source-lang` |  |
| `-s` | `--slang` | Include slang keywords when appropriate | `false` |
|  | `--show-enabled-languages` | Show enabled translation languages |  |
|  | `--show-enabled-styles` | Show enabled style keys |  |
|  | `--show-models-list` | Show cached model list (provider:model) |  |
|  | `--with-using-tokens` | Append token usage to output |  |
|  | `--with-using-model` | Append model name to output |  |
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
