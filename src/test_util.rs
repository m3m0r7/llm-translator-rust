#[cfg(test)]
pub(crate) fn with_temp_home<F, R>(func: F) -> R
where
    F: FnOnce(&std::path::Path) -> R,
{
    static HOME_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = HOME_MUTEX.lock().expect("home lock");
    let dir = tempfile::tempdir().expect("tempdir");
    let old_home = std::env::var("HOME").ok();
    std::env::set_var("HOME", dir.path());
    let result = func(dir.path());
    if let Some(old) = old_home {
        std::env::set_var("HOME", old);
    } else {
        std::env::remove_var("HOME");
    }
    result
}
