# llm-translator-rust

Français | [English](README.md) | [日本語](README.ja.md) | [中文](README.cn.md) | [Deutsch](README.ge.md) | [Italiano](README.it.md) | [한국어](README.kr.md) | [Русский](README.ru.md) | [UK English](README.uk.md)

Un petit traducteur CLI qui utilise des appels d’outils LLM et lit toujours depuis stdin.

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

- Lit l’entrée depuis stdin et affiche la traduction.
- Utilise uniquement des appels d’outil JSON (pas de sortie libre).
- Fournisseurs : OpenAI, Gemini, Claude.
- La liste des modèles est récupérée via l’API Models de chaque fournisseur et mise en cache 24 h.

## Installation

Choisissez l’une des options suivantes :

### 1) Télécharger depuis GitHub Releases

Les artefacts sont disponibles sur la page Releases :
[GitHub Releases](https://github.com/m3m0r7/llm-translator-rust/releases/latest)

Chaque fichier est nommé `llm-translator-rust-<os>-<arch>` (ex. `llm-translator-rust-macos-aarch64`).

### 2) Installer avec cargo (global)

```bash
cargo install --git https://github.com/m3m0r7/llm-translator-rust --locked
```

### 3) Construire depuis la source (git clone)

```bash
git clone https://github.com/m3m0r7/llm-translator-rust
cd llm-translator-rust
make
sudo make install
```

Binaire disponible ici :

```
./target/release/llm-translator-rust
```

Notes :
- macOS/Linux : `/usr/local/bin` par défaut (utilisez `sudo make install` si besoin)
- Windows (MSYS/Git Bash) : `%USERPROFILE%/.cargo/bin`
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

Quand `--data` pointe vers un dossier, l’outil le parcourt récursivement et traduit chaque fichier supporté.
La structure relative est conservée dans le dossier de sortie.

```bash
llm-translator-rust --data ./docs -l ja
# Output: ./docs_translated (default suffix; configurable via settings.toml)
```

Notes :
- `--data-mime` s’applique à tous les fichiers du dossier ; laissez `auto` pour des types mixtes.
- Les fichiers illisibles ou au MIME indétectable sont signalés ; ceux non supportés sont ignorés.
- Utilisez `--force` pour traiter une détection inconnue/peu sûre comme du texte.
- Traduction de dossier en parallèle (3 threads par défaut). Ajustez via `--directory-translation-threads` ou `settings.toml`.
- Exclusions via `--ignore-translation-file` ou un fichier d’ignore (par défaut `.llm-translation-rust-ignore`, configurable dans `settings.toml`).
  Règles compatibles `.gitignore` (`*`, `**`, `!`, commentaires).
- Les règles d’ignore ne s’appliquent que lorsque `--data` est un dossier.
- Utilisez `--out` pour choisir le dossier de sortie.
- En cas d’échec de traduction d’un dossier, le fichier original est copié dans la sortie.

## Overwrite mode (--overwrite)

`--overwrite` écrit les résultats à la place des fichiers/dossiers passés via `--data`.
Chaque fichier est sauvegardé dans `~/.llm-translator-rust/backup` avant écriture.
La rétention est contrôlée par `settings.toml` `[system].backup_ttl_days` (30 par défaut).

```bash
llm-translator-rust --data ./docs --overwrite -l ja
llm-translator-rust --data ./slide.pdf --overwrite -l en
```

## Output path (--out)

`--out` définit le chemin de sortie pour les traductions de fichier ou dossier.
Ne peut pas être utilisé avec `--overwrite`.

```bash
llm-translator-rust --data ./docs -l ja --out ./outdir
llm-translator-rust --data ./slide.pdf -l en --out ./translated.pdf
```

## Dictionary (--pos)

`--pos` renvoie des informations de type dictionnaire pour le terme d’entrée.

Utilisation :

```
echo 猫 | llm-translator-rust --pos -l en
```

Exemple de sortie (étiquettes dans la langue source) :

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

`--correction` relit l’entrée et signale les corrections dans la langue source.

Utilisation :

```
echo "This is pen" | llm-translator-rust --correction --source-lang en
```

Exemple de sortie :

```
This is a pen
        -

Correction reasons:
- English requires a/an before a countable noun
```

- Les libellés sont localisés dans la langue source.
- `Reading` est la prononciation de la traduction dans l’écriture de la langue source.
- `Alternatives` liste des traductions possibles avec leurs lectures.
- `Usage` et les phrases source d’exemples sont en langue source.
- Les exemples contiennent la traduction ou une alternative.

## Audio translation

Les fichiers audio sont transcrits via `whisper-rs`, traduits par le LLM, puis resynthétisés.

- Audio supporté : mp3, wav, m4a, flac, ogg
- Nécessite `ffmpeg`
- Nécessite un modèle Whisper (téléchargé au premier lancement)
- TTS utilise `say` (macOS) ou `espeak` (Linux)

Choisir un modèle :

```
llm-translator-rust --show-whisper-models
llm-translator-rust --whisper-model small -d ./voice.mp3 -l en
```

Vous pouvez aussi définir `LLM_TRANSLATOR_WHISPER_MODEL` (nom ou chemin).
`settings.toml` `[whisper] model` ou `--whisper-model` l’emporte.

## Dependencies

macOS (Homebrew) :

```
brew install tesseract ffmpeg
```

Ubuntu/Debian :

```
sudo apt-get install tesseract-ocr ffmpeg espeak
```

Windows (Chocolatey) :

```
choco install tesseract ffmpeg
```

## Image translation example

Original :

![Original image](docs/image.png)

Translated :

![Translated image](docs/image_translated.png)

## Model selection & cache

- Priorité par défaut : OpenAI → Gemini → Claude (première clé trouvée).
- `-m/--model` accepte :
  - Fournisseur seul : `openai`, `gemini`, `claude`
  - Fournisseur + modèle : `openai:MODEL_ID`
  - Toujours inclure le préfixe fournisseur.
- Par défaut : modèle par défaut du fournisseur, sinon le premier modèle compatible chat.
- Langues source/cible en ISO 639-1 ou ISO 639-2/3 (ex. `ja`, `en`, `jpn`, `eng`). Source peut être `auto`.
- Variantes chinoises : `zho-hans` (simplifié) ou `zho-hant` (traditionnel).
- Validation via la liste ISO 639 de Wikipedia : https://en.wikipedia.org/wiki/List_of_ISO_639_language_codes
- Liste des modèles mise en cache 24 h.
- Cache :
  - `~/.llm-translator/.cache/meta.json` (repli : `./.llm-translator/.cache/meta.json`)
- `--show-models-list` affiche la liste `provider:model`.
- `--show-whisper-models` affiche les modèles whisper disponibles.
- `--pos` renvoie des détails de dictionnaire (traduction + lecture, POS, alternatives, flexions, usage/exemples).
- `--correction` renvoie les corrections et raisons (langue source).
- `--whisper-model` sélectionne le modèle de transcription audio.
- Sans `--model`, `lastUsingModel` dans `meta.json` est préféré.
- L’historique est stocké dans `meta.json`. Les sorties sont dans `~/.llm-translator-rust/.cache/dest/<md5>`.
- Les images/PDF utilisent l’OCR (tesseract), normalisent le texte avec le LLM et ré‑rendent une surcouche numérotée + liste.
- Les fichiers Office (docx/xlsx/pptx) sont réécrits via traduction des nœuds XML.
- Le MIME de sortie correspond à l’entrée.
- Langues OCR déduites de `--source-lang` et `--lang`.
- `tesseract --list-langs` pour lister les langues OCR.
- OCR PDF nécessite un rendu (`mutool` ou `pdftoppm` de poppler).
- La sortie PDF est rasterisée (texte non sélectionnable).

Provider defaults:
- OpenAI: `openai:gpt-5.2`
- Gemini: `gemini:gemini-2.5-flash`
- Claude: `claude:claude-sonnet-4-5-20250929`

Provider model APIs:
- [OpenAI Models API](https://platform.openai.com/docs/api-reference/models)
- [Gemini Models API](https://ai.google.dev/api/models)
- [Anthropic Models API](https://docs.anthropic.com/en/api/models)

## Settings

Ordre de chargement (priorité décroissante) :

1. `~/.llm-translator-rust/settings.local.toml`
2. `~/.llm-translator-rust/settings.toml`
3. `./settings.local.toml`
4. `./settings.toml`

`-r/--read-settings` permet d’ajouter un TOML local (priorité la plus haute).

Format `settings.toml` :
`system.languages` doit être en ISO 639-3.

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

Les packs de langue se trouvent dans `src/languages/<iso-639-3>.toml`.
La première entrée de `system.languages` sert aux libellés de `--show-enabled-languages`.

Exemple (japonais) :

```toml
[translate.iso_country_lang.jpn]
jpn = "日本語"
eng = "英語"

```

## Environment variables

- OpenAI: `OPENAI_API_KEY`
- Gemini: `GEMINI_API_KEY` ou `GOOGLE_API_KEY`
- Claude: `ANTHROPIC_API_KEY`

`-k/--key` écrase les variables d’environnement.

## Options

| Flag | Long | Description | Default |
| --- | --- | --- | --- |
| `-l` | `--lang` | Langue cible | `en` |
| `-m` | `--model` | Sélecteur fournisseur/modèle | (auto) |
| `-k` | `--key` | Surcharge de clé API | (env) |
| `-f` | `--formal` | Style (depuis `settings.toml` `[formally]`) | `formal` |
| `-L` | `--source-lang` | Langue source (ISO 639-1/2/3 ou `auto`) | `auto` |
| `-s` | `--slang` | Inclure du slang si pertinent | `false` |
| `-d` | `--data` | Fichier joint (image/doc/docx/pptx/xlsx/pdf/txt/md/html/json/yaml/po/xml/js/ts/tsx/mermaid/audio) |  |
| `-M` | `--data-mime` | MIME pour `--data` (ou stdin) | `auto` |
|  | `--with-commentout` | Traduire les commentaires (HTML/YAML/PO) |  |
|  | `--show-enabled-languages` | Afficher les langues activées |  |
|  | `--show-enabled-styles` | Afficher les styles activés |  |
|  | `--show-models-list` | Afficher la liste des modèles en cache |  |
|  | `--show-whisper-models` | Afficher les modèles whisper disponibles |  |
|  | `--pos` | Sortie dictionnaire (POS/flexions) |  |
|  | `--correction` | Corriger le texte et indiquer les corrections |  |
|  | `--details` | Detailed translations across all formal styles |  |
|  | `--show-histories` | Afficher l’historique |  |
|  | `--with-using-tokens` | Ajouter l’usage de tokens |  |
|  | `--with-using-model` | Ajouter le nom du modèle |  |
|  | `--force` | Forcer la traduction en texte si la détection est incertaine |  |
|  | `--debug-ocr` | Sorties OCR debug (overlay/JSON) |  |
|  | `--whisper-model` | Modèle Whisper (nom ou chemin) |  |
|  | `--overwrite` | Écrire en place (backup `~/.llm-translator-rust/backup`) |  |
|  | `--directory-translation-threads` | Concurrence de traduction de dossiers |  |
|  | `--ignore-translation-file` | Motifs d’exclusion (gitignore-like) |  |
| `-o` | `--out` | Chemin de sortie |  |
|  | `--verbose` | Logs verbeux |  |
| `-i` | `--interactive` | Mode interactif |  |
| `-r` | `--read-settings` | Lire un TOML de paramètres supplémentaire |  |
|  | `--server` | Démarrer le serveur HTTP (`ADDR` par défaut : settings ou `0.0.0.0:11223`) |  |
|  | `--mcp` | Start MCP server over stdio |  |
| `-h` | `--help` | Aide |  |

## Server mode

Démarrer le serveur HTTP :

```bash
llm-translator-rust --server
llm-translator-rust --server 0.0.0.0:11223
```

Paramètres serveur dans `settings.toml` `[server]` :

```toml
[server]
host = "0.0.0.0"
port = 11223
tmp_dir = "/tmp/llm-translator-rust"
```

Requêtes JSON `POST /translate` (`text` ou `data`) :

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

Requête correction :

```json
{
  "text": "This is pen",
  "correction": true,
  "source_lang": "en"
}
```

Réponse (texte) :

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

Réponse correction (texte) :

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

Réponse (binaire) :

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

Si `data` est un dossier, `contents` contient plusieurs entrées.

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

- En‑tête C : `ext/llm_translator_rust.h`.
- Les fonctions renvoient des chaînes sur le tas ; libérez avec `llm_ext_free_string`.
- En cas d’échec, récupérez le message avec `llm_ext_last_error_message`.

## Notes

- Les erreurs API (y compris quota insuffisant) sont renvoyées telles quelles.
- Utilisez `-h/--help` pour les options à jour.

## Formality values (default settings)

- `casual`: ton décontracté
- `formal`: ton poli et formel
- `loose`: formulation détendue
- `academic`: style académique
- `gal`: ton “gyaru” joueur
- `yankee`: style rude / voyou
- `otaku`: diction et nuance otaku
- `elderly`: registre doux, respectueux
- `aristocrat`: ton aristocratique
- `samurai`: style samouraï archaïque
- `braille`: sortie en braille Unicode
- `morse`: sortie en code Morse international
- `engineer`: ton technique précis