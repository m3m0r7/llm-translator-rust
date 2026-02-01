use anyhow::{anyhow, Context, Result};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tempfile::tempdir;
use tracing::info;
use whisper_rs::{
    get_lang_str, FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters,
};

use crate::data;
use crate::languages::{map_lang_for_espeak, map_lang_for_whisper};
use crate::providers::Provider;
use crate::{TranslateOptions, Translator};

use crate::attachments::AttachmentTranslation;

pub(crate) async fn translate_audio<P: Provider + Clone>(
    data: &data::DataAttachment,
    translator: &Translator<P>,
    options: &TranslateOptions,
) -> Result<AttachmentTranslation> {
    ensure_command("ffmpeg", "audio translation requires ffmpeg")?;
    info!("audio: decoding with ffmpeg");

    let dir = tempdir().with_context(|| "failed to create temp dir for audio")?;
    let input_ext = data::extension_from_mime(&data.mime).unwrap_or("bin");
    let input_path = dir.path().join(format!("input.{}", input_ext));
    fs::write(&input_path, &data.bytes).with_context(|| "failed to write audio input")?;

    let wav_path = dir.path().join("input.wav");
    run_ffmpeg(&[
        "-y",
        "-i",
        input_path.to_string_lossy().as_ref(),
        "-ar",
        "16000",
        "-ac",
        "1",
        wav_path.to_string_lossy().as_ref(),
    ])
    .with_context(|| "failed to decode audio with ffmpeg")?;

    let transcript = transcribe_audio(
        &wav_path,
        &options.source_lang,
        translator.settings().whisper_model.as_deref(),
    )
    .await?;
    let transcript = transcript.trim();
    if transcript.is_empty() {
        return Err(anyhow!("no speech detected in audio"));
    }

    info!("audio: transcribed {} chars", transcript.chars().count());
    let exec = translator.exec(transcript, options.clone()).await?;
    let translated = exec.text.trim();
    if translated.is_empty() {
        return Err(anyhow!("translation returned empty text"));
    }

    let tts_wav = dir.path().join("tts.wav");
    info!("audio: synthesizing speech");
    synthesize_speech(translated, &options.lang, &tts_wav)?;

    let out_ext = data::extension_from_mime(&data.mime).unwrap_or("mp3");
    let output_path = dir.path().join(format!("output.{}", out_ext));
    run_ffmpeg(&[
        "-y",
        "-i",
        tts_wav.to_string_lossy().as_ref(),
        output_path.to_string_lossy().as_ref(),
    ])
    .with_context(|| "failed to encode translated audio")?;

    let bytes = fs::read(&output_path).with_context(|| "failed to read translated audio")?;

    Ok(AttachmentTranslation {
        bytes,
        mime: data.mime.clone(),
        model: exec.model,
        usage: exec.usage,
    })
}

async fn transcribe_audio(
    wav_path: &Path,
    source_lang: &str,
    whisper_model_override: Option<&str>,
) -> Result<String> {
    let forced_lang = resolve_forced_lang(source_lang);
    let outcome = transcribe_audio_with_params(
        wav_path,
        forced_lang.as_deref(),
        whisper_model_override,
        false,
    )
    .await?;
    if !outcome.text.trim().is_empty() {
        return Ok(outcome.text);
    }
    if forced_lang.is_none() {
        if let Some(detected) = outcome.detected_lang.as_deref() {
            let retry = transcribe_audio_with_params(
                wav_path,
                Some(detected),
                whisper_model_override,
                true,
            )
            .await?;
            if !retry.text.trim().is_empty() {
                return Ok(retry.text);
            }
        }
    }

    info!("audio: no speech detected, retrying with normalization");
    let dir = wav_path
        .parent()
        .ok_or_else(|| anyhow!("invalid wav path"))?;
    let normalized_path = dir.join("input_norm.wav");
    run_ffmpeg(&[
        "-y",
        "-i",
        wav_path.to_string_lossy().as_ref(),
        "-af",
        "dynaudnorm",
        normalized_path.to_string_lossy().as_ref(),
    ])
    .with_context(|| "failed to normalize audio")?;

    let outcome = transcribe_audio_with_params(
        &normalized_path,
        forced_lang.as_deref(),
        whisper_model_override,
        true,
    )
    .await?;
    if !outcome.text.trim().is_empty() {
        return Ok(outcome.text);
    }
    if forced_lang.is_none() {
        if let Some(detected) = outcome.detected_lang.as_deref() {
            let retry = transcribe_audio_with_params(
                &normalized_path,
                Some(detected),
                whisper_model_override,
                true,
            )
            .await?;
            if !retry.text.trim().is_empty() {
                return Ok(retry.text);
            }
        }
    }

    info!("audio: still empty, retrying with normalization + gain");
    let boosted_path = dir.join("input_boost.wav");
    run_ffmpeg(&[
        "-y",
        "-i",
        wav_path.to_string_lossy().as_ref(),
        "-af",
        "dynaudnorm,volume=6",
        boosted_path.to_string_lossy().as_ref(),
    ])
    .with_context(|| "failed to normalize audio with gain")?;

    let outcome = transcribe_audio_with_params(
        &boosted_path,
        forced_lang.as_deref(),
        whisper_model_override,
        true,
    )
    .await?;
    if !outcome.text.trim().is_empty() {
        return Ok(outcome.text);
    }
    if forced_lang.is_none() {
        if let Some(detected) = outcome.detected_lang.as_deref() {
            let retry = transcribe_audio_with_params(
                &boosted_path,
                Some(detected),
                whisper_model_override,
                true,
            )
            .await?;
            if !retry.text.trim().is_empty() {
                return Ok(retry.text);
            }
        }
    }

    Ok(outcome.text)
}

struct TranscribeOutcome {
    text: String,
    detected_lang: Option<String>,
}

async fn transcribe_audio_with_params(
    wav_path: &Path,
    forced_lang: Option<&str>,
    whisper_model_override: Option<&str>,
    relaxed: bool,
) -> Result<TranscribeOutcome> {
    let model = whisper_model_path(whisper_model_override).await?;
    let audio = read_wav_mono_f32(wav_path)?;

    let model_path = model.to_string_lossy();
    let ctx =
        WhisperContext::new_with_params(model_path.as_ref(), WhisperContextParameters::default())
            .with_context(|| "failed to load whisper model")?;
    let mut state = ctx
        .create_state()
        .with_context(|| "failed to init whisper state")?;
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_n_threads(num_cpus::get() as i32);
    params.set_translate(false);
    if relaxed {
        params.set_suppress_blank(false);
        params.set_suppress_non_speech_tokens(false);
        params.set_no_speech_thold(1.0);
        params.set_logprob_thold(-5.0);
        params.set_temperature(0.4);
        params.set_temperature_inc(0.2);
        params.set_no_timestamps(true);
        params.set_single_segment(true);
    }
    if let Some(lang) = forced_lang {
        params.set_language(Some(lang));
    } else {
        params.set_detect_language(true);
    }

    state
        .full(params, &audio[..])
        .with_context(|| "whisper transcription failed")?;

    let detected_lang = state
        .full_lang_id_from_state()
        .ok()
        .and_then(|id| get_lang_str(id))
        .map(|value: &str| value.to_string());
    let num_segments = state
        .full_n_segments()
        .with_context(|| "failed to read segments")?;
    let mut parts = Vec::new();
    for idx in 0..num_segments {
        let text = state
            .full_get_segment_text(idx)
            .with_context(|| "failed to read segment text")?;
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            parts.push(trimmed.to_string());
        }
    }
    Ok(TranscribeOutcome {
        text: parts.join(" "),
        detected_lang,
    })
}

fn resolve_forced_lang(source_lang: &str) -> Option<String> {
    if source_lang.trim().is_empty() || source_lang.eq_ignore_ascii_case("auto") {
        return None;
    }
    map_lang_for_whisper(source_lang).map(|value| value.to_string())
}

fn synthesize_speech(text: &str, target_lang: &str, out_wav: &Path) -> Result<()> {
    let text = text.replace('\n', " ");
    if command_exists("say") {
        #[cfg(target_os = "macos")]
        {
            std::env::set_var("OS_ACTIVITY_MODE", "disable");
            std::env::set_var("OS_ACTIVITY_DT_MODE", "0");
        }
        let aiff_path = out_wav.with_extension("aiff");
        let status = Command::new("say")
            .arg("-o")
            .arg(&aiff_path)
            .arg(&text)
            .env("OS_ACTIVITY_MODE", "disable")
            .env("OS_ACTIVITY_DT_MODE", "0")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .with_context(|| "failed to run say")?;
        if !status.success() {
            return Err(anyhow!("say failed to synthesize audio"));
        }
        run_ffmpeg(&[
            "-y",
            "-i",
            aiff_path.to_string_lossy().as_ref(),
            out_wav.to_string_lossy().as_ref(),
        ])
        .with_context(|| "failed to convert say output")?;
        return Ok(());
    }

    if command_exists("espeak") {
        let voice = map_lang_for_espeak(target_lang).unwrap_or("en");
        let status = Command::new("espeak")
            .arg("-v")
            .arg(voice)
            .arg("-w")
            .arg(out_wav)
            .arg(&text)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .with_context(|| "failed to run espeak")?;
        if !status.success() {
            return Err(anyhow!("espeak failed to synthesize audio"));
        }
        return Ok(());
    }

    Err(anyhow!(
        "no TTS engine found (install macOS 'say' or Linux 'espeak')"
    ))
}

pub(crate) fn command_exists(cmd: &str) -> bool {
    let path = Path::new(cmd);
    if path.components().count() > 1 {
        return is_executable(path);
    }

    let path_var = match env::var_os("PATH") {
        Some(value) => value,
        None => return false,
    };

    #[cfg(windows)]
    let candidates = windows_command_candidates(cmd);
    #[cfg(not(windows))]
    let candidates = vec![cmd.to_string()];

    for dir in env::split_paths(&path_var) {
        for candidate in &candidates {
            if is_executable(&dir.join(candidate)) {
                return true;
            }
        }
    }
    false
}

fn is_executable(path: &Path) -> bool {
    let metadata = match fs::metadata(path) {
        Ok(value) => value,
        Err(_) => return false,
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(windows)]
fn windows_command_candidates(cmd: &str) -> Vec<String> {
    let path = Path::new(cmd);
    if path.extension().is_some() {
        return vec![cmd.to_string()];
    }
    let pathext = env::var_os("PATHEXT").unwrap_or_else(|| ".EXE;.CMD;.BAT;.COM".into());
    pathext
        .to_string_lossy()
        .split(';')
        .filter(|ext| !ext.is_empty())
        .map(|ext| format!("{}{}", cmd, ext.to_lowercase()))
        .collect()
}

fn ensure_command(cmd: &str, message: &str) -> Result<()> {
    if command_exists(cmd) {
        Ok(())
    } else {
        Err(anyhow!("{}", message))
    }
}

fn run_ffmpeg(args: &[&str]) -> Result<()> {
    let output = Command::new("ffmpeg")
        .args(args)
        .output()
        .with_context(|| "failed to run ffmpeg")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("ffmpeg failed: {}", stderr.trim()));
    }
    Ok(())
}

const WHISPER_MODEL_BASE_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

async fn whisper_model_path(override_model: Option<&str>) -> Result<PathBuf> {
    if let Some(value) = override_model {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            let path = PathBuf::from(trimmed);
            if path.exists() {
                return Ok(path);
            }
            if let Some(model) = normalize_model_name(trimmed) {
                return ensure_whisper_model(&model).await;
            }
        }
    }
    if let Ok(path) = std::env::var("LLM_TRANSLATOR_WHISPER_MODEL") {
        let path = path.trim();
        if !path.is_empty() {
            let path = PathBuf::from(path);
            if path.exists() {
                return Ok(path);
            }
            if let Some(model) = normalize_model_name(path.to_string_lossy().as_ref()) {
                return ensure_whisper_model(&model).await;
            }
        }
    }
    if let Ok(path) = std::env::var("WHISPER_CPP_MODEL") {
        let path = path.trim();
        if !path.is_empty() {
            let path = PathBuf::from(path);
            if path.exists() {
                return Ok(path);
            }
            if let Some(model) = normalize_model_name(path.to_string_lossy().as_ref()) {
                return ensure_whisper_model(&model).await;
            }
        }
    }

    ensure_whisper_model("base").await
}

async fn ensure_whisper_model(model: &str) -> Result<PathBuf> {
    let normalized = normalize_model_name(model).unwrap_or_else(|| "base".to_string());
    let dest = default_model_path(&normalized)?;
    if dest.exists() {
        return Ok(dest);
    }

    let url = whisper_model_url(&normalized)?;
    info!("whisper model not found; downloading {} ...", normalized);
    download_whisper_model(&url, &dest).await?;
    Ok(dest)
}

fn default_model_path(model: &str) -> Result<PathBuf> {
    let file = format!("ggml-{}.bin", model);
    if let Ok(home) = std::env::var("HOME") {
        let home = home.trim();
        if !home.is_empty() {
            return Ok(Path::new(home)
                .join(".llm-translator-rust/.cache/whisper")
                .join(file));
        }
    }
    Ok(Path::new(".llm-translator-rust/.cache/whisper").join(file))
}

fn whisper_model_url(model: &str) -> Result<String> {
    let file = format!("ggml-{}.bin", model);
    Ok(format!("{}/{}", WHISPER_MODEL_BASE_URL, file))
}

fn normalize_model_name(input: &str) -> Option<String> {
    let raw = input.trim().to_lowercase();
    if raw.is_empty() {
        return None;
    }
    let trimmed = raw
        .strip_prefix("ggml-")
        .unwrap_or(raw.as_str())
        .strip_suffix(".bin")
        .unwrap_or(raw.as_str());

    let allowed = [
        "tiny",
        "base",
        "small",
        "medium",
        "large",
        "large-v2",
        "large-v3",
        "tiny.en",
        "base.en",
        "small.en",
        "medium.en",
    ];
    if allowed.contains(&trimmed) {
        return Some(trimmed.to_string());
    }
    None
}

async fn download_whisper_model(url: &str, dest: &Path) -> Result<()> {
    let dir = dest.parent().ok_or_else(|| anyhow!("invalid model path"))?;
    fs::create_dir_all(dir)
        .with_context(|| format!("failed to create model dir: {}", dir.display()))?;

    let response = reqwest::get(url)
        .await
        .with_context(|| format!("failed to download whisper model: {}", url))?;
    if !response.status().is_success() {
        return Err(anyhow!(
            "failed to download whisper model: {} (status {})",
            url,
            response.status()
        ));
    }

    let tmp = dest.with_extension("bin.part");
    let mut file = fs::File::create(&tmp)
        .with_context(|| format!("failed to write model: {}", tmp.display()))?;
    let mut stream = response.bytes_stream();
    use futures_util::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.with_context(|| "failed to read model bytes")?;
        std::io::Write::write_all(&mut file, &chunk)?;
    }
    fs::rename(&tmp, dest)
        .with_context(|| format!("failed to finalize model: {}", dest.display()))?;
    Ok(())
}

fn read_wav_mono_f32(path: &Path) -> Result<Vec<f32>> {
    let mut reader = hound::WavReader::open(path)
        .with_context(|| format!("failed to open wav: {}", path.display()))?;
    let spec = reader.spec();
    let channels = spec.channels as usize;
    if channels == 0 {
        return Err(anyhow!("wav has no channels"));
    }

    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().map(|s| s.unwrap_or(0.0)).collect(),
        hound::SampleFormat::Int => {
            let bits = spec.bits_per_sample;
            let max = (1i64 << (bits - 1)) as f32;
            if bits <= 16 {
                reader
                    .samples::<i16>()
                    .map(|s| s.unwrap_or(0) as f32 / max)
                    .collect()
            } else {
                reader
                    .samples::<i32>()
                    .map(|s| s.unwrap_or(0) as f32 / max)
                    .collect()
            }
        }
    };

    if channels == 1 {
        return Ok(samples);
    }

    let mut mono = Vec::with_capacity(samples.len() / channels);
    for chunk in samples.chunks(channels) {
        let sum: f32 = chunk.iter().sum();
        mono.push(sum / channels as f32);
    }
    Ok(mono)
}
