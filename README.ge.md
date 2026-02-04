# llm-translator-rust

Deutsch | [Italiano](README.it.md) | [English](README.md) | [日本語](README.ja.md) | [中文](README.cn.md) | [Français](README.fr.md) | [한국어](README.kr.md) | [Русский](README.ru.md) | [UK English](README.uk.md)

Ein kleines CLI‑Übersetzungstool, das LLM‑Tool‑Calls nutzt und immer von stdin liest.

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

- Liest Eingaben von stdin und gibt die Übersetzung aus.
- Verwendet ausschließlich Tool‑Call‑JSON (keine freie Ausgabe).
- Provider: OpenAI, Gemini, Claude.
- Modellliste wird über die Models‑API jedes Providers geholt und 24 h gecacht.

## Installation

Wähle eine der folgenden Optionen:

### 1) Von GitHub Releases herunterladen

Release‑Artefakte gibt es auf der Releases‑Seite:
[GitHub Releases](https://github.com/m3m0r7/llm-translator-rust/releases/latest)

Jedes Asset heißt `llm-translator-rust-<os>-<arch>` (z. B. `llm-translator-rust-macos-aarch64`).

### 2) Mit cargo installieren (global)

```bash
cargo install --git https://github.com/m3m0r7/llm-translator-rust --locked
```

### 3) Aus dem Quellcode bauen (git clone)

```bash
git clone https://github.com/m3m0r7/llm-translator-rust
cd llm-translator-rust
make
sudo make install
```

Binary liegt unter:

```
./target/release/llm-translator-rust
```

Hinweise:
- macOS/Linux: Standard `/usr/local/bin` (bei Bedarf `sudo make install`)
- Windows (MSYS/Git Bash): `%USERPROFILE%/.cargo/bin`
- make writes build_env.toml and embeds it into the binary (portable builds don't need the file at runtime).
- Override paths via env vars passed to make, e.g. BASE_DIRECTORY=~/.llm-translator-rust BIN_DIRECTORY=target/release INSTALL_DIRECTORY=/usr/local/bin SETTINGS_FILE=~/.llm-translator-rust/settings.toml BUILD_ENV_PATH=build_env.toml make
- `make install` copies `settings.toml` to `baseDirectory` if it does not exist.

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

Wenn `--data` auf ein Verzeichnis zeigt, wird dieses rekursiv durchlaufen und jede unterstützte Datei übersetzt.
Die relative Struktur bleibt im Ausgabeverzeichnis erhalten.

```bash
llm-translator-rust --data ./docs -l ja
# Output: ./docs_translated (default suffix; configurable via settings.toml)
```

Hinweise:
- `--data-mime` gilt für alle Dateien im Verzeichnis; für gemischte Typen `auto` verwenden.
- Nicht lesbare Dateien oder Dateien mit nicht erkennbarer MIME werden als Fehler gemeldet; nicht unterstützte werden übersprungen.
- Mit `--force` unbekannte/unsichere Erkennung als Text behandeln.
- Verzeichnisübersetzung läuft parallel (Standard 3 Threads). Änderbar mit `--directory-translation-threads` oder `settings.toml`.
- Ausschlüsse via `--ignore-translation-file` oder Ignore‑Datei (Standard `.llm-translation-rust-ignore`, konfigurierbar in `settings.toml`).
  Muster wie in `.gitignore` (`*`, `**`, `!`, Kommentare).
- Ignore‑Regeln gelten nur, wenn `--data` ein Verzeichnis ist.
- `--out` setzt das Ausgabeverzeichnis.
- Bei Fehlern wird die Originaldatei in die Ausgabe kopiert.

## Overwrite mode (--overwrite)

`--overwrite` schreibt die Ergebnisse direkt zurück in die über `--data` angegebenen Dateien/Verzeichnisse.
Vor dem Schreiben wird jede Datei nach `~/.llm-translator-rust/backup` gesichert.
Aufbewahrung über `settings.toml` `[system].backup_ttl_days` (Standard: 30 Tage).

```bash
llm-translator-rust --data ./docs --overwrite -l ja
llm-translator-rust --data ./slide.pdf --overwrite -l en
```

## Output path (--out)

`--out` legt den Ausgabepfad für Datei‑/Verzeichnisübersetzungen fest.
Nicht mit `--overwrite` kombinierbar.

```bash
llm-translator-rust --data ./docs -l ja --out ./outdir
llm-translator-rust --data ./slide.pdf -l en --out ./translated.pdf
```

## Dictionary (--pos)

`--pos` liefert Wörterbuch‑Details zum Eingabeterm.

Verwendung:

```
echo 猫 | llm-translator-rust --pos -l en
```

Beispielausgabe (Labels in der Quellsprache):

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

`--correction` prüft den Text und nennt Korrekturen in der Quellsprache.

Verwendung:

```
echo "This is pen" | llm-translator-rust --correction --source-lang en
```

Beispielausgabe:

```
This is a pen
        -

Correction reasons:
- English requires a/an before a countable noun
```

- Labels sind auf die Quellsprache lokalisiert.
- `Reading` ist die Aussprache der Übersetzung in der Schrift der Quellsprache.
- `Alternatives` listet alternative Übersetzungen mit Lesungen.
- `Usage` und Beispiel‑Quellsätze sind in der Quellsprache.
- Beispiele enthalten die Übersetzung oder eine Alternative.

## Audio translation

Audio wird mit `whisper-rs` transkribiert, vom LLM übersetzt und anschließend neu synthetisiert.

- Unterstützt: mp3, wav, m4a, flac, ogg
- Benötigt `ffmpeg`
- Benötigt ein Whisper‑Modell (wird beim ersten Lauf geladen)
- TTS nutzt macOS `say` oder Linux `espeak`

Modell wählen:

```
llm-translator-rust --show-whisper-models
llm-translator-rust --whisper-model small -d ./voice.mp3 -l en
```

`LLM_TRANSLATOR_WHISPER_MODEL` kann ebenfalls gesetzt werden (Name oder Pfad).
`settings.toml` `[whisper] model` oder `--whisper-model` überschreibt dies.

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

- Standardpriorität: OpenAI → Gemini → Claude (erste gefundene API‑Key).
- `-m/--model` akzeptiert:
  - Nur Provider: `openai`, `gemini`, `claude`
  - Provider + Modell: `openai:MODEL_ID`
  - Provider‑Präfix immer angeben.
- Standardmäßig Provider‑Default, sonst erstes chat‑fähiges Modell.
- Sprachcodes ISO 639-1 oder ISO 639-2/3 (z. B. `ja`, `en`, `jpn`, `eng`), Quelle kann `auto` sein.
- Chinesische Varianten: `zho-hans` (vereinfacht), `zho-hant` (traditionell).
- Sprachvalidierung via ISO‑639‑Liste von Wikipedia: https://en.wikipedia.org/wiki/List_of_ISO_639_language_codes
- Modellliste wird 24 h gecacht.
- Cache:
  - `~/.llm-translator/.cache/meta.json` (Fallback: `./.llm-translator/.cache/meta.json`)
- `--show-models-list` gibt die Liste `provider:model` aus.
- `--show-whisper-models` zeigt verfügbare Whisper‑Modelle.
- `--pos` liefert Wörterbuchdetails.
- `--correction` liefert Korrekturen und Gründe (Quellsprache).
- `--whisper-model` wählt das Whisper‑Modell.
- Ohne `--model` wird `lastUsingModel` aus `meta.json` bevorzugt.
- Historien liegen in `meta.json`, Ziele in `~/.llm-translator-rust/.cache/dest/<md5>`.
- Bild/PDF nutzt OCR (tesseract) und rendert Overlays + Fußliste neu.
- Office‑Dateien (docx/xlsx/pptx) werden durch Übersetzung der XML‑Textknoten geschrieben.
- Ausgabemime entspricht Eingabemime.
- OCR‑Sprachen aus `--source-lang` und `--lang` abgeleitet.
- `tesseract --list-langs` zeigt installierte OCR‑Sprachen.
- PDF‑OCR benötigt Renderer (`mutool` oder `pdftoppm` aus poppler).
- PDF‑Ausgabe ist gerastert (Text nicht mehr selektierbar).

Provider defaults:
- OpenAI: `openai:gpt-5.2`
- Gemini: `gemini:gemini-2.5-flash`
- Claude: `claude:claude-sonnet-4-5-20250929`

Provider model APIs:
- [OpenAI Models API](https://platform.openai.com/docs/api-reference/models)
- [Gemini Models API](https://ai.google.dev/api/models)
- [Anthropic Models API](https://docs.anthropic.com/en/api/models)

## Settings

Lade-Reihenfolge (höchste Priorität zuerst):

1. `~/.llm-translator-rust/settings.local.toml`
2. `~/.llm-translator-rust/settings.toml`
3. `./settings.local.toml`
4. `./settings.toml`

Zusätzliche Datei mit `-r/--read-settings` (höchste Priorität).

`settings.toml` Format:
`system.languages` sollten ISO 639-3 Codes sein.

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

Sprachpakete liegen in `src/languages/<iso-639-3>.toml`.
Der erste Eintrag in `system.languages` wird für `--show-enabled-languages` genutzt.

Beispiel (Japanisch):

```toml
[translate.iso_country_lang.jpn]
jpn = "日本語"
eng = "英語"

```

## Environment variables

- OpenAI: `OPENAI_API_KEY`
- Gemini: `GEMINI_API_KEY` oder `GOOGLE_API_KEY`
- Claude: `ANTHROPIC_API_KEY`

`-k/--key` überschreibt Umgebungsvariablen.

## Options

| Flag | Long | Description | Default |
| --- | --- | --- | --- |
| `-l` | `--lang` | Zielsprache | `en` |
| `-m` | `--model` | Provider/Model‑Auswahl | (auto) |
| `-k` | `--key` | API‑Key überschreiben | (env) |
| `-f` | `--formal` | Stil (aus `settings.toml` `[formally]`) | `formal` |
| `-L` | `--source-lang` | Quellsprache (ISO 639-1/2/3 oder `auto`) | `auto` |
| `-s` | `--slang` | Slang‑Wörter einbeziehen | `false` |
| `-d` | `--data` | Datei‑Attachment (image/doc/docx/pptx/xlsx/pdf/txt/md/html/json/yaml/po/xml/js/ts/tsx/mermaid/audio) |  |
| `-M` | `--data-mime` | MIME für `--data` (oder stdin) | `auto` |
|  | `--with-commentout` | Kommentare übersetzen (HTML/YAML/PO) |  |
|  | `--show-enabled-languages` | Aktivierte Sprachen anzeigen |  |
|  | `--show-enabled-styles` | Aktivierte Stile anzeigen |  |
|  | `--show-models-list` | Modellliste aus Cache anzeigen |  |
|  | `--show-whisper-models` | Whisper‑Modelle anzeigen |  |
|  | `--pos` | Wörterbuchausgabe (Wortart/Flexionen) |  |
|  | `--correction` | Text korrigieren und Hinweise geben |  |
|  | `--details` | Detailed translations across all formal styles |  |
|  | `--report` | Generate a translation report (html/xml/json) |  |
|  | `--report-out` | Report output path |  |
|  | `--show-histories` | Übersetzungsverläufe anzeigen |  |
|  | `--with-using-tokens` | Token‑Nutzung anhängen |  |
|  | `--with-using-model` | Modellnamen anhängen |  |
|  | `--force` | Unsichere MIME als Text behandeln |  |
|  | `--debug-ocr` | OCR‑Debug‑Overlays/JSON ausgeben |  |
|  | `--whisper-model` | Whisper‑Modellname oder Pfad |  |
|  | `--overwrite` | Dateien überschreiben (Backup `~/.llm-translator-rust/backup`) |  |
|  | `--directory-translation-threads` | Parallelität für Verzeichnisse |  |
|  | `--ignore-translation-file` | Ignore‑Muster (gitignore‑ähnlich) |  |
| `-o` | `--out` | Ausgabepfad |  |
|  | `--verbose` | Ausführliche Logs |  |
| `-i` | `--interactive` | Interaktiver Modus |  |
| `-r` | `--read-settings` | Zusätzliches TOML laden |  |
|  | `--server` | HTTP‑Server starten (Standard `0.0.0.0:11223`) |  |
|  | `--mcp` | Start MCP server over stdio |  |
| `-h` | `--help` | Hilfe |  |

## Server mode

HTTP‑Server starten:

```bash
llm-translator-rust --server
llm-translator-rust --server 0.0.0.0:11223
```

Server‑Einstellungen in `settings.toml` `[server]`:

```toml
[server]
host = "0.0.0.0"
port = 11223
tmp_dir = "/tmp/llm-translator-rust"
```

JSON‑Requests `POST /translate` (`text` oder `data`):

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

Korrektur‑Request:

```json
{
  "text": "This is pen",
  "correction": true,
  "source_lang": "en"
}
```

Antwort (Text):

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

Antwort Korrektur (Text):

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

Antwort (Binär):

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

Wenn `data` ein Verzeichnis ist, gibt `contents` mehrere Einträge zurück.

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

- C‑Header: `ext/llm_translator_rust.h`.
- Funktionen geben Heap‑Strings zurück; mit `llm_ext_free_string` freigeben.
- Fehlertext mit `llm_ext_last_error_message` abrufen.

## Notes

- API‑Fehler (inkl. fehlendes Guthaben) werden vom Provider durchgereicht.
- `-h/--help` zeigt die aktuellen Optionen.

## Formality values (default settings)

- `casual`: locker, alltagssprachlich
- `formal`: höflich, formell
- `loose`: entspannt, locker
- `academic`: wissenschaftlich
- `gal`: verspielter Gyaru‑Ton
- `yankee`: rauer „Yankee“-Stil
- `otaku`: Otaku‑Diktion
- `elderly`: sanfter, älterer Ton
- `aristocrat`: aristokratischer Ton
- `samurai`: archaischer Samurai‑Stil
- `braille`: Ausgabe in Unicode‑Braille
- `morse`: Ausgabe in Morse‑Code
- `engineer`: präziser technischer Stil