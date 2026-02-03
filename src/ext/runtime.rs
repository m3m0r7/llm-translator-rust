use std::sync::OnceLock;

use tokio::runtime::Runtime;

pub(crate) fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| Runtime::new().expect("failed to init runtime"))
}
