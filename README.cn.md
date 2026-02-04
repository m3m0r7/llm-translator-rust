# llm-translator-rust

中文 | [English](README.md) | [日本語](README.ja.md) | [Français](README.fr.md) | [Deutsch](README.ge.md) | [Italiano](README.it.md) | [한국어](README.kr.md) | [Русский](README.ru.md) | [UK English](README.uk.md)

一个小巧的 CLI 翻译器，使用 LLM 工具调用，并始终从 stdin 读取。

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

- 从 stdin 读取输入并输出翻译结果。
- 只使用工具调用 JSON（不输出自由文本）。
- 支持 OpenAI、Gemini、Claude。
- 模型列表通过各提供方的 Models API 获取并缓存 24 小时。

## Installation

请选择以下任一方式：

### 1) 从 GitHub Releases 下载

发布包在 Releases 页面：
[GitHub Releases](https://github.com/m3m0r7/llm-translator-rust/releases/latest)

每个资产命名为 `llm-translator-rust-<os>-<arch>`（例如 `llm-translator-rust-macos-aarch64`）。

### 2) 使用 cargo 安装（全局）

```bash
cargo install --git https://github.com/m3m0r7/llm-translator-rust --locked
```

### 3) 从源码构建（git clone）

```bash
git clone https://github.com/m3m0r7/llm-translator-rust
cd llm-translator-rust
make
sudo make install
```

二进制位于：

```
./target/release/llm-translator-rust
```

注意：
- macOS/Linux 默认安装到 `/usr/local/bin`（必要时使用 `sudo make install`）
- Windows（MSYS/Git Bash）：`%USERPROFILE%/.cargo/bin`
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

当 `--data` 指向目录时，CLI 会递归遍历并翻译每个支持的文件。
输出目录会保留相对结构。

```bash
llm-translator-rust --data ./docs -l ja
# Output: ./docs_translated (default suffix; configurable via settings.toml)
```

注意：
- `--data-mime` 会应用到目录内所有文件；混合类型时请保持 `auto`。
- 无法读取或无法检测 MIME 的文件会报错；检测到但不支持的会被跳过。
- 使用 `--force` 将未知/低置信 MIME 当作文本处理。
- 目录翻译支持并发（默认 3 线程）。可用 `--directory-translation-threads` 或 `settings.toml` 调整。
- 可通过 `--ignore-translation-file` 或忽略文件排除（默认 `.llm-translation-rust-ignore`，可在 `settings.toml` 配置）。
  规则兼容 `.gitignore`（`*`, `**`, `!`, 注释）。
- 忽略规则只在 `--data` 指向目录时生效。
- 使用 `--out` 指定输出目录。
- 当目录翻译失败时，原文件会被复制到输出目录。

## Overwrite mode (--overwrite)

`--overwrite` 会对 `--data` 的文件或目录原地写入。
写入前会备份到 `~/.llm-translator-rust/backup`。
保留天数由 `settings.toml` `[system].backup_ttl_days` 控制（默认 30）。

```bash
llm-translator-rust --data ./docs --overwrite -l ja
llm-translator-rust --data ./slide.pdf --overwrite -l en
```

## Output path (--out)

`--out` 指定文件或目录翻译的输出路径。
不能与 `--overwrite` 同时使用。

```bash
llm-translator-rust --data ./docs -l ja --out ./outdir
llm-translator-rust --data ./slide.pdf -l en --out ./translated.pdf
```

## Dictionary (--pos)

`--pos` 返回词典式信息。

使用方法：

```
echo 猫 | llm-translator-rust --pos -l en
```

示例输出（标签跟随源语言）：

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

`--correction` 会对输入进行校对，并在源语言中给出指正。

使用方法：

```
echo "This is pen" | llm-translator-rust --correction --source-lang en
```

示例输出：

```
This is a pen
        -

Correction reasons:
- English requires a/an before a countable noun
```

- 标签使用源语言本地化。
- `Reading` 为译文的发音（使用源语言常用文字体系，例如日文=片假名、中文=带声调拼音、韩文=韩文字母）。
- `Alternatives` 列出其他可能译法及读音。
- `Usage` 与示例源句使用源语言。
- 示例会包含主译文或某个别译。

## Audio translation

音频文件会先用 `whisper-rs` 转写，再由 LLM 翻译，最后重新合成语音。

- 支持：mp3, wav, m4a, flac, ogg
- 需要 `ffmpeg`
- 需要 Whisper 模型（首次运行会自动下载）
- TTS 使用 macOS `say` 或 Linux `espeak`

选择模型：

```
llm-translator-rust --show-whisper-models
llm-translator-rust --whisper-model small -d ./voice.mp3 -l en
```

也可设置 `LLM_TRANSLATOR_WHISPER_MODEL` 为模型名或文件路径。
`settings.toml` 的 `[whisper] model` 或 `--whisper-model` 会覆盖它。

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

- 默认提供方优先级：OpenAI → Gemini → Claude（找到第一个 API key）。
- `-m/--model` 支持：
  - 仅提供方：`openai`, `gemini`, `claude`（若可用则使用其默认模型）
  - 提供方 + 模型：`openai:MODEL_ID`
  - 指定模型时务必包含提供方前缀。
- 默认优先使用提供方默认模型；不可用时选择首个可用于聊天的模型。
- 源/目标语言支持 ISO 639-1 或 ISO 639-2/3（如 `ja`, `en`, `jpn`, `eng`），源语言可为 `auto`。
- 中文变体使用 `zho-hans`（简体）或 `zho-hant`（繁体）。
- 语言校验使用 Wikipedia 的 ISO 639 列表：https://en.wikipedia.org/wiki/List_of_ISO_639_language_codes
- 模型列表通过各提供方 Models API 获取并缓存 24 小时。
- 缓存路径：
  - `~/.llm-translator/.cache/meta.json`（`HOME` 未设置时为 `./.llm-translator/.cache/meta.json`）
- `--show-models-list` 以 `provider:model` 每行打印缓存列表。
- `--show-whisper-models` 输出可用 whisper 模型名。
- `--pos` 返回词典式详情（译词+读音、词性、别译、活用、用法/用例）。
- `--correction` 返回校对结果与原因（源语言）。
- `--whisper-model` 指定音频转写模型名或路径。
- 未指定 `--model` 时优先使用 `meta.json` 的 `lastUsingModel`（无效时回退到默认选择）。
- 历史记录存于 `meta.json`。目标文件写入 `~/.llm-translator-rust/.cache/dest/<md5>`。
- 图像/PDF 使用 OCR（tesseract），再由 LLM 规范化文本并重绘编号标注与脚注列表。
- Office 文件（docx/xlsx/pptx）通过翻译 XML 文本节点重写。
- 输出 MIME 与输入一致（例如 png 仍为 png，pdf 仍为 pdf）。
- OCR 语言由 `--source-lang` 与 `--lang` 推断。
- 使用 `tesseract --list-langs` 查看已安装的 OCR 语言。
- PDF OCR 需要渲染器（`mutool` 或 poppler 的 `pdftoppm`）。
- PDF 输出会被栅格化（文本不可再选择）。

Provider defaults:
- OpenAI: `openai:gpt-5.2`
- Gemini: `gemini:gemini-2.5-flash`
- Claude: `claude:claude-sonnet-4-5-20250929`

Provider model APIs:
- [OpenAI Models API](https://platform.openai.com/docs/api-reference/models)
- [Gemini Models API](https://ai.google.dev/api/models)
- [Anthropic Models API](https://docs.anthropic.com/en/api/models)

## Settings

设置文件加载优先级（从高到低）：

1. `~/.llm-translator-rust/settings.local.toml`
2. `~/.llm-translator-rust/settings.toml`
3. `./settings.local.toml`
4. `./settings.toml`

也可使用 `-r/--read-settings` 额外加载一个本地 TOML 文件（优先级最高）。

`settings.toml` 格式如下：
`system.languages` 应为 ISO 639-3 代码。

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

语言包位于 `src/languages/<iso-639-3>.toml`。
`system.languages` 的第一项用于 `--show-enabled-languages` 的标签显示。

示例（Japanese）：

```toml
[translate.iso_country_lang.jpn]
jpn = "日本語"
eng = "英語"

```

## Environment variables

- OpenAI: `OPENAI_API_KEY`
- Gemini: `GEMINI_API_KEY` 或 `GOOGLE_API_KEY`
- Claude: `ANTHROPIC_API_KEY`

`-k/--key` 会覆盖环境变量。

## Options

| Flag | Long | Description | Default |
| --- | --- | --- | --- |
| `-l` | `--lang` | 目标语言 | `en` |
| `-m` | `--model` | 提供方/模型选择 | (auto) |
| `-k` | `--key` | API key 覆盖 | (env) |
| `-f` | `--formal` | 语气/风格键（来自 `settings.toml` `[formally]`） | `formal` |
| `-L` | `--source-lang` | 源语言（ISO 639-1/2/3 或 `auto`） | `auto` |
| `-s` | `--slang` | 适当时加入俚语 | `false` |
| `-d` | `--data` | 附件文件（image/doc/docx/pptx/xlsx/pdf/txt/md/html/json/yaml/po/xml/js/ts/tsx/mermaid/audio） |  |
| `-M` | `--data-mime` | `--data`（或 stdin）MIME（`auto`, `image/*`, `pdf`, `doc`, `docx`, `docs`, `pptx`, `xlsx`, `txt`, `md`, `markdown`, `html`, `json`, `yaml`, `po`, `xml`, `js`, `ts`, `tsx`, `mermaid`, `mp3`, `wav`, `m4a`, `flac`, `ogg`） | `auto` |
|  | `--with-commentout` | 翻译注释内容（HTML/YAML/PO） |  |
|  | `--show-enabled-languages` | 显示启用的翻译语言 |  |
|  | `--show-enabled-styles` | 显示可用风格键 |  |
|  | `--show-models-list` | 显示缓存模型列表（provider:model） |  |
|  | `--show-whisper-models` | 显示 whisper 模型名 |  |
|  | `--pos` | 词典式输出（词性/活用） |  |
|  | `--correction` | 校对输入并给出指正 |  |
|  | `--details` | Detailed translations across all formal styles |  |
|  | `--show-histories` | 显示翻译历史 |  |
|  | `--with-using-tokens` | 在输出追加 token 使用量 |  |
|  | `--with-using-model` | 在输出追加模型名 |  |
|  | `--force` | MIME 判定不确定时强制按文本翻译 |  |
|  | `--debug-ocr` | 输出 OCR 调试叠加/JSON |  |
|  | `--whisper-model` | Whisper 模型名或路径 |  |
|  | `--overwrite` | 覆盖写入（备份在 `~/.llm-translator-rust/backup`） |  |
|  | `--directory-translation-threads` | 目录翻译并发数 |  |
|  | `--ignore-translation-file` | 目录翻译忽略规则（gitignore 风格） |  |
| `-o` | `--out` | 翻译输出路径 |  |
|  | `--verbose` | 详细日志 |  |
| `-i` | `--interactive` | 交互模式 |  |
| `-r` | `--read-settings` | 读取额外设置 TOML 文件 |  |
|  | `--server` | 启动 HTTP 服务器（`ADDR` 默认读取设置或 `0.0.0.0:11223`） |  |
|  | `--mcp` | Start MCP server over stdio |  |
| `-h` | `--help` | 帮助 |  |

## Server mode

启动 HTTP 服务器：

```bash
llm-translator-rust --server
llm-translator-rust --server 0.0.0.0:11223
```

服务器设置在 `settings.toml` 的 `[server]`：

```toml
[server]
host = "0.0.0.0"
port = 11223
tmp_dir = "/tmp/llm-translator-rust"
```

请求为 JSON `POST /translate`（`text` 或 `data` 路径二选一）：

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

校正请求：

```json
{
  "text": "This is pen",
  "correction": true,
  "source_lang": "en"
}
```

响应（文本）：

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

校正响应（文本）：

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

响应（二进制）：

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

当 `data` 为目录时，`contents` 会包含多条结果。

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

- C 头文件：`ext/llm_translator_rust.h`。
- 函数返回堆字符串；使用 `llm_ext_free_string` 释放。
- 调用失败时，使用 `llm_ext_last_error_message` 获取错误信息。

## Notes

- API 错误（含余额不足）会直接返回提供方的错误信息。
- 使用 `-h/--help` 查看最新选项。

## Formality values (default settings)

- `casual`: 随意、日常语气
- `formal`: 礼貌、正式语气
- `loose`: 轻松、随意措辞
- `academic`: 学术、论文风格
- `gal`: 俏皮辣妹风
- `yankee`: 粗犷不良风
- `otaku`: 宅系用语与语感
- `elderly`: 温和长者语气
- `aristocrat`: 高雅贵族风
- `samurai`: 古风武士语气
- `braille`: 输出为 Unicode 盲文
- `morse`: 输出为国际摩斯电码
- `engineer`: 精确技术风格