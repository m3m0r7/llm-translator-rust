# llm-translator-rust

[English](README.md) | 日本語 | [中文](README.cn.md) | [Français](README.fr.md) | [Deutsch](README.ge.md) | [Italiano](README.it.md) | [한국어](README.kr.md) | [Русский](README.ru.md) | [UK English](README.uk.md)

LLM のツール呼び出し (JSON) を使った、stdin 入力専用の翻訳 CLI です。

## 目次

- [概要](#概要)
- [インストール](#インストール)
- [クイックスタート](#クイックスタート)
- [エイリアス `t` を使う](#エイリアス-t-を使う)
- [使い方](#使い方)
- [モデル選択とキャッシュ](#モデル選択とキャッシュ)
- [設定](#設定)
- [言語パック](#言語パック)
- [環境変数](#環境変数)
- [オプション](#オプション)
- [サーバーモード](#サーバーモード)
- [FFI (C ABI)](#ffi-c-abi)
- [補足](#補足)

## 概要

- stdin から入力して翻訳結果のみ出力します。
- ツール呼び出し (JSON) のみを返す前提です。
- 対応プロバイダ: OpenAI / Gemini / Claude。
- 各プロバイダの Models API から一覧を取得し、24 時間キャッシュします。

## インストール

以下のいずれかを選んでください。

### 1) GitHub Releases からダウンロード

Releases ページにビルド済みバイナリがあります:
[GitHub Releases](https://github.com/m3m0r7/llm-translator-rust/releases/latest)

ファイル名は `llm-translator-rust-<os>-<arch>` 形式です（例: `llm-translator-rust-macos-aarch64`）。

### 2) cargo でグローバルにインストール

```bash
cargo install --git https://github.com/m3m0r7/llm-translator-rust --locked
```

### 3) ソースからビルド（git clone）

```bash
git clone https://github.com/m3m0r7/llm-translator-rust
cd llm-translator-rust
make
sudo make install
```

生成物:

```
./target/release/llm-translator-rust
```
補足:
- macOS/Linux は `/usr/local/bin` が既定（必要なら `sudo make install`）
- Windows (MSYS/Git Bash) は `%USERPROFILE%/.cargo/bin`
- `make` は `build_env.toml` を生成し、バイナリに埋め込みます（ポータブル配布時にファイルは不要）。
- `make` に環境変数を渡してパスを上書きできます。例:
  `BASE_DIRECTORY=~/.llm-translator-rust BIN_DIRECTORY=target/release INSTALL_DIRECTORY=/usr/local/bin SETTINGS_FILE=~/.llm-translator-rust/settings.toml BUILD_ENV_PATH=build_env.toml make`
- `make install` は `settings.toml` が無い場合 `baseDirectory` にコピーします。

## クイックスタート

```bash
export OPENAI_API_KEY="..."
./target/release/llm-translator-rust <<< "ねこ"
```

## エイリアス `t` を使う

```bash
alias t="/path/to/llm-translator-rust/target/release/llm-translator-rust"

echo ねこ | t
```

## 使い方

```bash
echo ねこ | llm-translator-rust

echo ねこ | llm-translator-rust -l en

echo ねこ | llm-translator-rust --source-lang ja -l en

# 出力例
echo ねこ | llm-translator-rust
# Cat

echo ねこ | llm-translator-rust -l en
# Cat

echo ねこ | llm-translator-rust -l kor
# 고양이

echo ねこ | llm-translator-rust -l zho-hans
# 猫

echo ねこ | llm-translator-rust -l zho-hant
# 貓

echo ねこ | llm-translator-rust -l en --formal academic
# Cat

echo 最高だね | llm-translator-rust -l en --slang
# Awesome

# 辞書（品詞・活用）
echo 猫 | llm-translator-rust --pos -l en

# ファイル翻訳
cat foobar.txt | llm-translator-rust -l en

# 添付ファイル翻訳（画像/doc/docx/pptx/xlsx/pdf/txt/md/html/json/yaml/po/xml/js/ts/tsx/mermaid/音声）
llm-translator-rust --data ./slides.pptx --data-mime pptx -l en
llm-translator-rust --data ./scan.png -l ja
llm-translator-rust --data ./voice.mp3 -l en

# stdin から添付（自動判定 or --data-mime）
cat ./scan.png | llm-translator-rust -l ja
cat ./report.pdf | llm-translator-rust --data-mime pdf -l en

# 画像/PDF は番号付き注釈で再レンダリング（パスを返します）
# 画像の高さを下に伸ばして、フッターに一覧を追加します:
# (N) 原文 (読み): 翻訳
# - 読みは非ラテン文字の発音をラテン文字で表記（例: ローマ字/ピンイン）
# - 同じ翻訳語は同じ番号になります
# --data でファイルを指定した場合（--overwrite なし）は、同じ場所に出力します。
# 接尾辞は settings.toml の [system].translated_suffix（既定: _translated）。
# --data にディレクトリを指定した場合は、同じ接尾辞の出力ディレクトリを作成します。
```

## ディレクトリ翻訳

`--data` にディレクトリを渡すと再帰的に走査し、対応ファイルを翻訳します。
相対パス構成は出力ディレクトリに維持されます。

```bash
llm-translator-rust --data ./docs -l ja
# 出力: ./docs_translated（既定の接尾辞。settings.toml で変更可）
```

補足:
- `--data-mime` はディレクトリ内の全ファイルに適用されます。混在する場合は `auto` のままにしてください。
- 読み込み不可や MIME 判定失敗は failure として報告され、対応外の形式は skip されます。
- 判定が不確かな場合は `--force` でテキスト扱いにできます。
- ディレクトリ翻訳は並列実行します（既定 3 スレッド）。`--directory-translation-threads` または
  `settings.toml` で変更できます。
- `--ignore-translation-file` または無視ファイル（既定: `.llm-translation-rust-ignore`、
  `settings.toml` で変更可）で翻訳対象から除外できます。
  パターンは `.gitignore` と同様です（`*`, `**`, `!`, コメント）。
- 無視ルールは `--data` がディレクトリのときのみ適用されます。
- ディレクトリ翻訳で失敗したファイルは、元の内容をそのまま出力先へコピーします。
- 出力先ディレクトリは `--out` で指定できます。

## 上書きモード (--overwrite)

`--overwrite` は `--data` で指定したファイル/ディレクトリに上書きで書き込みます。
書き込み前に `~/.llm-translator-rust/backup` へバックアップします。
保持期間は `settings.toml` の `[system].backup_ttl_days`（既定: 30）で制御します。

```bash
llm-translator-rust --data ./docs --overwrite -l ja
llm-translator-rust --data ./slide.pdf --overwrite -l en
```

## 出力先 (--out)

`--out` でファイル/ディレクトリの出力先を指定できます。
`--overwrite` と同時には使えません。

```bash
llm-translator-rust --data ./docs -l ja --out ./outdir
llm-translator-rust --data ./slide.pdf -l en --out ./translated.pdf
```

## 辞書機能（--pos）

`--pos` は入力語に対する辞書形式の情報を返します。

使い方:

```
echo 猫 | llm-translator-rust --pos -l en
```

出力例（ラベルは source language で出力されます）:

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

- ラベルは source language の言語で出力します。
- `読み` は翻訳語の発音を source language の文字体系で表記します
  （例: 日本語=カタカナ、中国語=声調付きピンイン、韓国語=ハングル）。
- `別訳` は候補の翻訳語と読みを列挙します。
- `使用用途` と `使用例` の原文は source language で統一します。
- 使用例は翻訳語または別訳を必ず含むように補正します。

## 校正（--correction）

`--correction` は入力文の校正を行い、指摘内容を返します。

使い方:

```
echo "This is pen" | llm-translator-rust --correction --source-lang en
```

出力例:

```
This is a pen
        -

Correction reasons:
- English requires a/an before a countable noun
```

## 音声翻訳

音声は `whisper-rs` で文字起こし → LLM 翻訳 → TTS で再合成します。

- 対応音声: mp3, wav, m4a, flac, ogg
- `ffmpeg` が必要
- Whisper モデルは初回自動ダウンロード
- TTS は macOS `say` / Linux `espeak`

モデル指定:

```
llm-translator-rust --show-whisper-models
llm-translator-rust --whisper-model small -d ./voice.mp3 -l en
```

`LLM_TRANSLATOR_WHISPER_MODEL` にモデル名またはパスを指定してもOKです。
`settings.toml` の `[whisper] model` または `--whisper-model` が優先されます。

## 依存ツール

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

## 画像翻訳の例

翻訳前:

![翻訳前](docs/image.png)

翻訳後:

![翻訳後](docs/image_translated.png)

## モデル選択とキャッシュ

- 既定の優先順: OpenAI → Gemini → Claude（最初に見つかった API キー）。
- `-m/--model` は次の形式を受け付けます。
  - プロバイダ名のみ: `openai`, `gemini`, `claude`（下記の既定モデルを使用。なければチャット対応モデルの先頭）
  - `provider:model`: `openai:MODEL_ID`
  - モデル指定時は必ず `provider:` を付けてください。
- 既定モデルが利用できない場合はチャット対応モデルの先頭を選びます（非対応モデルは弾きます）。
- 入力/出力言語は ISO 639-1 または ISO 639-2/3 のコード（例: `ja`, `en`, `jpn`, `eng`）を指定します。入力は `auto` も可。
- 中国語の簡体/繁体は `zho-hans`（簡体）または `zho-hant`（繁体）を指定します。
- 言語コードの検証は Wikipedia の ISO 639 一覧を使用します: https://en.wikipedia.org/wiki/List_of_ISO_639_language_codes
- モデル一覧は各プロバイダの Models API から取得し、24 時間キャッシュします。
- キャッシュ場所:
  - `~/.llm-translator/.cache/meta.json`（`HOME` 未設定時は `./.llm-translator/.cache/meta.json`）
- `--show-models-list` で `provider:model` 形式の一覧を表示します。
- `--show-whisper-models` で Whisper のモデル名一覧を表示します。
- `--pos` は辞書形式で訳語・読み・別訳・品詞・活用・使用例を返します。
- `--correction` は入力文の校正結果と理由を返します。
- `--whisper-model` で音声文字起こしのモデル名/パスを指定できます。
- `--model` を省略した場合は `meta.json` の `lastUsingModel` を優先します（未設定/無効なら従来の解決方法にフォールバック）。
- 履歴は `meta.json` に保存します。出力先は `~/.llm-translator-rust/.cache/dest/<md5>` です。
- 画像/PDF は OCR（tesseract）で抽出した文字を LLM で正規化し、番号付き注釈とフッター一覧を再レンダリングします。
- Office（docx/xlsx/pptx）は XML 内のテキストを置き換えて出力します。
- 出力の MIME は入力に合わせます（png は png、pdf は pdf）。
- OCR 言語は `--source-lang` と `--lang` から推定します。
- 利用可能な OCR 言語は `tesseract --list-langs` で確認できます。
- PDF OCR にはレンダラが必要です（`mutool` または `pdftoppm`/poppler）。
- PDF は画像化されるためテキストは選択不可になります。

既定モデル:
- OpenAI: `openai:gpt-5.2`
- Gemini: `gemini:gemini-2.5-flash`
- Claude: `claude:claude-sonnet-4-5-20250929`

各プロバイダの Models API:
- [OpenAI Models API](https://platform.openai.com/docs/api-reference/models)
- [Gemini Models API](https://ai.google.dev/api/models)
- [Anthropic Models API](https://docs.anthropic.com/en/api/models)

## 設定

設定ファイルは次の優先順で読み込まれます（上ほど優先）:

1. `~/.llm-translator-rust/settings.local.toml`
2. `~/.llm-translator-rust/settings.toml`
3. `./settings.local.toml`
4. `./settings.toml`

`-r/--read-settings` で任意の TOML ファイルを追加で読み込めます（最優先）。

`settings.toml` の書式:
`system.languages` は ISO 639-3 のコードを指定します。

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

## 言語パック

言語パックは `src/languages/<iso-639-3>.toml` に置きます。`system.languages` の先頭が `--show-enabled-languages` の表示言語になります。

例（日本語）:

```toml
[translate.iso_country_lang.jpn]
jpn = "日本語"
eng = "英語"

```

## 環境変数

- OpenAI: `OPENAI_API_KEY`
- Gemini: `GEMINI_API_KEY` または `GOOGLE_API_KEY`
- Claude: `ANTHROPIC_API_KEY`

`-k/--key` を指定すると環境変数より優先されます。

## オプション

| フラグ | ロング | 説明 | 既定 |
| --- | --- | --- | --- |
| `-l` | `--lang` | 翻訳先言語 | `en` |
| `-m` | `--model` | プロバイダ/モデル選択 | (自動) |
| `-k` | `--key` | API キーを直接指定 | (env) |
| `-f` | `--formal` | スタイルキー（`settings.toml` の `[formally]` 参照） | `formal` |
| `-L` | `--source-lang` | 入力言語（ISO 639-1/2/3 または `auto`） | `auto` |
| `-s` | `--slang` | スラングのキーワードを許可 | `false` |
| `-d` | `--data` | 添付ファイル（画像/doc/docx/pptx/xlsx/pdf/txt/md/html/json/yaml/po/xml/js/ts/tsx/mermaid/音声） |  |
| `-M` | `--data-mime` | `--data` または stdin の MIME（`auto`, `image/*`, `pdf`, `doc`, `docx`, `docs`, `pptx`, `xlsx`, `txt`, `md`, `markdown`, `html`, `json`, `yaml`, `po`, `xml`, `js`, `ts`, `tsx`, `mermaid`, `mp3`, `wav`, `m4a`, `flac`, `ogg`） | `auto` |
|  | `--with-commentout` | コメントアウトも翻訳する（HTML/YAML/PO） |  |
|  | `--show-enabled-languages` | 有効な翻訳言語を表示 |  |
|  | `--show-enabled-styles` | 有効なスタイルキーを表示 |  |
|  | `--show-models-list` | 取得済みモデル一覧を表示（provider:model） |  |
|  | `--show-whisper-models` | Whisper モデル名の一覧を表示 |  |
|  | `--pos` | 品詞・活用などの辞書形式で出力 |  |
|  | `--correction` | 入力文の校正（指摘）を行う |  |
|  | `--show-histories` | 翻訳履歴を表示 |  |
|  | `--with-using-tokens` | トークン使用量を付加 |  |
|  | `--with-using-model` | 使用モデル名を付加 |  |
|  | `--force` | MIME 判定が不確かな場合でもテキスト扱いで翻訳 |  |
|  | `--debug-ocr` | OCR デバッグ用の bbox 画像/JSON を出力 |  |
|  | `--whisper-model` | Whisper モデル名またはパス |  |
|  | `--overwrite` | 入力ファイル/ディレクトリを上書き（バックアップは `~/.llm-translator-rust/backup`） |  |
|  | `--directory-translation-threads` | ディレクトリ翻訳の並列数 |  |
|  | `--ignore-translation-file` | ディレクトリ翻訳の無視パターン（gitignore 形式） |  |
| `-o` | `--out` | 出力先（ファイル/ディレクトリ） |  |
|  | `--verbose` | 詳細ログを出力 |  |
| `-i` | `--interactive` | インタラクティブモード |  |
| `-r` | `--read-settings` | 追加の設定 TOML を読み込む |  |
|  | `--server` | HTTP サーバーを起動（`ADDR` は settings または `0.0.0.0:11223` が既定） |  |
| `-h` | `--help` | ヘルプ表示 |  |

## サーバーモード

HTTP サーバーを起動します。

```bash
llm-translator-rust --server
llm-translator-rust --server 0.0.0.0:11223
```

`settings.toml` の `[server]` で設定できます。

```toml
[server]
host = "0.0.0.0"
port = 11223
tmp_dir = "/tmp/llm-translator-rust"
```

リクエストは JSON の `POST /translate`（`text` か `data` のどちらか）です。

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

校正リクエスト:

```json
{
  "text": "This is pen",
  "correction": true,
  "source_lang": "en"
}
```

レスポンス（テキスト）:

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

校正レスポンス（テキスト）:

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

レスポンス（バイナリ）:

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

`data` がディレクトリの場合は `contents` に複数エントリが入ります。

## FFI (C ABI)

- C ヘッダは `ext/llm_translator_rust.h` にあります。
- 文字列を返す関数は `llm_ext_free_string` で解放します。
- 失敗時は `llm_ext_last_error_message` でエラー内容を取得できます。

## 補足

- クレジット不足などの API エラーは、各プロバイダのエラーメッセージをそのまま表示します。
- 最新のオプションは `-h/--help` で確認できます。

## Formality の値（デフォルト設定）

- `casual`: 普段の会話調
- `formal`: 丁寧でフォーマル
- `loose`: くだけた自然体
- `academic`: 論文調
- `gal`: ギャルのニュアンス
- `yankee`: ヤンキー口調
- `otaku`: オタク寄りの語彙や言い回し
- `elderly`: 年配の落ち着いた口調
- `aristocrat`: 貴族的で上品な語り口
- `samurai`: 武士風の古風な言い回し
- `braille`: 点字（Unicode Braille パターン）
- `morse`: モールス信号（国際標準）
- `engineer`: 技術者向けの精密な表現
