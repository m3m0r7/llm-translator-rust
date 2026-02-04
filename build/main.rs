use anyhow::{anyhow, Context, Result};
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
    #[arg(long = "env-path", default_value = "build_env.toml", alias = "env")]
    env_path: PathBuf,

    /// Base directory for runtime assets
    #[arg(long = "base-directory", alias = "base-dir")]
    base_dir: Option<String>,

    /// Directory that contains the built binary
    #[arg(long = "bin-directory", alias = "bin-dir")]
    bin_dir: Option<String>,

    /// Install directory (destination for make install)
    #[arg(long = "install-directory", alias = "install-dir")]
    install_dir: Option<String>,

    /// Path to settings.toml
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
    #[arg(long = "env-path", default_value = "build_env.toml", alias = "env")]
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
    #[serde(rename = "baseDirectory")]
    base_directory: String,
    #[serde(rename = "binDirectory")]
    bin_directory: String,
    #[serde(rename = "installDirectory")]
    install_directory: String,
    #[serde(rename = "settingsFile")]
    settings_file: String,
}

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
    let project_env_path = project_dir.join("build_env.toml");
    if project_env_path != env_path {
        write_build_env(&project_env_path, &build_env)?;
    }

    let env_path_abs = env_path
        .canonicalize()
        .unwrap_or_else(|_| env_path.clone());

    let builder = resolve_builder(&args.builder)?;
    let mut cmd = Command::new(builder);
    cmd.args(["build", "--release"])
        .env("BUILD_ENV_TOML_PATH", &env_path_abs)
        .current_dir(&project_dir);
    if let Some(target) = args.target.as_deref() {
        if !target.trim().is_empty() {
            cmd.args(["--target", target.trim()]);
        }
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
    let install_dir = resolve_string_path(&project_dir, &env.install_directory);
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

    ensure_settings_file(&env, &project_dir)?;

    println!("Installed to {}", install_path.display());
    Ok(())
}

fn build_env_from_args(args: &BuildArgs) -> BuildEnv {
    let base_directory = args
        .base_dir
        .clone()
        .or_else(|| env_value("BUILD_ENV_BASE_DIR"))
        .unwrap_or_else(|| "~/.llm-translator-rust".to_string());

    let bin_directory = args
        .bin_dir
        .clone()
        .or_else(|| env_value("BUILD_ENV_BIN_DIR"))
        .unwrap_or_else(|| default_bin_dir(args.target.as_deref()));

    let install_directory = args
        .install_dir
        .clone()
        .or_else(|| env_value("BUILD_ENV_INSTALL_DIR"))
        .or_else(|| prefix_install_dir())
        .unwrap_or_else(default_install_dir);

    let settings_file = args
        .settings_file
        .clone()
        .or_else(|| env_value("BUILD_ENV_SETTINGS_FILE"))
        .unwrap_or_else(|| format!("{}/settings.toml", base_directory));

    BuildEnv {
        base_directory,
        bin_directory,
        install_directory,
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
    let expanded = expand_tilde(&raw);
    let path = PathBuf::from(expanded);
    if path.is_absolute() {
        path
    } else {
        base.join(path)
    }
}

fn resolve_string_path(base: &Path, value: &str) -> PathBuf {
    let expanded = expand_tilde(value);
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

fn ensure_settings_file(env: &BuildEnv, project_dir: &Path) -> Result<()> {
    let settings_path_raw = env.settings_file.trim();
    if settings_path_raw.is_empty() {
        return Ok(());
    }
    let settings_path = resolve_string_path(project_dir, settings_path_raw);
    if settings_path.exists() {
        return Ok(());
    }
    if let Some(parent) = settings_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create settings directory: {}",
                parent.display()
            )
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
    Ok(())
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

fn default_install_dir() -> String {
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

fn default_bin_dir(target: Option<&str>) -> String {
    if let Some(target) = target {
        let target = target.trim();
        if !target.is_empty() {
            return format!("target/{}/release", target);
        }
    }
    "target/release".to_string()
}

fn default_bin_ext() -> &'static str {
    if cfg!(windows) {
        ".exe"
    } else {
        ""
    }
}

fn expand_tilde(value: &str) -> String {
    if value == "~" {
        return home_dir().unwrap_or_else(|| value.to_string());
    }
    if let Some(stripped) = value.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return Path::new(&home)
                .join(stripped)
                .to_string_lossy()
                .to_string();
        }
    }
    value.to_string()
}

fn home_dir() -> Option<String> {
    if let Some(home) = env_value("HOME") {
        return Some(home);
    }
    env_value("USERPROFILE")
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
