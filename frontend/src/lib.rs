use animation_lang::program::Program;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(getter_with_clone)]
pub struct CompileResult(pub Vec<u8>, pub String);

#[wasm_bindgen]
pub fn compile_prog(source: &str) -> Result<CompileResult, JsValue> {
    let program = Program::from_source(source).map_err(|e| e.to_string())?;

    Ok(CompileResult(
        program.code().to_vec(),
        format!("{:?}", program),
    ))
}
