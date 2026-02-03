use std::os::raw::c_char;
use std::ptr;

use crate::settings::{self, Settings};

use super::error::{cstr_to_string, set_last_error, string_to_c};

#[repr(C)]
pub struct ExtSettings {
    pub(crate) inner: Settings,
}

#[no_mangle]
pub extern "C" fn llm_ext_settings_new() -> *mut ExtSettings {
    Box::into_raw(Box::new(ExtSettings {
        inner: Settings::default(),
    }))
}

#[no_mangle]
pub extern "C" fn llm_ext_settings_free(settings: *mut ExtSettings) {
    if settings.is_null() {
        return;
    }
    unsafe {
        let _ = Box::from_raw(settings);
    }
}

#[no_mangle]
pub extern "C" fn llm_ext_settings_load_from_file(path: *const c_char) -> *mut ExtSettings {
    let path = cstr_to_string(path);
    let loaded = match path.as_deref() {
        Some(value) => settings::load_settings(Some(std::path::Path::new(value))),
        None => settings::load_settings(None),
    };
    match loaded {
        Ok(settings) => Box::into_raw(Box::new(ExtSettings { inner: settings })),
        Err(err) => {
            set_last_error(err.to_string());
            ptr::null_mut()
        }
    }
}

macro_rules! settings_set_string {
    ($name:ident, $field:ident) => {
        #[no_mangle]
        pub extern "C" fn $name(settings: *mut ExtSettings, value: *const c_char) -> bool {
            let Some(settings) = (unsafe { settings.as_mut() }) else {
                set_last_error("settings is null");
                return false;
            };
            let Some(value) = cstr_to_string(value) else {
                set_last_error("value is null");
                return false;
            };
            settings.inner.$field = value;
            true
        }
    };
}

macro_rules! settings_get_string {
    ($name:ident, $field:ident) => {
        #[no_mangle]
        pub extern "C" fn $name(settings: *const ExtSettings) -> *mut c_char {
            let Some(settings) = (unsafe { settings.as_ref() }) else {
                set_last_error("settings is null");
                return ptr::null_mut();
            };
            string_to_c(&settings.inner.$field)
        }
    };
}

macro_rules! settings_set_option_string {
    ($name:ident, $field:ident) => {
        #[no_mangle]
        pub extern "C" fn $name(settings: *mut ExtSettings, value: *const c_char) -> bool {
            let Some(settings) = (unsafe { settings.as_mut() }) else {
                set_last_error("settings is null");
                return false;
            };
            if value.is_null() {
                settings.inner.$field = None;
                return true;
            }
            let Some(value) = cstr_to_string(value) else {
                set_last_error("value is null");
                return false;
            };
            settings.inner.$field = Some(value);
            true
        }
    };
}

macro_rules! settings_get_option_string {
    ($name:ident, $field:ident) => {
        #[no_mangle]
        pub extern "C" fn $name(settings: *const ExtSettings) -> *mut c_char {
            let Some(settings) = (unsafe { settings.as_ref() }) else {
                set_last_error("settings is null");
                return ptr::null_mut();
            };
            match settings.inner.$field.as_deref() {
                Some(value) => string_to_c(value),
                None => ptr::null_mut(),
            }
        }
    };
}

macro_rules! settings_set_bool {
    ($name:ident, $field:ident) => {
        #[no_mangle]
        pub extern "C" fn $name(settings: *mut ExtSettings, value: bool) -> bool {
            let Some(settings) = (unsafe { settings.as_mut() }) else {
                set_last_error("settings is null");
                return false;
            };
            settings.inner.$field = value;
            true
        }
    };
}

macro_rules! settings_get_bool {
    ($name:ident, $field:ident) => {
        #[no_mangle]
        pub extern "C" fn $name(settings: *const ExtSettings) -> bool {
            let Some(settings) = (unsafe { settings.as_ref() }) else {
                set_last_error("settings is null");
                return false;
            };
            settings.inner.$field
        }
    };
}

macro_rules! settings_set_usize {
    ($name:ident, $field:ident) => {
        #[no_mangle]
        pub extern "C" fn $name(settings: *mut ExtSettings, value: usize) -> bool {
            let Some(settings) = (unsafe { settings.as_mut() }) else {
                set_last_error("settings is null");
                return false;
            };
            settings.inner.$field = value;
            true
        }
    };
}

macro_rules! settings_get_usize {
    ($name:ident, $field:ident) => {
        #[no_mangle]
        pub extern "C" fn $name(settings: *const ExtSettings) -> usize {
            let Some(settings) = (unsafe { settings.as_ref() }) else {
                set_last_error("settings is null");
                return 0;
            };
            settings.inner.$field
        }
    };
}

macro_rules! settings_set_u64 {
    ($name:ident, $field:ident) => {
        #[no_mangle]
        pub extern "C" fn $name(settings: *mut ExtSettings, value: u64) -> bool {
            let Some(settings) = (unsafe { settings.as_mut() }) else {
                set_last_error("settings is null");
                return false;
            };
            settings.inner.$field = value;
            true
        }
    };
}

macro_rules! settings_get_u64 {
    ($name:ident, $field:ident) => {
        #[no_mangle]
        pub extern "C" fn $name(settings: *const ExtSettings) -> u64 {
            let Some(settings) = (unsafe { settings.as_ref() }) else {
                set_last_error("settings is null");
                return 0;
            };
            settings.inner.$field
        }
    };
}

macro_rules! settings_set_option_f32 {
    ($name:ident, $field:ident) => {
        #[no_mangle]
        pub extern "C" fn $name(settings: *mut ExtSettings, value: f32) -> bool {
            let Some(settings) = (unsafe { settings.as_mut() }) else {
                set_last_error("settings is null");
                return false;
            };
            if value <= 0.0 {
                settings.inner.$field = None;
            } else {
                settings.inner.$field = Some(value);
            }
            true
        }
    };
}

macro_rules! settings_get_option_f32 {
    ($name:ident, $field:ident) => {
        #[no_mangle]
        pub extern "C" fn $name(settings: *const ExtSettings) -> f32 {
            let Some(settings) = (unsafe { settings.as_ref() }) else {
                set_last_error("settings is null");
                return -1.0;
            };
            match settings.inner.$field {
                Some(value) => value,
                None => -1.0,
            }
        }
    };
}

settings_set_string!(llm_ext_settings_set_translated_suffix, translated_suffix);
settings_get_string!(llm_ext_settings_get_translated_suffix, translated_suffix);
settings_set_string!(
    llm_ext_settings_set_translation_ignore_file,
    translation_ignore_file
);
settings_get_string!(
    llm_ext_settings_get_translation_ignore_file,
    translation_ignore_file
);
settings_set_string!(llm_ext_settings_set_overlay_text_color, overlay_text_color);
settings_get_string!(llm_ext_settings_get_overlay_text_color, overlay_text_color);
settings_set_string!(
    llm_ext_settings_set_overlay_stroke_color,
    overlay_stroke_color
);
settings_get_string!(
    llm_ext_settings_get_overlay_stroke_color,
    overlay_stroke_color
);
settings_set_string!(llm_ext_settings_set_overlay_fill_color, overlay_fill_color);
settings_get_string!(llm_ext_settings_get_overlay_fill_color, overlay_fill_color);
settings_set_option_string!(
    llm_ext_settings_set_overlay_font_family,
    overlay_font_family
);
settings_get_option_string!(
    llm_ext_settings_get_overlay_font_family,
    overlay_font_family
);
settings_set_option_string!(llm_ext_settings_set_overlay_font_path, overlay_font_path);
settings_get_option_string!(llm_ext_settings_get_overlay_font_path, overlay_font_path);
settings_set_option_string!(llm_ext_settings_set_whisper_model, whisper_model);
settings_get_option_string!(llm_ext_settings_get_whisper_model, whisper_model);
settings_set_bool!(llm_ext_settings_set_ocr_normalize, ocr_normalize);
settings_get_bool!(llm_ext_settings_get_ocr_normalize, ocr_normalize);
settings_set_usize!(llm_ext_settings_set_history_limit, history_limit);
settings_get_usize!(llm_ext_settings_get_history_limit, history_limit);
settings_set_u64!(llm_ext_settings_set_backup_ttl_days, backup_ttl_days);
settings_get_u64!(llm_ext_settings_get_backup_ttl_days, backup_ttl_days);
settings_set_usize!(
    llm_ext_settings_set_directory_translation_threads,
    directory_translation_threads
);
settings_get_usize!(
    llm_ext_settings_get_directory_translation_threads,
    directory_translation_threads
);
settings_set_option_f32!(llm_ext_settings_set_overlay_font_size, overlay_font_size);
settings_get_option_f32!(llm_ext_settings_get_overlay_font_size, overlay_font_size);
settings_set_string!(llm_ext_settings_set_server_host, server_host);
settings_get_string!(llm_ext_settings_get_server_host, server_host);
settings_set_option_string!(llm_ext_settings_set_server_tmp_dir, server_tmp_dir);
settings_get_option_string!(llm_ext_settings_get_server_tmp_dir, server_tmp_dir);

#[no_mangle]
pub extern "C" fn llm_ext_settings_set_server_port(settings: *mut ExtSettings, value: u16) -> bool {
    let Some(settings) = (unsafe { settings.as_mut() }) else {
        set_last_error("settings is null");
        return false;
    };
    if value == 0 {
        return false;
    }
    settings.inner.server_port = value;
    true
}

#[no_mangle]
pub extern "C" fn llm_ext_settings_get_server_port(settings: *const ExtSettings) -> u16 {
    let Some(settings) = (unsafe { settings.as_ref() }) else {
        set_last_error("settings is null");
        return 0;
    };
    settings.inner.server_port
}

#[no_mangle]
pub extern "C" fn llm_ext_settings_clear_system_languages(settings: *mut ExtSettings) -> bool {
    let Some(settings) = (unsafe { settings.as_mut() }) else {
        set_last_error("settings is null");
        return false;
    };
    settings.inner.system_languages.clear();
    true
}

#[no_mangle]
pub extern "C" fn llm_ext_settings_add_system_language(
    settings: *mut ExtSettings,
    value: *const c_char,
) -> bool {
    let Some(settings) = (unsafe { settings.as_mut() }) else {
        set_last_error("settings is null");
        return false;
    };
    let Some(value) = cstr_to_string(value) else {
        set_last_error("value is null");
        return false;
    };
    settings.inner.system_languages.push(value);
    true
}

#[no_mangle]
pub extern "C" fn llm_ext_settings_system_languages_len(settings: *const ExtSettings) -> usize {
    let Some(settings) = (unsafe { settings.as_ref() }) else {
        set_last_error("settings is null");
        return 0;
    };
    settings.inner.system_languages.len()
}

#[no_mangle]
pub extern "C" fn llm_ext_settings_get_system_language(
    settings: *const ExtSettings,
    index: usize,
) -> *mut c_char {
    let Some(settings) = (unsafe { settings.as_ref() }) else {
        set_last_error("settings is null");
        return ptr::null_mut();
    };
    match settings.inner.system_languages.get(index) {
        Some(value) => string_to_c(value),
        None => ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn llm_ext_settings_set_formal(
    settings: *mut ExtSettings,
    key: *const c_char,
    value: *const c_char,
) -> bool {
    let Some(settings) = (unsafe { settings.as_mut() }) else {
        set_last_error("settings is null");
        return false;
    };
    let Some(key) = cstr_to_string(key) else {
        set_last_error("key is null");
        return false;
    };
    let Some(value) = cstr_to_string(value) else {
        set_last_error("value is null");
        return false;
    };
    settings.inner.formally.insert(key, value);
    true
}

#[no_mangle]
pub extern "C" fn llm_ext_settings_get_formal(
    settings: *const ExtSettings,
    key: *const c_char,
) -> *mut c_char {
    let Some(settings) = (unsafe { settings.as_ref() }) else {
        set_last_error("settings is null");
        return ptr::null_mut();
    };
    let Some(key) = cstr_to_string(key) else {
        set_last_error("key is null");
        return ptr::null_mut();
    };
    match settings.inner.formally.get(&key) {
        Some(value) => string_to_c(value),
        None => ptr::null_mut(),
    }
}

#[no_mangle]
pub extern "C" fn llm_ext_settings_remove_formal(
    settings: *mut ExtSettings,
    key: *const c_char,
) -> bool {
    let Some(settings) = (unsafe { settings.as_mut() }) else {
        set_last_error("settings is null");
        return false;
    };
    let Some(key) = cstr_to_string(key) else {
        set_last_error("key is null");
        return false;
    };
    settings.inner.formally.remove(&key);
    true
}

#[no_mangle]
pub extern "C" fn llm_ext_settings_formal_len(settings: *const ExtSettings) -> usize {
    let Some(settings) = (unsafe { settings.as_ref() }) else {
        set_last_error("settings is null");
        return 0;
    };
    settings.inner.formally.len()
}
