use llm_translator_rust::settings;
use llm_translator_rust::translations::{TOOL_NAME, TranslateOptions, render_system_prompt};

#[test]
fn system_prompt_snapshot() {
    let settings = settings::load_settings(None).unwrap();
    let options = TranslateOptions {
        lang: "en".to_string(),
        formality: "formal".to_string(),
        source_lang: "auto".to_string(),
        slang: false,
    };
    let prompt = render_system_prompt(&options, TOOL_NAME, &settings).unwrap();
    insta::assert_snapshot!(prompt);
}
