# llm-translator-rust

Italiano | [English](README.md) | [日本語](README.ja.md) | [中文](README.cn.md) | [Français](README.fr.md) | [Deutsch](README.ge.md) | [한국어](README.kr.md) | [Русский](README.ru.md) | [UK English](README.uk.md)

Un piccolo traduttore CLI che usa chiamate di strumenti LLM e legge sempre da stdin.

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
- [FFI (C ABI)](#ffi-c-abi)
- [Notes](#notes)

## Overview

- Legge l'input da stdin e stampa il testo tradotto.
- Usa solo JSON di tool-calling (nessun output libero).
- Provider: OpenAI, Gemini, Claude.
- L'elenco dei modelli viene recuperato tramite l'API Models di ciascun provider e messo in cache per 24 ore.

## Installation

Scegli una delle seguenti opzioni:

### 1) Scarica da GitHub Releases

Gli artefatti di release sono disponibili nella pagina Releases:
[GitHub Releases](https://github.com/m3m0r7/llm-translator-rust/releases/latest)

Ogni asset è denominato `llm-translator-rust-<os>-<arch>` (ad es. `llm-translator-rust-macos-aarch64`).

### 2) Installa con cargo (globale)

```bash
cargo install --git https://github.com/m3m0r7/llm-translator-rust --locked
```

### 3) Compila dai sorgenti (git clone)

```bash
git clone https://github.com/m3m0r7/llm-translator-rust
cd llm-translator-rust
make install
```

Il binario sarà disponibile in:

```
./target/release/llm-translator-rust
```
Note:
- macOS/Linux predefinito: `/usr/local/bin` (usa `sudo make install` se necessario)
- Windows (MSYS/Git Bash): `%USERPROFILE%/.cargo/bin`
- `make install` copia anche `settings.toml` in `~/.llm-translator-rust/settings.toml` se non esiste.

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

Quando `--data` punta a una directory, la CLI la percorre ricorsivamente e traduce ogni file supportato.
La struttura relativa delle directory è preservata nella directory di output.

```bash
llm-translator-rust --data ./docs -l ja
# Output: ./docs_translated (default suffix; configurable via settings.toml)
```

Note:
- `--data-mime` si applica a ogni file nella directory; lascialo su `auto` per tipi misti.
- I file che non possono essere letti o il cui mime non può essere rilevato sono segnalati come failure; i file
  rilevati ma non supportati dal traduttore vengono ignorati.
- Usa `--force` per trattare rilevamenti sconosciuti/a bassa confidenza come testo.
- La traduzione delle directory viene eseguita in parallelo (predefinito 3 thread). Usa
  `--directory-translation-threads` o `settings.toml` per cambiarlo.
- I file possono essere esclusi con `--ignore-translation-file` o un file di ignore
  (predefinito: `.llm-translation-rust-ignore`, configurabile via `settings.toml`).
  I pattern seguono le regole `.gitignore` (`*`, `**`, `!`, commenti).
- Le regole di ignore si applicano solo quando `--data` punta a una directory.
- Usa `--out` per scegliere la directory di output per la traduzione delle directory.
- Quando una traduzione di directory fallisce, il file originale viene copiato nella directory di output.

## Overwrite mode (--overwrite)

`--overwrite` scrive i risultati in loco per file o directory passati tramite `--data`.
Prima di scrivere, ogni file viene salvato in `~/.llm-translated-rust/backup`.
La conservazione è controllata da `settings.toml` `[system].backup_ttl_days` (predefinito: 30).

```bash
llm-translator-rust --data ./docs --overwrite -l ja
llm-translator-rust --data ./slide.pdf --overwrite -l en
```

## Output path (--out)

`--out` imposta il percorso di output per le traduzioni di file o directory.
Non può essere usato con `--overwrite`.

```bash
llm-translator-rust --data ./docs -l ja --out ./outdir
llm-translator-rust --data ./slide.pdf -l en --out ./translated.pdf
```

## Dictionary (--pos)

`--pos` restituisce dettagli in stile dizionario per il termine in input.

Uso:

```
echo 猫 | llm-translator-rust --pos -l en
```

Esempio di output (le etichette seguono la lingua sorgente):

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

`--correction` rilegge l'input e segnala le correzioni nella lingua sorgente.

Uso:

```
echo "This is pen" | llm-translator-rust --correction --source-lang en
```

Esempio di output:

```
This is a pen
        -

Correction reasons:
- English requires a/an before a countable noun
```

- Le etichette sono localizzate nella lingua sorgente.
- `Reading` è la pronuncia della traduzione resa nella scrittura tipica della lingua sorgente
  (ad es. giapponese=katakana, cinese=pinyin con segni di tono, coreano=hangul).
- `Alternatives` elenca altre traduzioni plausibili con letture.
- `Usage` e le frasi sorgente di esempio sono nella lingua sorgente.
- Gli esempi includono la traduzione o una delle alternative.

## Audio translation

I file audio vengono trascritti con `whisper-rs`, tradotti dall'LLM, quindi risintetizzati.

- Audio supportati: mp3, wav, m4a, flac, ogg
- Richiede `ffmpeg`
- Richiede un modello Whisper (scaricato automaticamente al primo avvio)
- TTS usa `say` su macOS o `espeak` su Linux

Scegli un modello:

```
llm-translator-rust --show-whisper-models
llm-translator-rust --whisper-model small -d ./voice.mp3 -l en
```

Puoi anche impostare `LLM_TRANSLATOR_WHISPER_MODEL` su un nome di modello o un percorso di file.
`settings.toml` `[whisper] model` o `--whisper-model` sovrascrive questa impostazione.

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

Originale:

![Original image](docs/image.png)

Tradotto:

![Translated image](docs/image_translated.png)

## Model selection & cache

- Priorità predefinita dei provider: OpenAI → Gemini → Claude (prima API key trovata).
- `-m/--model` accetta:
  - Solo provider: `openai`, `gemini`, `claude` (usa i predefiniti del provider qui sotto, se disponibili)
  - Provider + modello: `openai:MODEL_ID`
  - Quando specifichi un modello, includi sempre il prefisso del provider.
- I predefiniti usano quelli del provider qui sotto; se non disponibili, viene usato il primo modello compatibile con chat.
- Le lingue sorgente/destinazione usano codici ISO 639-1 o ISO 639-2/3 (ad es. `ja`, `en`, `jpn`, `eng`). La sorgente può essere `auto`.
- Per le varianti del cinese, usa `zho-hans` (Semplificato) o `zho-hant` (Tradizionale).
- La validazione delle lingue usa la lista ISO 639 da Wikipedia: https://en.wikipedia.org/wiki/List_of_ISO_639_language_codes
- L'elenco dei modelli viene recuperato dall'API Models di ciascun provider e messo in cache per 24 ore.
- Percorso cache:
  - `~/.llm-translator/.cache/meta.json` (fallback: `./.llm-translator/.cache/meta.json`)
- `--show-models-list` stampa la lista in cache come `provider:model` per riga.
- `--show-whisper-models` stampa i nomi dei modelli whisper disponibili.
- `--pos` restituisce dettagli in stile dizionario (traduzione + lettura, POS, alternative, flessioni, uso/esempi).
- `--correction` restituisce correzioni di proofreading e motivi nella lingua sorgente.
- `--whisper-model` seleziona il nome o il percorso del modello whisper per la trascrizione audio.
- Quando `--model` è omesso, viene preferito `lastUsingModel` in `meta.json` (fallback alla risoluzione predefinita se mancante o non valido).
- Le cronologie sono salvate in `meta.json`. I file di destinazione sono scritti in `~/.llm-translator-rust/.cache/dest/<md5>`.
- Gli allegati immagine/PDF usano OCR (tesseract), normalizzano il testo OCR con LLM e ri-renderizzano un overlay numerato più una lista a piè di pagina.
- I file Office (docx/xlsx/pptx) vengono riscritti traducendo i nodi di testo nell'XML.
- Il mime di output coincide con il mime di input (ad es. png resta png, pdf resta pdf).
- Le lingue OCR sono dedotte da `--source-lang` e `--lang`.
- Usa `tesseract --list-langs` per vedere i codici lingua OCR installati.
- L'OCR dei PDF richiede un renderer PDF (`mutool` o `pdftoppm` da poppler).
- L'output PDF è rasterizzato (il testo non è più selezionabile).

Provider defaults:
- OpenAI: `openai:gpt-5.1`
- Gemini: `gemini:gemini-2.5-flash`
- Claude: `claude:claude-sonnet-4-5-20250929`

Provider model APIs:
- [OpenAI Models API](https://platform.openai.com/docs/api-reference/models)
- [Gemini Models API](https://ai.google.dev/api/models)
- [Anthropic Models API](https://docs.anthropic.com/en/api/models)

## Settings

I file di configurazione sono caricati con la seguente precedenza (la più alta per prima):

1. `~/.llm-translator-rust/settings.local.toml`
2. `~/.llm-translator-rust/settings.toml`
3. `./settings.local.toml`
4. `./settings.toml`

Puoi anche passare `-r/--read-settings` per caricare un file TOML locale aggiuntivo (priorità più alta).

`settings.toml` usa il seguente formato:
`system.languages` deve essere in codici ISO 639-3.

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

I pacchetti lingua si trovano in `src/languages/<iso-639-3>.toml`. La prima voce in `system.languages` è usata per la visualizzazione delle etichette in `--show-enabled-languages`.

Esempio (giapponese):

```toml
[translate.iso_country_lang.jpn]
jpn = "日本語"
eng = "英語"

```

## Environment variables

- OpenAI: `OPENAI_API_KEY`
- Gemini: `GEMINI_API_KEY` o `GOOGLE_API_KEY`
- Claude: `ANTHROPIC_API_KEY`

`-k/--key` sovrascrive le variabili d'ambiente.

## Options

| Flag | Long | Descrizione | Predefinito |
| --- | --- | --- | --- |
| `-l` | `--lang` | Lingua di destinazione | `en` |
| `-m` | `--model` | Selettore provider/modello | (auto) |
| `-k` | `--key` | Override della chiave API | (env) |
| `-f` | `--formal` | Chiave di formalità (da `settings.toml` `[formally]`) | `formal` |
| `-L` | `--source-lang` | Lingua sorgente (ISO 639-1/2/3 o `auto`) | `auto` |
| `-s` | `--slang` | Includi parole slang quando appropriato | `false` |
| `-d` | `--data` | Allegato file (image/doc/docx/pptx/xlsx/pdf/txt/md/html/json/yaml/po/xml/js/ts/tsx/mermaid/audio) |  |
| `-M` | `--data-mime` | Tipo mime per `--data` (o stdin) (`auto`, `image/*`, `pdf`, `doc`, `docx`, `docs`, `pptx`, `xlsx`, `txt`, `md`, `markdown`, `html`, `json`, `yaml`, `po`, `xml`, `js`, `ts`, `tsx`, `mermaid`, `mp3`, `wav`, `m4a`, `flac`, `ogg`) | `auto` |
|  | `--with-commentout` | Traduci il testo commentato (HTML/YAML/PO) |  |
|  | `--show-enabled-languages` | Mostra le lingue di traduzione abilitate |  |
|  | `--show-enabled-styles` | Mostra le chiavi di stile abilitate |  |
|  | `--show-models-list` | Mostra l'elenco modelli in cache (provider:model) |  |
|  | `--show-whisper-models` | Mostra i nomi dei modelli whisper disponibili |  |
|  | `--pos` | Output dizionario (parti del discorso/flessioni) |  |
|  | `--correction` | Correggi il testo in input e segnala le correzioni |  |
|  | `--show-histories` | Mostra lo storico delle traduzioni |  |
|  | `--with-using-tokens` | Aggiungi l'uso dei token all'output |  |
|  | `--with-using-model` | Aggiungi il nome del modello all'output |  |
|  | `--force` | Forza la traduzione quando il rilevamento mime è incerto (tratta come testo) |  |
|  | `--debug-ocr` | Output overlay/JSON di debug OCR per gli allegati |  |
|  | `--whisper-model` | Nome o percorso del modello Whisper |  |
|  | `--overwrite` | Sovrascrivi i file di input in loco (backup in `~/.llm-translated-rust/backup`) |  |
|  | `--directory-translation-threads` | Concorrenza traduzione directory |  |
|  | `--ignore-translation-file` | Pattern di esclusione per la traduzione directory (simile a gitignore) |  |
| `-o` | `--out` | Percorso di output per file o directory tradotti |  |
|  | `--verbose` | Log verbosi |  |
| `-i` | `--interactive` | Modalità interattiva |  |
| `-r` | `--read-settings` | Leggi un file TOML di impostazioni extra |  |
|  | `--server` | Avvia il server HTTP (`ADDR` predefinito: settings o `0.0.0.0:11223`) |  |
| `-h` | `--help` | Mostra aiuto |  |

## Server mode

Avvia il server HTTP:

```bash
llm-translator-rust --server
llm-translator-rust --server 0.0.0.0:11223
```

Le impostazioni del server sono configurabili in `settings.toml` sotto `[server]`:

```toml
[server]
host = "0.0.0.0"
port = 11223
tmp_dir = "/tmp/llm-translator-rust"
```

Le richieste sono JSON `POST /translate` (`text` o percorso `data`):

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

Richiesta di correzione:

```json
{
  "text": "This is pen",
  "correction": true,
  "source_lang": "en"
}
```

Risposta (testo):

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

Risposta correzione (testo):

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

Risposta (binario):

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

Quando `data` è una directory, in `contents` vengono restituiti più elementi.

## FFI (C ABI)

- L'header C si trova in `ext/llm_translator_rust.h`.
- Le funzioni restituiscono stringhe allocate sull'heap; liberale con `llm_ext_free_string`.
- Quando una chiamata fallisce, recupera un messaggio con `llm_ext_last_error_message`.

## Notes

- Gli errori API (incluse quote insufficienti) vengono mostrati con i messaggi di errore del provider.
- Usa `-h/--help` per vedere le opzioni più recenti.

## Formality values (default settings)

- `casual`: tono colloquiale quotidiano
- `formal`: tono formale e cortese
- `loose`: formulazione rilassata e sciolta
- `academic`: formulazione accademica
- `gal`: tono giocoso gyaru/gal
- `yankee`: stile duro da teppista
- `otaku`: dizione e sfumature da otaku
- `elderly`: registro gentile da anziano
- `aristocrat`: tono aristocratico raffinato
- `samurai`: formulazione arcaica stile samurai
- `braille`: output in pattern Braille Unicode
- `morse`: output in codice Morse internazionale
- `engineer`: tono tecnico preciso
