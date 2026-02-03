use std::os::raw::c_char;
use std::ptr;

use crate::{run, run_with_settings};

use super::config::ExtConfig;
use super::error::{cstr_to_string, set_last_error, string_to_c};
use super::runtime::runtime;
use super::settings::ExtSettings;

#[no_mangle]
pub extern "C" fn llm_ext_run(config: *const ExtConfig, input: *const c_char) -> *mut c_char {
    let Some(config) = (unsafe { config.as_ref() }) else {
        set_last_error("config is null");
        return ptr::null_mut();
    };
    let input = cstr_to_string(input);
    match runtime().block_on(run(config.inner.clone(), input)) {
        Ok(output) => string_to_c(&output),
        Err(err) => {
            set_last_error(err.to_string());
            ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn llm_ext_run_with_settings(
    config: *const ExtConfig,
    settings: *const ExtSettings,
    input: *const c_char,
) -> *mut c_char {
    let Some(config) = (unsafe { config.as_ref() }) else {
        set_last_error("config is null");
        return ptr::null_mut();
    };
    let Some(settings) = (unsafe { settings.as_ref() }) else {
        set_last_error("settings is null");
        return ptr::null_mut();
    };
    let input = cstr_to_string(input);
    match runtime().block_on(run_with_settings(
        config.inner.clone(),
        settings.inner.clone(),
        input,
    )) {
        Ok(output) => string_to_c(&output),
        Err(err) => {
            set_last_error(err.to_string());
            ptr::null_mut()
        }
    }
}
