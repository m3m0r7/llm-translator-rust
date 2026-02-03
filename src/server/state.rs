use crate::languages::LanguageRegistry;
use crate::settings;

#[derive(Clone)]
pub(crate) struct ServerState {
    pub(crate) settings: settings::Settings,
    pub(crate) registry: LanguageRegistry,
}
