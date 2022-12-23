use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;
use wasm_bindgen_cli_support::Bindgen;

fn main() -> anyhow::Result<()> {
    embuild::build::CfgArgs::output_propagated("ESP_IDF")?;
    embuild::build::LinkArgs::output_propagated("ESP_IDF")?;

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let frontend_crate_dir = Path::new("./frontend");
    let frontend_target_dir = out_dir.join("frontend");
    let frontend_out_dir = frontend_target_dir
        .join("wasm32-unknown-unknown")
        .join("release");

    // Rerun build script if frontend crate changed
    println!("cargo:rerun-if-changed={}", frontend_crate_dir.display());
    // Compile frontend crate to webassembly
    assert!(Command::new("cargo")
        .arg("build")
        .arg("--release")
        .arg("--target=wasm32-unknown-unknown")
        .arg("--package=frontend")
        .args(["--target-dir", frontend_target_dir.to_str().unwrap()])
        .status()?
        .success());

    // Generate bindgen wasm and js module
    Bindgen::new()
        .input_path(&frontend_out_dir.join("frontend.wasm"))
        .typescript(false)
        .remove_name_section(true)
        .remove_producers_section(true)
        .demangle(false)
        .web(true)?
        .generate(&frontend_out_dir)?;

    // Export path to bg.wasm and js bindings
    println!(
        "cargo:rustc-env=JS_BLOB_PATH={}",
        frontend_out_dir.join("frontend.js").display()
    );
    println!(
        "cargo:rustc-env=WASM_BLOB_PATH={}",
        frontend_out_dir.join("frontend_bg.wasm").display()
    );

    Ok(())
}
