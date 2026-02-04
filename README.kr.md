# llm-translator-rust

한국어 | [English](README.md) | [日本語](README.ja.md) | [中文](README.cn.md) | [Français](README.fr.md) | [Deutsch](README.ge.md) | [Italiano](README.it.md) | [Русский](README.ru.md) | [UK English](README.uk.md)

LLM 툴 호출을 사용하는 작은 CLI 번역기이며, 항상 stdin에서 입력을 읽습니다.

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

- stdin에서 입력을 읽고 번역 결과를 출력합니다.
- 도구 호출 JSON만 사용합니다(자유 형식 출력 없음).
- Provider: OpenAI, Gemini, Claude.
- 모델 목록은 각 Provider의 Models API로 가져와 24시간 캐시합니다.

## Installation

다음 중 하나를 선택하세요:

### 1) GitHub Releases에서 다운로드

Releases 페이지에서 받을 수 있습니다:
[GitHub Releases](https://github.com/m3m0r7/llm-translator-rust/releases/latest)

자산 이름은 `llm-translator-rust-<os>-<arch>` 형식입니다(예: `llm-translator-rust-macos-aarch64`).

### 2) cargo로 설치(전역)

```bash
cargo install --git https://github.com/m3m0r7/llm-translator-rust --locked
```

### 3) 소스에서 빌드(git clone)

```bash
git clone https://github.com/m3m0r7/llm-translator-rust
cd llm-translator-rust
make
sudo make install
```

바이너리는 다음 경로에 있습니다:

```
./target/release/llm-translator-rust
```

참고:
- macOS/Linux 기본 경로: `/usr/local/bin`(필요 시 `sudo make install`)
- Windows(MSYS/Git Bash): `%USERPROFILE%/.cargo/bin`
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

`--data`가 디렉터리를 가리키면 재귀적으로 순회하며 지원되는 파일을 번역합니다.
출력 디렉터리는 상대 경로 구조를 유지합니다.

```bash
llm-translator-rust --data ./docs -l ja
# Output: ./docs_translated (default suffix; configurable via settings.toml)
```

메모:
- `--data-mime`는 디렉터리 내 모든 파일에 적용됩니다. 혼합 타입이면 `auto`를 유지하세요.
- 읽을 수 없거나 MIME을 감지할 수 없는 파일은 실패로 보고되며, 지원되지 않는 타입은 건너뜁니다.
- `--force`로 불확실/미확정 MIME을 텍스트로 처리할 수 있습니다.
- 디렉터리 번역은 병렬 실행(기본 3 스레드). `--directory-translation-threads` 또는 `settings.toml`로 변경.
- `--ignore-translation-file` 또는 ignore 파일로 제외 가능(기본 `.llm-translation-rust-ignore`, `settings.toml`에서 변경).
  패턴은 `.gitignore` 규칙(`*`, `**`, `!`, 주석)을 따릅니다.
- ignore 규칙은 `--data`가 디렉터리일 때만 적용됩니다.
- `--out`으로 출력 디렉터리를 지정할 수 있습니다.
- 디렉터리 번역이 실패하면 원본 파일을 출력 디렉터리에 복사합니다.

## Overwrite mode (--overwrite)

`--overwrite`는 `--data`로 지정된 파일/디렉터리를 제자리에서 덮어씁니다.
쓰기 전 백업은 `~/.llm-translator-rust/backup`에 저장됩니다.
보관 기간은 `settings.toml`의 `[system].backup_ttl_days`로 설정됩니다(기본 30일).

```bash
llm-translator-rust --data ./docs --overwrite -l ja
llm-translator-rust --data ./slide.pdf --overwrite -l en
```

## Output path (--out)

`--out`은 파일/디렉터리 번역의 출력 경로를 지정합니다.
`--overwrite`와 함께 사용할 수 없습니다.

```bash
llm-translator-rust --data ./docs -l ja --out ./outdir
llm-translator-rust --data ./slide.pdf -l en --out ./translated.pdf
```

## Dictionary (--pos)

`--pos`는 입력 단어에 대한 사전형 정보를 반환합니다.

사용법:

```
echo 猫 | llm-translator-rust --pos -l en
```

예시 출력(라벨은 source language 기준):

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

`--correction`은 입력을 교정하고 수정 사항을 소스 언어로 알려줍니다.

사용법:

```
echo "This is pen" | llm-translator-rust --correction --source-lang en
```

예시 출력:

```
This is a pen
        -

Correction reasons:
- English requires a/an before a countable noun
```

- 라벨은 소스 언어로 로컬라이즈됩니다.
- `Reading`은 번역의 발음을 소스 언어의 일반적인 문자 체계로 표시합니다.
- `Alternatives`는 다른 번역 후보와 발음을 나열합니다.
- `Usage`와 예문의 원문은 소스 언어로 작성됩니다.
- 예문에는 번역 또는 대체 번역이 포함됩니다.

## Audio translation

오디오 파일은 `whisper-rs`로 전사한 뒤 LLM이 번역하고, 다시 음성 합성합니다.

- 지원 오디오: mp3, wav, m4a, flac, ogg
- `ffmpeg` 필요
- Whisper 모델 필요(첫 실행 시 자동 다운로드)
- TTS는 macOS `say` 또는 Linux `espeak`

모델 선택:

```
llm-translator-rust --show-whisper-models
llm-translator-rust --whisper-model small -d ./voice.mp3 -l en
```

`LLM_TRANSLATOR_WHISPER_MODEL`을 모델 이름 또는 파일 경로로 설정할 수도 있습니다.
`settings.toml`의 `[whisper] model` 또는 `--whisper-model`이 이를 덮어씁니다.

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

- 기본 Provider 우선순위: OpenAI → Gemini → Claude(첫 번째로 찾은 API 키).
- `-m/--model` 사용법:
  - Provider만: `openai`, `gemini`, `claude`
  - Provider+모델: `openai:MODEL_ID`
  - 모델 지정 시 Provider 접두사를 반드시 포함.
- 기본은 Provider 기본 모델, 없으면 채팅 가능한 첫 모델을 사용.
- 소스/대상 언어는 ISO 639-1 또는 ISO 639-2/3 (예: `ja`, `en`, `jpn`, `eng`). 소스는 `auto` 가능.
- 중국어 변형: `zho-hans`(간체), `zho-hant`(번체).
- 언어 검증은 Wikipedia ISO 639 목록 사용: https://en.wikipedia.org/wiki/List_of_ISO_639_language_codes
- 모델 목록은 24시간 캐시.
- 캐시 경로:
  - `~/.llm-translator/.cache/meta.json`(fallback: `./.llm-translator/.cache/meta.json`)
- `--show-models-list`는 `provider:model` 형식으로 출력.
- `--show-whisper-models`는 whisper 모델 목록 출력.
- `--pos`는 사전형 상세 정보 출력.
- `--correction`은 교정 결과와 이유 출력(소스 언어).
- `--whisper-model`은 오디오 전사 모델 선택.
- `--model`이 없으면 `meta.json`의 `lastUsingModel`을 우선 사용.
- 히스토리는 `meta.json`에 저장, 결과는 `~/.llm-translator-rust/.cache/dest/<md5>`.
- 이미지/PDF는 OCR(tesseract) 후 LLM으로 정규화하고 번호 오버레이+풋터 목록으로 렌더링.
- Office(docx/xlsx/pptx)는 XML 텍스트 노드를 번역해 재작성.
- 출력 MIME은 입력과 동일.
- OCR 언어는 `--source-lang`과 `--lang`에서 추론.
- `tesseract --list-langs`로 설치된 OCR 언어 확인.
- PDF OCR은 렌더러 필요(`mutool` 또는 poppler의 `pdftoppm`).
- PDF 출력은 래스터라이즈됩니다(텍스트 선택 불가).

Provider defaults:
- OpenAI: `openai:gpt-5.2`
- Gemini: `gemini:gemini-2.5-flash`
- Claude: `claude:claude-sonnet-4-5-20250929`

Provider model APIs:
- [OpenAI Models API](https://platform.openai.com/docs/api-reference/models)
- [Gemini Models API](https://ai.google.dev/api/models)
- [Anthropic Models API](https://docs.anthropic.com/en/api/models)

## Settings

설정 파일 로드 순서(우선순위 높은 순):

1. `~/.llm-translator-rust/settings.local.toml`
2. `~/.llm-translator-rust/settings.toml`
3. `./settings.local.toml`
4. `./settings.toml`

`-r/--read-settings`로 추가 로컬 TOML을 최우선으로 로드할 수 있습니다.

`settings.toml` 형식:
`system.languages`는 ISO 639-3 코드여야 합니다.

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

언어 팩은 `src/languages/<iso-639-3>.toml`에 있습니다.
`system.languages`의 첫 항목은 `--show-enabled-languages`의 라벨 표시용입니다.

예시(Japanese):

```toml
[translate.iso_country_lang.jpn]
jpn = "日本語"
eng = "英語"

```

## Environment variables

- OpenAI: `OPENAI_API_KEY`
- Gemini: `GEMINI_API_KEY` 또는 `GOOGLE_API_KEY`
- Claude: `ANTHROPIC_API_KEY`

`-k/--key`는 환경 변수를 덮어씁니다.

## Options

| Flag | Long | Description | Default |
| --- | --- | --- | --- |
| `-l` | `--lang` | 대상 언어 | `en` |
| `-m` | `--model` | Provider/모델 선택 | (auto) |
| `-k` | `--key` | API 키 지정 | (env) |
| `-f` | `--formal` | 스타일 키(`settings.toml` `[formally]`) | `formal` |
| `-L` | `--source-lang` | 소스 언어(ISO 639-1/2/3 또는 `auto`) | `auto` |
| `-s` | `--slang` | 적절한 경우 슬랭 포함 | `false` |
| `-d` | `--data` | 첨부 파일(image/doc/docx/pptx/xlsx/pdf/txt/md/html/json/yaml/po/xml/js/ts/tsx/mermaid/audio) |  |
| `-M` | `--data-mime` | `--data`(또는 stdin) MIME | `auto` |
|  | `--with-commentout` | 주석 번역(HTML/YAML/PO) |  |
|  | `--show-enabled-languages` | 활성 언어 표시 |  |
|  | `--show-enabled-styles` | 활성 스타일 표시 |  |
|  | `--show-models-list` | 캐시된 모델 목록 표시 |  |
|  | `--show-whisper-models` | whisper 모델 목록 표시 |  |
|  | `--pos` | 사전형 출력(POS/활용) |  |
|  | `--correction` | 교정 결과 출력 |  |
|  | `--details` | Detailed translations across all formal styles |  |
|  | `--show-histories` | 번역 이력 표시 |  |
|  | `--with-using-tokens` | 토큰 사용량 추가 |  |
|  | `--with-using-model` | 모델명 추가 |  |
|  | `--force` | MIME 판정이 불확실할 때 텍스트로 처리 |  |
|  | `--debug-ocr` | OCR 디버그 오버레이/JSON 출력 |  |
|  | `--whisper-model` | Whisper 모델 이름/경로 |  |
|  | `--overwrite` | 파일 덮어쓰기(백업 `~/.llm-translator-rust/backup`) |  |
|  | `--directory-translation-threads` | 디렉터리 번역 병렬 수 |  |
|  | `--ignore-translation-file` | 제외 패턴(gitignore 스타일) |  |
| `-o` | `--out` | 출력 경로 |  |
|  | `--verbose` | 상세 로그 |  |
| `-i` | `--interactive` | 인터랙티브 모드 |  |
| `-r` | `--read-settings` | 추가 설정 TOML 읽기 |  |
|  | `--server` | HTTP 서버 시작(`ADDR` 기본: settings 또는 `0.0.0.0:11223`) |  |
|  | `--mcp` | Start MCP server over stdio |  |
| `-h` | `--help` | 도움말 |  |

## Server mode

HTTP 서버 시작:

```bash
llm-translator-rust --server
llm-translator-rust --server 0.0.0.0:11223
```

서버 설정은 `settings.toml`의 `[server]`에서 가능합니다:

```toml
[server]
host = "0.0.0.0"
port = 11223
tmp_dir = "/tmp/llm-translator-rust"
```

요청은 JSON `POST /translate`(`text` 또는 `data`)입니다:

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

교정 요청:

```json
{
  "text": "This is pen",
  "correction": true,
  "source_lang": "en"
}
```

응답(텍스트):

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

교정 응답(텍스트):

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

응답(바이너리):

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

`data`가 디렉터리면 `contents`에 여러 항목이 반환됩니다.

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

- C 헤더: `ext/llm_translator_rust.h`.
- 함수는 힙 문자열을 반환하므로 `llm_ext_free_string`로 해제하세요.
- 실패 시 `llm_ext_last_error_message`로 메시지를 얻습니다.

## Notes

- API 오류(쿼터 부족 포함)는 Provider 메시지로 전달됩니다.
- 최신 옵션은 `-h/--help`로 확인하세요.

## Formality values (default settings)

- `casual`: 일상적인 캐주얼 톤
- `formal`: 공손하고 포멀한 톤
- `loose`: 느슨하고 편한 표현
- `academic`: 학술/논문 스타일
- `gal`: 장난스럽고 갸루 톤
- `yankee`: 거친 불량 스타일
- `otaku`: 오타쿠 어감
- `elderly`: 부드러운 어르신 톤
- `aristocrat`: 귀족적 톤
- `samurai`: 고풍스러운 사무라이 톤
- `braille`: 유니코드 점자 출력
- `morse`: 국제 모스 부호 출력
- `engineer`: 정확한 기술 톤