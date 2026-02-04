use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Parser, Debug)]
#[command(name = "llm-translator-build")]
#[command(about = "Build helper for llm-translator-rust", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Build(BuildArgs),
    Install(InstallArgs),
}

#[derive(Parser, Debug)]
struct BuildArgs {
    /// Path to write build_env.toml
    #[arg(
        long = "env-path",
        default_value = "build/build_env.toml",
        alias = "env"
    )]
    env_path: PathBuf,

    /// Data directory for runtime assets
    #[arg(long = "data-directory", alias = "data-dir", alias = "base-directory")]
    data_dir: Option<String>,

    /// Directory that contains the built binary
    #[arg(long = "bin-directory", alias = "bin-dir")]
    bin_dir: Option<String>,

    /// Runtime directory (destination for make install)
    #[arg(
        long = "runtime-directory",
        alias = "runtime-dir",
        alias = "install-directory"
    )]
    runtime_dir: Option<String>,

    /// Config directory (settings.toml lives here)
    #[arg(long = "config-directory", alias = "config-dir")]
    config_dir: Option<String>,

    /// Path to settings.toml (overrides config directory)
    #[arg(long = "settings-file", alias = "settings")]
    settings_file: Option<String>,

    /// Project root (used for cargo build and relative paths)
    #[arg(long)]
    project_dir: Option<PathBuf>,

    /// Build target triple (e.g. x86_64-unknown-linux-gnu)
    #[arg(long)]
    target: Option<String>,

    /// Builder command (cargo or cross)
    #[arg(long, default_value = "cargo")]
    builder: String,
}

#[derive(Parser, Debug)]
struct InstallArgs {
    /// Path to read build_env.toml
    #[arg(
        long = "env-path",
        default_value = "build/build_env.toml",
        alias = "env"
    )]
    env_path: PathBuf,

    /// Binary name (without extension)
    #[arg(long = "bin-name")]
    bin_name: String,

    /// Binary extension (e.g. .exe)
    #[arg(long = "bin-ext")]
    bin_ext: Option<String>,

    /// Project root (used for relative paths)
    #[arg(long)]
    project_dir: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct BuildEnv {
    #[serde(rename = "dataDirectory", alias = "baseDirectory")]
    data_directory: String,
    #[serde(rename = "binDirectory")]
    bin_directory: String,
    #[serde(rename = "runtimeDirectory", alias = "installDirectory")]
    runtime_directory: String,
    #[serde(rename = "configDirectory", default)]
    config_directory: String,
    #[serde(
        rename = "settingsFile",
        default,
        skip_serializing_if = "String::is_empty"
    )]
    settings_file: String,
}

const XDG_APP_DIR: &str = "llm-translator-rust";

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Build(args) => run_build(args),
        Commands::Install(args) => run_install(args),
    }
}

fn run_build(args: BuildArgs) -> Result<()> {
    let project_dir = resolve_project_dir(args.project_dir.clone())?;
    let env_path = resolve_path(&project_dir, &args.env_path);

    let build_env = build_env_from_args(&args);
    write_build_env(&env_path, &build_env)?;
    let project_env_path = project_dir.join("build/build_env.toml");
    if project_env_path != env_path {
        write_build_env(&project_env_path, &build_env)?;
    }

    let env_path_abs = env_path.canonicalize().unwrap_or_else(|_| env_path.clone());

    let builder = resolve_builder(&args.builder)?;
    let mut cmd = Command::new(builder);
    cmd.args(["build", "--release"])
        .env("BUILD_ENV_TOML_PATH", &env_path_abs)
        .current_dir(&project_dir);
    if let Some(target) = args.target.as_deref()
        && !target.trim().is_empty()
    {
        cmd.args(["--target", target.trim()]);
    }

    let status = cmd
        .status()
        .with_context(|| "failed to run build command")?;
    if !status.success() {
        return Err(anyhow!("build failed with status: {}", status));
    }

    Ok(())
}

fn run_install(args: InstallArgs) -> Result<()> {
    let project_dir = resolve_project_dir(args.project_dir.clone())?;
    let env_path = resolve_path(&project_dir, &args.env_path);
    let env = read_build_env(&env_path)?;

    let bin_dir = resolve_string_path(&project_dir, &env.bin_directory);
    let install_dir = resolve_string_path(&project_dir, &env.runtime_directory);
    let bin_ext = args
        .bin_ext
        .unwrap_or_else(|| default_bin_ext().to_string());
    let bin_name = args.bin_name.trim();
    if bin_name.is_empty() {
        return Err(anyhow!("--bin-name is required"));
    }

    let bin_file = format!("{}{}", bin_name, bin_ext);
    let bin_path = bin_dir.join(&bin_file);
    if !bin_path.exists() {
        return Err(anyhow!(
            "binary not found: {} (run `make` first)",
            bin_path.display()
        ));
    }

    fs::create_dir_all(&install_dir).with_context(|| {
        format!(
            "failed to create install directory: {}",
            install_dir.display()
        )
    })?;
    let install_path = install_dir.join(&bin_file);
    fs::copy(&bin_path, &install_path).with_context(|| {
        format!(
            "failed to copy {} to {}",
            bin_path.display(),
            install_path.display()
        )
    })?;

    let settings_status = ensure_settings_file(&env, &project_dir)?;
    let header_status = ensure_header_files(&env, &project_dir)?;

    println!("Installed to {}", install_path.display());
    if let Some(status) = settings_status {
        match status {
            SettingsStatus::Copied(path) => {
                println!("Settings copied to {}", path.display());
            }
            SettingsStatus::Exists(path) => {
                println!("Settings already exists: {}", path.display());
            }
        }
    }
    if let Some(status) = header_status {
        match status {
            HeaderStatus::Copied(path) => {
                println!("Headers copied to {}", path.display());
            }
            HeaderStatus::Exists(path) => {
                println!("Headers already exists: {}", path.display());
            }
        }
    }
    Ok(())
}

fn build_env_from_args(args: &BuildArgs) -> BuildEnv {
    let data_directory = args
        .data_dir
        .clone()
        .or_else(|| env_value("BUILD_ENV_DATA_DIR"))
        .or_else(|| env_value("BUILD_ENV_BASE_DIR"))
        .unwrap_or_else(default_data_dir);

    let bin_directory = args
        .bin_dir
        .clone()
        .or_else(|| env_value("BUILD_ENV_BIN_DIR"))
        .unwrap_or_else(|| default_bin_dir(args.target.as_deref()));

    let runtime_directory = args
        .runtime_dir
        .clone()
        .or_else(|| env_value("BUILD_ENV_RUNTIME_DIR"))
        .or_else(|| env_value("BUILD_ENV_INSTALL_DIR"))
        .or_else(prefix_install_dir)
        .unwrap_or_else(default_runtime_dir);

    let config_directory = args
        .config_dir
        .clone()
        .or_else(|| env_value("BUILD_ENV_CONFIG_DIR"))
        .unwrap_or_else(default_config_dir);

    let settings_file = args
        .settings_file
        .clone()
        .or_else(|| env_value("BUILD_ENV_SETTINGS_FILE"))
        .unwrap_or_default();

    BuildEnv {
        data_directory,
        bin_directory,
        runtime_directory,
        config_directory,
        settings_file,
    }
}

fn resolve_project_dir(project_dir: Option<PathBuf>) -> Result<PathBuf> {
    let dir = match project_dir {
        Some(path) => path,
        None => env::current_dir().with_context(|| "failed to read current dir")?,
    };
    Ok(dir)
}

fn resolve_path(base: &Path, path: &Path) -> PathBuf {
    let raw = path.to_string_lossy();
    let expanded = expand_path(&raw);
    let path = PathBuf::from(expanded);
    if path.is_absolute() {
        path
    } else {
        base.join(path)
    }
}

fn resolve_string_path(base: &Path, value: &str) -> PathBuf {
    let expanded = expand_path(value);
    let path = PathBuf::from(expanded);
    if path.is_absolute() {
        path
    } else {
        base.join(path)
    }
}

fn write_build_env(path: &Path, env: &BuildEnv) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("failed to create build env directory: {}", parent.display())
        })?;
    }
    let content = toml::to_string_pretty(env).with_context(|| "failed to encode build env")?;
    fs::write(path, content)
        .with_context(|| format!("failed to write build env: {}", path.display()))?;
    Ok(())
}

fn read_build_env(path: &Path) -> Result<BuildEnv> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read build env: {}", path.display()))?;
    let env = toml::from_str(&content).with_context(|| "failed to parse build env")?;
    Ok(env)
}

enum SettingsStatus {
    Copied(PathBuf),
    Exists(PathBuf),
}

enum HeaderStatus {
    Copied(PathBuf),
    Exists(PathBuf),
}

fn ensure_settings_file(env: &BuildEnv, project_dir: &Path) -> Result<Option<SettingsStatus>> {
    let Some(settings_path) = resolve_settings_path(env, project_dir) else {
        return Ok(None);
    };
    if settings_path.exists() {
        return Ok(Some(SettingsStatus::Exists(settings_path)));
    }
    if let Some(parent) = settings_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("failed to create settings directory: {}", parent.display())
        })?;
    }
    let source = project_dir.join("settings.toml");
    if !source.exists() {
        return Err(anyhow!(
            "settings.toml not found at project root: {}",
            source.display()
        ));
    }
    fs::copy(&source, &settings_path).with_context(|| {
        format!(
            "failed to copy settings.toml to {}",
            settings_path.display()
        )
    })?;
    Ok(Some(SettingsStatus::Copied(settings_path)))
}

fn ensure_header_files(env: &BuildEnv, project_dir: &Path) -> Result<Option<HeaderStatus>> {
    let source_dir = project_dir.join("ext");
    if !source_dir.exists() {
        return Ok(None);
    }
    let Some(data_dir) = resolve_data_dir(env, project_dir) else {
        return Ok(None);
    };
    fs::create_dir_all(&data_dir).with_context(|| {
        format!(
            "failed to create data directory for headers: {}",
            data_dir.display()
        )
    })?;
    let mut copied = false;
    let mut has_entries = false;
    for entry in fs::read_dir(&source_dir)
        .with_context(|| format!("failed to read header directory: {}", source_dir.display()))?
    {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        has_entries = true;
        let dest = data_dir.join(entry.file_name());
        if dest.exists() {
            continue;
        }
        fs::copy(entry.path(), &dest).with_context(|| {
            format!(
                "failed to copy header {} to {}",
                entry.path().display(),
                dest.display()
            )
        })?;
        copied = true;
    }
    if !has_entries {
        return Ok(None);
    }
    if copied {
        Ok(Some(HeaderStatus::Copied(data_dir)))
    } else {
        Ok(Some(HeaderStatus::Exists(data_dir)))
    }
}

fn resolve_settings_path(env: &BuildEnv, project_dir: &Path) -> Option<PathBuf> {
    let raw = env.settings_file.trim();
    if !raw.is_empty() {
        return Some(resolve_string_path(project_dir, raw));
    }
    let config_raw = env.config_directory.trim();
    if !config_raw.is_empty() {
        let dir = resolve_string_path(project_dir, config_raw);
        return Some(dir.join("settings.toml"));
    }
    if let Some(home) = xdg_config_home_for_install() {
        return Some(home.join(XDG_APP_DIR).join("settings.toml"));
    }
    if raw.is_empty() {
        if env.data_directory.trim().is_empty() {
            return None;
        }
        return Some(resolve_string_path(
            project_dir,
            &format!("{}/settings.toml", env.data_directory.trim()),
        ));
    }
    Some(resolve_string_path(project_dir, raw))
}

fn resolve_data_dir(env: &BuildEnv, project_dir: &Path) -> Option<PathBuf> {
    let raw = env.data_directory.trim();
    if !raw.is_empty() {
        return Some(resolve_string_path(project_dir, raw));
    }
    if let Some(home) = xdg_data_home_for_install() {
        return Some(home.join(XDG_APP_DIR));
    }
    None
}

fn env_value(key: &str) -> Option<String> {
    let value = env::var(key).ok()?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn prefix_install_dir() -> Option<String> {
    let prefix = env_value("PREFIX")?;
    let prefix = prefix.trim_end_matches('/');
    if prefix.is_empty() {
        None
    } else {
        Some(format!("{}/bin", prefix))
    }
}

fn default_runtime_dir() -> String {
    if cfg!(windows) {
        if let Some(profile) = env_value("USERPROFILE") {
            return Path::new(&profile)
                .join(".cargo")
                .join("bin")
                .to_string_lossy()
                .to_string();
        }
        return "~/.cargo/bin".to_string();
    }
    "$XDG_RUNTIME_DIR".to_string()
}

fn default_bin_dir(target: Option<&str>) -> String {
    if let Some(target) = target {
        let target = target.trim();
        if !target.is_empty() {
            return format!("target/{}/release", target);
        }
    }
    "target/release".to_string()
}

fn default_data_dir() -> String {
    "$XDG_DATA_HOME/llm-translator-rust".to_string()
}

fn default_config_dir() -> String {
    "$XDG_CONFIG_HOME/llm-translator-rust".to_string()
}

fn default_runtime_dir_fallback() -> String {
    if cfg!(windows) {
        if let Some(profile) = env_value("USERPROFILE") {
            return Path::new(&profile)
                .join(".cargo")
                .join("bin")
                .to_string_lossy()
                .to_string();
        }
        return "~/.cargo/bin".to_string();
    }
    "/usr/local/bin".to_string()
}

fn default_bin_ext() -> &'static str {
    if cfg!(windows) { ".exe" } else { "" }
}

fn expand_path(value: &str) -> String {
    let expanded = expand_env_prefix(value);
    expand_tilde(&expanded)
}

fn expand_env_prefix(value: &str) -> String {
    let data_home = xdg_data_home().map(|path| path.to_string_lossy().to_string());
    if let Some(replaced) = expand_named_prefix(value, "XDG_DATA_HOME", data_home) {
        return replaced;
    }
    let config_home = xdg_config_home().map(|path| path.to_string_lossy().to_string());
    if let Some(replaced) = expand_named_prefix(value, "XDG_CONFIG_HOME", config_home) {
        return replaced;
    }
    let state_home = xdg_state_home().map(|path| path.to_string_lossy().to_string());
    if let Some(replaced) = expand_named_prefix(value, "XDG_STATE_HOME", state_home) {
        return replaced;
    }
    let cache_home = xdg_cache_home().map(|path| path.to_string_lossy().to_string());
    if let Some(replaced) = expand_named_prefix(value, "XDG_CACHE_HOME", cache_home) {
        return replaced;
    }
    if let Some(replaced) = expand_named_prefix(
        value,
        "XDG_RUNTIME_DIR",
        Some(default_runtime_dir_fallback()),
    ) {
        return replaced;
    }
    let home = home_dir();
    if let Some(replaced) = expand_named_prefix(value, "HOME", home.clone()) {
        return replaced;
    }
    if let Some(replaced) = expand_named_prefix(value, "USERPROFILE", home) {
        return replaced;
    }
    value.to_string()
}

fn expand_named_prefix(value: &str, key: &str, fallback: Option<String>) -> Option<String> {
    let env_value = env_value(key);
    let replacement = env_value.or(fallback)?;
    let prefix = format!("${}", key);
    if let Some(rest) = value.strip_prefix(&prefix) {
        return Some(format!("{}{}", replacement, rest));
    }
    let brace_prefix = format!("${{{}}}", key);
    if let Some(rest) = value.strip_prefix(&brace_prefix) {
        return Some(format!("{}{}", replacement, rest));
    }
    None
}

fn expand_tilde(value: &str) -> String {
    if value == "~" {
        return home_dir().unwrap_or_else(|| value.to_string());
    }
    if let Some(stripped) = value.strip_prefix("~/")
        && let Some(home) = home_dir()
    {
        return Path::new(&home)
            .join(stripped)
            .to_string_lossy()
            .to_string();
    }
    value.to_string()
}

fn home_dir() -> Option<String> {
    if let Some(home) = sudo_user_home() {
        return Some(home.to_string_lossy().to_string());
    }
    env_value("HOME").or_else(|| env_value("USERPROFILE"))
}

fn xdg_config_home() -> Option<PathBuf> {
    if let Some(home) = env_value("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(expand_tilde(&home)));
    }
    home_dir().map(|home| Path::new(&home).join(".config"))
}

fn xdg_config_home_for_install() -> Option<PathBuf> {
    if env_value("XDG_CONFIG_HOME").is_some() {
        return xdg_config_home();
    }
    if let Some(home) = sudo_user_home() {
        return Some(home.join(".config"));
    }
    xdg_config_home()
}

fn xdg_data_home_for_install() -> Option<PathBuf> {
    if env_value("XDG_DATA_HOME").is_some() {
        return xdg_data_home();
    }
    if let Some(home) = sudo_user_home() {
        return Some(home.join(".local/share"));
    }
    xdg_data_home()
}

fn xdg_data_home() -> Option<PathBuf> {
    if let Some(home) = env_value("XDG_DATA_HOME") {
        return Some(PathBuf::from(expand_tilde(&home)));
    }
    home_dir().map(|home| Path::new(&home).join(".local/share"))
}

fn xdg_cache_home() -> Option<PathBuf> {
    if let Some(home) = env_value("XDG_CACHE_HOME") {
        return Some(PathBuf::from(expand_tilde(&home)));
    }
    home_dir().map(|home| Path::new(&home).join(".cache"))
}

fn xdg_state_home() -> Option<PathBuf> {
    if let Some(home) = env_value("XDG_STATE_HOME") {
        return Some(PathBuf::from(expand_tilde(&home)));
    }
    home_dir().map(|home| Path::new(&home).join(".local/state"))
}

#[cfg(unix)]
fn sudo_user_home() -> Option<PathBuf> {
    let user = env_value("SUDO_USER")?;
    if user.trim().is_empty() || user == "root" {
        return None;
    }
    let passwd = fs::read_to_string("/etc/passwd").ok()?;
    for line in passwd.lines() {
        let mut parts = line.split(':');
        let name = parts.next()?;
        if name != user {
            continue;
        }
        let _passwd = parts.next()?;
        let _uid = parts.next()?;
        let _gid = parts.next()?;
        let _gecos = parts.next()?;
        let home = parts.next()?;
        if home.trim().is_empty() {
            return None;
        }
        return Some(PathBuf::from(home.trim()));
    }
    None
}

#[cfg(not(unix))]
fn sudo_user_home() -> Option<PathBuf> {
    None
}

fn resolve_builder(value: &str) -> Result<&'static str> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("cargo") {
        return Ok("cargo");
    }
    if trimmed.eq_ignore_ascii_case("cross") {
        return Ok("cross");
    }
    Err(anyhow!(
        "unsupported builder: {} (use cargo or cross)",
        trimmed
    ))
}
