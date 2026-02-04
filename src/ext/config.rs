use std::os::raw::c_char;
use std::ptr;

use crate::Config;

use super::error::{cstr_to_string, set_last_error, string_to_c};

#[repr(C)]
pub struct ExtConfig {
    pub(crate) inner: Config,
}

fn default_config() -> Config {
    Config {
        lang: "en".to_string(),
        model: None,
        key: None,
        formal: "formal".to_string(),
        source_lang: "auto".to_string(),
        slang: false,
        data: None,
        data_mime: None,
        data_attachment: None,
        directory_translation_threads: None,
        ignore_translation_files: Vec::new(),
        out_path: None,
        overwrite: false,
        force_translation: false,
        settings_path: None,
        show_enabled_languages: false,
        show_enabled_styles: false,
        show_models_list: false,
        show_whisper_models: false,
        pos: false,
        correction: false,
        details: false,
        show_histories: false,
        with_using_tokens: false,
        with_using_model: false,
        with_commentout: false,
        debug_ocr: false,
        verbose: false,
        whisper_model: None,
    }
}

#[no_mangle]
pub extern "C" fn llm_ext_config_new() -> *mut ExtConfig {
    Box::into_raw(Box::new(ExtConfig {
        inner: default_config(),
    }))
}

#[no_mangle]
pub extern "C" fn llm_ext_config_free(config: *mut ExtConfig) {
    if config.is_null() {
        return;
    }
    unsafe {
        let _ = Box::from_raw(config);
    }
}

macro_rules! config_set_string {
    ($name:ident, $field:ident) => {
        #[no_mangle]
        pub extern "C" fn $name(config: *mut ExtConfig, value: *const c_char) -> bool {
            let Some(config) = (unsafe { config.as_mut() }) else {
                set_last_error("config is null");
                return false;
            };
            let Some(value) = cstr_to_string(value) else {
                set_last_error("value is null");
                return false;
            };
            config.inner.$field = value;
            true
        }
    };
}

macro_rules! config_get_string {
    ($name:ident, $field:ident) => {
        #[no_mangle]
        pub extern "C" fn $name(config: *const ExtConfig) -> *mut c_char {
            let Some(config) = (unsafe { config.as_ref() }) else {
                set_last_error("config is null");
                return ptr::null_mut();
            };
            string_to_c(&config.inner.$field)
        }
    };
}

macro_rules! config_set_option_string {
    ($name:ident, $field:ident) => {
        #[no_mangle]
        pub extern "C" fn $name(config: *mut ExtConfig, value: *const c_char) -> bool {
            let Some(config) = (unsafe { config.as_mut() }) else {
                set_last_error("config is null");
                return false;
            };
            if value.is_null() {
                config.inner.$field = None;
                return true;
            }
            let Some(value) = cstr_to_string(value) else {
                set_last_error("value is null");
                return false;
            };
            config.inner.$field = Some(value);
            true
        }
    };
}

macro_rules! config_get_option_string {
    ($name:ident, $field:ident) => {
        #[no_mangle]
        pub extern "C" fn $name(config: *const ExtConfig) -> *mut c_char {
            let Some(config) = (unsafe { config.as_ref() }) else {
                set_last_error("config is null");
                return ptr::null_mut();
            };
            match config.inner.$field.as_deref() {
                Some(value) => string_to_c(value),
                None => ptr::null_mut(),
            }
        }
    };
}

macro_rules! config_set_bool {
    ($name:ident, $field:ident) => {
        #[no_mangle]
        pub extern "C" fn $name(config: *mut ExtConfig, value: bool) -> bool {
            let Some(config) = (unsafe { config.as_mut() }) else {
                set_last_error("config is null");
                return false;
            };
            config.inner.$field = value;
            true
        }
    };
}

macro_rules! config_get_bool {
    ($name:ident, $field:ident) => {
        #[no_mangle]
        pub extern "C" fn $name(config: *const ExtConfig) -> bool {
            let Some(config) = (unsafe { config.as_ref() }) else {
                set_last_error("config is null");
                return false;
            };
            config.inner.$field
        }
    };
}

macro_rules! config_set_option_usize {
    ($name:ident, $field:ident) => {
        #[no_mangle]
        pub extern "C" fn $name(config: *mut ExtConfig, value: isize) -> bool {
            let Some(config) = (unsafe { config.as_mut() }) else {
                set_last_error("config is null");
                return false;
            };
            if value <= 0 {
                config.inner.$field = None;
            } else {
                config.inner.$field = Some(value as usize);
            }
            true
        }
    };
}

macro_rules! config_get_option_usize {
    ($name:ident, $field:ident) => {
        #[no_mangle]
        pub extern "C" fn $name(config: *const ExtConfig) -> isize {
            let Some(config) = (unsafe { config.as_ref() }) else {
                set_last_error("config is null");
                return -1;
            };
            match config.inner.$field {
                Some(value) => value as isize,
                None => -1,
            }
        }
    };
}

config_set_string!(llm_ext_config_set_lang, lang);
config_get_string!(llm_ext_config_get_lang, lang);
config_set_option_string!(llm_ext_config_set_model, model);
config_get_option_string!(llm_ext_config_get_model, model);
config_set_option_string!(llm_ext_config_set_key, key);
config_get_option_string!(llm_ext_config_get_key, key);
config_set_string!(llm_ext_config_set_formal, formal);
config_get_string!(llm_ext_config_get_formal, formal);
config_set_string!(llm_ext_config_set_source_lang, source_lang);
config_get_string!(llm_ext_config_get_source_lang, source_lang);
config_set_bool!(llm_ext_config_set_slang, slang);
config_get_bool!(llm_ext_config_get_slang, slang);
config_set_option_string!(llm_ext_config_set_data, data);
config_get_option_string!(llm_ext_config_get_data, data);
config_set_option_string!(llm_ext_config_set_data_mime, data_mime);
config_get_option_string!(llm_ext_config_get_data_mime, data_mime);
config_set_option_usize!(
    llm_ext_config_set_directory_translation_threads,
    directory_translation_threads
);
config_get_option_usize!(
    llm_ext_config_get_directory_translation_threads,
    directory_translation_threads
);
config_set_option_string!(llm_ext_config_set_out_path, out_path);
config_get_option_string!(llm_ext_config_get_out_path, out_path);
config_set_bool!(llm_ext_config_set_overwrite, overwrite);
config_get_bool!(llm_ext_config_get_overwrite, overwrite);
config_set_bool!(llm_ext_config_set_force_translation, force_translation);
config_get_bool!(llm_ext_config_get_force_translation, force_translation);
config_set_option_string!(llm_ext_config_set_settings_path, settings_path);
config_get_option_string!(llm_ext_config_get_settings_path, settings_path);
config_set_bool!(
    llm_ext_config_set_show_enabled_languages,
    show_enabled_languages
);
config_get_bool!(
    llm_ext_config_get_show_enabled_languages,
    show_enabled_languages
);
config_set_bool!(llm_ext_config_set_show_enabled_styles, show_enabled_styles);
config_get_bool!(llm_ext_config_get_show_enabled_styles, show_enabled_styles);
config_set_bool!(llm_ext_config_set_show_models_list, show_models_list);
config_get_bool!(llm_ext_config_get_show_models_list, show_models_list);
config_set_bool!(llm_ext_config_set_show_whisper_models, show_whisper_models);
config_get_bool!(llm_ext_config_get_show_whisper_models, show_whisper_models);
config_set_bool!(llm_ext_config_set_pos, pos);
config_get_bool!(llm_ext_config_get_pos, pos);
config_set_bool!(llm_ext_config_set_correction, correction);
config_get_bool!(llm_ext_config_get_correction, correction);
config_set_bool!(llm_ext_config_set_show_histories, show_histories);
config_get_bool!(llm_ext_config_get_show_histories, show_histories);
config_set_bool!(llm_ext_config_set_with_using_tokens, with_using_tokens);
config_get_bool!(llm_ext_config_get_with_using_tokens, with_using_tokens);
config_set_bool!(llm_ext_config_set_with_using_model, with_using_model);
config_get_bool!(llm_ext_config_get_with_using_model, with_using_model);
config_set_bool!(llm_ext_config_set_with_commentout, with_commentout);
config_get_bool!(llm_ext_config_get_with_commentout, with_commentout);
config_set_bool!(llm_ext_config_set_debug_ocr, debug_ocr);
config_get_bool!(llm_ext_config_get_debug_ocr, debug_ocr);
config_set_bool!(llm_ext_config_set_verbose, verbose);
config_get_bool!(llm_ext_config_get_verbose, verbose);
config_set_option_string!(llm_ext_config_set_whisper_model, whisper_model);
config_get_option_string!(llm_ext_config_get_whisper_model, whisper_model);

#[no_mangle]
pub extern "C" fn llm_ext_config_clear_ignore_translation_files(config: *mut ExtConfig) -> bool {
    let Some(config) = (unsafe { config.as_mut() }) else {
        set_last_error("config is null");
        return false;
    };
    config.inner.ignore_translation_files.clear();
    true
}

#[no_mangle]
pub extern "C" fn llm_ext_config_add_ignore_translation_file(
    config: *mut ExtConfig,
    value: *const c_char,
) -> bool {
    let Some(config) = (unsafe { config.as_mut() }) else {
        set_last_error("config is null");
        return false;
    };
    let Some(value) = cstr_to_string(value) else {
        set_last_error("value is null");
        return false;
    };
    config.inner.ignore_translation_files.push(value);
    true
}

#[no_mangle]
pub extern "C" fn llm_ext_config_ignore_translation_files_len(config: *const ExtConfig) -> usize {
    let Some(config) = (unsafe { config.as_ref() }) else {
        set_last_error("config is null");
        return 0;
    };
    config.inner.ignore_translation_files.len()
}

#[no_mangle]
pub extern "C" fn llm_ext_config_get_ignore_translation_file(
    config: *const ExtConfig,
    index: usize,
) -> *mut c_char {
    let Some(config) = (unsafe { config.as_ref() }) else {
        set_last_error("config is null");
        return ptr::null_mut();
    };
    match config.inner.ignore_translation_files.get(index) {
        Some(value) => string_to_c(value),
        None => ptr::null_mut(),
    }
}
