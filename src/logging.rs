use anyhow::Result;
use tracing_subscriber::fmt;

pub fn init(verbose: bool) -> Result<()> {
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
