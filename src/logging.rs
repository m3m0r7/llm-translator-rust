use anyhow::Result;
use tracing_subscriber::fmt;
use whisper_rs::install_whisper_tracing_trampoline;

pub fn init(verbose: bool) -> Result<()> {
    install_whisper_tracing_trampoline();
    if !verbose {
        return Ok(());
    }
    let _ = fmt()
        .with_target(false)
        .with_level(true)
        .with_thread_ids(false)
        .with_thread_names(false)
        .try_init();
    Ok(())
}
