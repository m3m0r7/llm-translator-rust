#ifndef LLM_TRANSLATOR_RUST_H
#define LLM_TRANSLATOR_RUST_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct ExtConfig ExtConfig;
typedef struct ExtSettings ExtSettings;

// Error and memory helpers
char *llm_ext_last_error_message(void);
void llm_ext_free_string(char *value);

// Config lifecycle
ExtConfig *llm_ext_config_new(void);
void llm_ext_config_free(ExtConfig *config);

// Settings lifecycle
ExtSettings *llm_ext_settings_new(void);
void llm_ext_settings_free(ExtSettings *settings);
ExtSettings *llm_ext_settings_load_from_file(const char *path);

// Config setters/getters
bool llm_ext_config_set_lang(ExtConfig *config, const char *value);
char *llm_ext_config_get_lang(const ExtConfig *config);
bool llm_ext_config_set_model(ExtConfig *config, const char *value);
char *llm_ext_config_get_model(const ExtConfig *config);
bool llm_ext_config_set_key(ExtConfig *config, const char *value);
char *llm_ext_config_get_key(const ExtConfig *config);
bool llm_ext_config_set_formal(ExtConfig *config, const char *value);
char *llm_ext_config_get_formal(const ExtConfig *config);
bool llm_ext_config_set_source_lang(ExtConfig *config, const char *value);
char *llm_ext_config_get_source_lang(const ExtConfig *config);
bool llm_ext_config_set_slang(ExtConfig *config, bool value);
bool llm_ext_config_get_slang(const ExtConfig *config);
bool llm_ext_config_set_data(ExtConfig *config, const char *value);
char *llm_ext_config_get_data(const ExtConfig *config);
bool llm_ext_config_set_data_mime(ExtConfig *config, const char *value);
char *llm_ext_config_get_data_mime(const ExtConfig *config);
bool llm_ext_config_set_directory_translation_threads(ExtConfig *config, ptrdiff_t value);
ptrdiff_t llm_ext_config_get_directory_translation_threads(const ExtConfig *config);
bool llm_ext_config_set_out_path(ExtConfig *config, const char *value);
char *llm_ext_config_get_out_path(const ExtConfig *config);
bool llm_ext_config_set_overwrite(ExtConfig *config, bool value);
bool llm_ext_config_get_overwrite(const ExtConfig *config);
bool llm_ext_config_set_force_translation(ExtConfig *config, bool value);
bool llm_ext_config_get_force_translation(const ExtConfig *config);
bool llm_ext_config_set_settings_path(ExtConfig *config, const char *value);
char *llm_ext_config_get_settings_path(const ExtConfig *config);
bool llm_ext_config_set_show_enabled_languages(ExtConfig *config, bool value);
bool llm_ext_config_get_show_enabled_languages(const ExtConfig *config);
bool llm_ext_config_set_show_enabled_styles(ExtConfig *config, bool value);
bool llm_ext_config_get_show_enabled_styles(const ExtConfig *config);
bool llm_ext_config_set_show_models_list(ExtConfig *config, bool value);
bool llm_ext_config_get_show_models_list(const ExtConfig *config);
bool llm_ext_config_set_show_whisper_models(ExtConfig *config, bool value);
bool llm_ext_config_get_show_whisper_models(const ExtConfig *config);
bool llm_ext_config_set_pos(ExtConfig *config, bool value);
bool llm_ext_config_get_pos(const ExtConfig *config);
bool llm_ext_config_set_correction(ExtConfig *config, bool value);
bool llm_ext_config_get_correction(const ExtConfig *config);
bool llm_ext_config_set_show_histories(ExtConfig *config, bool value);
bool llm_ext_config_get_show_histories(const ExtConfig *config);
bool llm_ext_config_set_with_using_tokens(ExtConfig *config, bool value);
bool llm_ext_config_get_with_using_tokens(const ExtConfig *config);
bool llm_ext_config_set_with_using_model(ExtConfig *config, bool value);
bool llm_ext_config_get_with_using_model(const ExtConfig *config);
bool llm_ext_config_set_with_commentout(ExtConfig *config, bool value);
bool llm_ext_config_get_with_commentout(const ExtConfig *config);
bool llm_ext_config_set_debug_ocr(ExtConfig *config, bool value);
bool llm_ext_config_get_debug_ocr(const ExtConfig *config);
bool llm_ext_config_set_verbose(ExtConfig *config, bool value);
bool llm_ext_config_get_verbose(const ExtConfig *config);
bool llm_ext_config_set_whisper_model(ExtConfig *config, const char *value);
char *llm_ext_config_get_whisper_model(const ExtConfig *config);

// Config ignore list
bool llm_ext_config_clear_ignore_translation_files(ExtConfig *config);
bool llm_ext_config_add_ignore_translation_file(ExtConfig *config, const char *value);
size_t llm_ext_config_ignore_translation_files_len(const ExtConfig *config);
char *llm_ext_config_get_ignore_translation_file(const ExtConfig *config, size_t index);

// Settings setters/getters
bool llm_ext_settings_set_translated_suffix(ExtSettings *settings, const char *value);
char *llm_ext_settings_get_translated_suffix(const ExtSettings *settings);
bool llm_ext_settings_set_translation_ignore_file(ExtSettings *settings, const char *value);
char *llm_ext_settings_get_translation_ignore_file(const ExtSettings *settings);
bool llm_ext_settings_set_overlay_text_color(ExtSettings *settings, const char *value);
char *llm_ext_settings_get_overlay_text_color(const ExtSettings *settings);
bool llm_ext_settings_set_overlay_stroke_color(ExtSettings *settings, const char *value);
char *llm_ext_settings_get_overlay_stroke_color(const ExtSettings *settings);
bool llm_ext_settings_set_overlay_fill_color(ExtSettings *settings, const char *value);
char *llm_ext_settings_get_overlay_fill_color(const ExtSettings *settings);
bool llm_ext_settings_set_overlay_font_family(ExtSettings *settings, const char *value);
char *llm_ext_settings_get_overlay_font_family(const ExtSettings *settings);
bool llm_ext_settings_set_overlay_font_path(ExtSettings *settings, const char *value);
char *llm_ext_settings_get_overlay_font_path(const ExtSettings *settings);
bool llm_ext_settings_set_whisper_model(ExtSettings *settings, const char *value);
char *llm_ext_settings_get_whisper_model(const ExtSettings *settings);
bool llm_ext_settings_set_ocr_normalize(ExtSettings *settings, bool value);
bool llm_ext_settings_get_ocr_normalize(const ExtSettings *settings);
bool llm_ext_settings_set_history_limit(ExtSettings *settings, size_t value);
size_t llm_ext_settings_get_history_limit(const ExtSettings *settings);
bool llm_ext_settings_set_backup_ttl_days(ExtSettings *settings, uint64_t value);
uint64_t llm_ext_settings_get_backup_ttl_days(const ExtSettings *settings);
bool llm_ext_settings_set_directory_translation_threads(ExtSettings *settings, size_t value);
size_t llm_ext_settings_get_directory_translation_threads(const ExtSettings *settings);
bool llm_ext_settings_set_overlay_font_size(ExtSettings *settings, float value);
float llm_ext_settings_get_overlay_font_size(const ExtSettings *settings);
bool llm_ext_settings_set_server_host(ExtSettings *settings, const char *value);
char *llm_ext_settings_get_server_host(const ExtSettings *settings);
bool llm_ext_settings_set_server_port(ExtSettings *settings, uint16_t value);
uint16_t llm_ext_settings_get_server_port(const ExtSettings *settings);
bool llm_ext_settings_set_server_tmp_dir(ExtSettings *settings, const char *value);
char *llm_ext_settings_get_server_tmp_dir(const ExtSettings *settings);

// Settings language list
bool llm_ext_settings_clear_system_languages(ExtSettings *settings);
bool llm_ext_settings_add_system_language(ExtSettings *settings, const char *value);
size_t llm_ext_settings_system_languages_len(const ExtSettings *settings);
char *llm_ext_settings_get_system_language(const ExtSettings *settings, size_t index);

// Settings formal map
bool llm_ext_settings_set_formal(ExtSettings *settings, const char *key, const char *value);
char *llm_ext_settings_get_formal(const ExtSettings *settings, const char *key);
bool llm_ext_settings_remove_formal(ExtSettings *settings, const char *key);
size_t llm_ext_settings_formal_len(const ExtSettings *settings);

// Run
char *llm_ext_run(const ExtConfig *config, const char *input);
char *llm_ext_run_with_settings(const ExtConfig *config, const ExtSettings *settings, const char *input);

#ifdef __cplusplus
}
#endif

#endif
