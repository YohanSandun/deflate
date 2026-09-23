use wasm_bindgen::prelude::*;

use crate::compression::inflater::Inflater;

#[cfg(not(target_feature = "atomics"))]
#[global_allocator]
static TALC: talc::wasm::WasmDynamicTalc = talc::wasm::new_wasm_dynamic_allocator();

/// Decompresses raw DEFLATE data (RFC 1951, no zlib or gzip wrapper).
#[wasm_bindgen]
pub fn decompress(data: &[u8]) -> Result<Vec<u8>, JsError> {
    Inflater::new(data).inflate().map_err(|e| JsError::new(&e))
}
