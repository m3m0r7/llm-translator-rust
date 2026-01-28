use anyhow::Result;

use crate::data::{DataAttachment, DataInfo};
use crate::languages::LanguageRegistry;
use crate::providers::{Provider, ProviderResponse, ProviderUsage, ToolSpec};
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

#[derive(Debug, Clone)]
pub struct TranslationInput {
    pub text: String,
    pub data: Option<DataAttachment>,
}

impl<P: Provider + Clone> Translator<P> {
    pub fn new(provider: P, settings: Settings, registry: LanguageRegistry) -> Self {
        Self {
            provider,
            settings,
            registry,
        }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub async fn call_tool_with_data(
        &self,
        tool: ToolSpec,
        system_prompt: String,
        user_input: String,
        data: Option<DataAttachment>,
    ) -> Result<ProviderResponse> {
        let tool_name = tool.name.clone();
        let mut provider = self
            .provider
            .clone()
            .register_tool(tool)
            .append_system_input(system_prompt);
        if let Some(data) = data {
            provider = provider.append_user_data(data);
        }
        provider
            .append_user_input(user_input)
            .call_tool(&tool_name)
            .await
    }

    pub async fn exec(&self, input: &str, options: TranslateOptions) -> Result<ExecutionOutput> {
        self.exec_with_data(
            TranslationInput {
                text: input.to_string(),
                data: None,
            },
            options,
        )
        .await
    }

    pub async fn exec_with_data(
        &self,
        input: TranslationInput,
        options: TranslateOptions,
    ) -> Result<ExecutionOutput> {
        let tool = tool_spec(TOOL_NAME);
        let data_info: Option<DataInfo> = input.data.as_ref().map(|data| data.info());
        let system_prompt = translations::render_system_prompt_with_data(
            &options,
            TOOL_NAME,
            &self.settings,
            data_info.as_ref(),
        )?;

        let image_mode = input
            .data
            .as_ref()
            .map(|data| data.mime.starts_with("image/"))
            .unwrap_or(false);

        let mut provider = self
            .provider
            .clone()
            .register_tool(tool)
            .append_system_input(system_prompt);
        if let Some(data) = input.data {
            provider = provider.append_user_data(data);
        }
        let response = provider
            .append_user_input(input.text)
            .call_tool(TOOL_NAME)
            .await?;

        let parsed =
            translations::parse_tool_args(response.args, &options, &self.registry, image_mode)?;
        let text = if !parsed.segments.is_empty() {
            translations::format_segments_output(&parsed.segments)?
        } else {
            parsed.translation
        };
        Ok(ExecutionOutput {
            text,
            model: response.model,
            usage: response.usage,
        })
    }
}
