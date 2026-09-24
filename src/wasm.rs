use wasm_bindgen::prelude::*;

use crate::compression::inflater::Inflater;

#[cfg(not(target_feature = "atomics"))]
#[global_allocator]
static TALC: talc::wasm::WasmDynamicTalc = talc::wasm::new_wasm_dynamic_allocator();

/// Decodes/Decompresses a Deflate stream.
///
/// This function supports:
///
/// - Stored blocks
/// - Fixed Huffman blocks
/// - Dynamic Huffman blocks
#[wasm_bindgen]
pub fn decompress(data: &[u8]) -> Result<Vec<u8>, JsError> {
    Inflater::new(data).inflate().map_err(|e| JsError::new(&e))
}
