use anyhow::Result;

use crate::languages::LanguageRegistry;
use crate::providers::{Provider, ProviderUsage};
use crate::settings::Settings;
use crate::translations::{self, tool_spec, TranslateOptions, TOOL_NAME};

#[derive(Debug, Clone)]
pub struct Translator<P: Provider + Clone> {
    provider: P,
    settings: Settings,
    registry: LanguageRegistry,
}

#[derive(Debug, Clone)]
pub struct ExecutionOutput {
    pub text: String,
    pub model: Option<String>,
    pub usage: Option<ProviderUsage>,
}

impl<P: Provider + Clone> Translator<P> {
    pub fn new(provider: P, settings: Settings, registry: LanguageRegistry) -> Self {
        Self {
            provider,
            settings,
            registry,
        }
    }

    pub async fn exec(&self, input: &str, options: TranslateOptions) -> Result<ExecutionOutput> {
        let tool = tool_spec(TOOL_NAME);
        let system_prompt =
            translations::render_system_prompt(&options, TOOL_NAME, &self.settings)?;

        let response = self
            .provider
            .clone()
            .register_tool(tool)
            .append_system_input(system_prompt)
            .append_user_input(input.to_string())
            .call_tool(TOOL_NAME)
            .await?;

        let parsed = translations::parse_tool_args(response.args, &options, &self.registry)?;
        Ok(ExecutionOutput {
            text: parsed.translation,
            model: response.model,
            usage: response.usage,
        })
    }
}
