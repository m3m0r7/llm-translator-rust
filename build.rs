use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is not set by cargo"),
    );
    let languages_dir = manifest_dir.join("src").join("languages");
    println!("cargo:rerun-if-changed={}", languages_dir.display());

    let mut codes = Vec::new();
    let entries = fs::read_dir(&languages_dir).expect("failed to list src/languages");
    for entry in entries {
        let entry = entry.expect("failed to read src/languages entry");
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|value| value.to_str()) {
            let code = stem.to_ascii_lowercase();
            codes.push(code);
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }

    codes.sort();
    codes.dedup();

    let mut generated = String::new();
    generated
        .push_str("pub(crate) fn embedded_language_pack(code: &str) -> Option<&'static str> {\n");
    generated.push_str("    match code {\n");
    for code in codes {
        generated.push_str(&format!(
            "        \"{code}\" => Some(include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/src/languages/{code}.toml\"))),\n"
        ));
    }
    generated.push_str("        _ => None,\n");
    generated.push_str("    }\n");
    generated.push_str("}\n");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is not set by cargo"));
    let destination = out_dir.join("embedded_language_packs.rs");
    fs::write(&destination, generated).expect("failed to write embedded language pack index");
}
