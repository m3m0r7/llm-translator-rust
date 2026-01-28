# llm-translator-rust

[English](README.md) | 日本語

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
make install
```

生成物:

```
./target/release/llm-translator-rust
```
補足:
- macOS/Linux は `/usr/local/bin` が既定（必要なら `sudo make install`）
- Windows (MSYS/Git Bash) は `%USERPROFILE%/.cargo/bin`
- `make install` は `~/.llm-translator-rust/settings.toml` が無ければ `settings.toml` をコピーします。

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

# ファイル翻訳
cat foobar.txt | llm-translator-rust -l en

# 添付ファイル翻訳（画像/doc/docx/pptx/xlsx/pdf/txt）
llm-translator-rust --data ./slides.pptx --data-mime pptx -l en
llm-translator-rust --data ./scan.png -l ja

# stdin から添付（自動判定 or --data-mime）
cat ./scan.png | llm-translator-rust -l ja
cat ./report.pdf | llm-translator-rust --data-mime pdf -l en

# 画像/PDF は番号付き注釈で再レンダリング（パスを返します）
# 画像の高さを下に伸ばして、フッターに一覧を追加します:
# (N) 原文 (読み): 翻訳
# - 読みは非ラテン文字の発音をラテン文字で表記（例: ローマ字/ピンイン）
# - 同じ翻訳語は同じ番号になります
# --data でファイルを指定した場合は、同じ場所に *_translated.<ext> も出力します。
```

## 画像翻訳の例

翻訳前:

![翻訳前](docs/image.png)

翻訳後:

![翻訳後](docs/image_translated.png)

## モデル選択とキャッシュ

- 既定の優先順: OpenAI → Gemini → Claude（最初に見つかった API キー）。
- `-M/--model` は次の形式を受け付けます。
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
- OpenAI: `openai:gpt-5.1`
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
| `-M` | `--model` | プロバイダ/モデル選択 | (自動) |
| `-k` | `--key` | API キーを直接指定 | (env) |
| `-f` | `--formal` | スタイルキー（`settings.toml` の `[formally]` 参照） | `formal` |
| `-c` | `--source-lang` | 入力言語（ISO 639-1/2/3 または `auto`） | `auto` |
|  | `--countery-language` | `--source-lang` の別名 |  |
| `-s` | `--slang` | スラングのキーワードを許可 | `false` |
| `-d` | `--data` | 添付ファイル（画像/doc/docx/pptx/xlsx/pdf/txt） |  |
| `-m` | `--data-mime` | `--data` または stdin の MIME（`auto`, `image/*`, `pdf`, `doc`, `docx`, `docs`, `pptx`, `xlsx`, `txt`, `png`, `jpeg`, `gif`） | `auto` |
|  | `--show-enabled-languages` | 有効な翻訳言語を表示 |  |
|  | `--show-enabled-styles` | 有効なスタイルキーを表示 |  |
|  | `--show-models-list` | 取得済みモデル一覧を表示（provider:model） |  |
|  | `--show-histories` | 翻訳履歴を表示 |  |
|  | `--with-using-tokens` | トークン使用量を付加 |  |
|  | `--with-using-model` | 使用モデル名を付加 |  |
|  | `--debug-ocr` | OCR デバッグ用の bbox 画像/JSON を出力 |  |
| `-r` | `--read-settings` | 追加の設定 TOML を読み込む |  |
| `-h` | `--help` | ヘルプ表示 |  |

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
