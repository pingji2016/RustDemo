use wasm_bindgen::prelude::*;
use image::ImageOutputFormat;
use std::io::Cursor;

#[wasm_bindgen]
extern "C" {
    fn alert(s: &str);
}

#[wasm_bindgen]
pub fn greet(name: &str) -> String {
    format!("Hello, {}! From Rust WASM.", name)
}

#[wasm_bindgen]
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

// 接收图片字节数组，转为灰度图，返回新的字节数组
#[wasm_bindgen]
pub fn grayscale(data: &[u8]) -> Result<Vec<u8>, JsValue> {
    // 1. 加载图片
    let img = image::load_from_memory(data)
        .map_err(|e| JsValue::from_str(&format!("加载图片失败: {}", e)))?;

    // 2. 转灰度
    let gray_img = img.grayscale();

    // 3. 写回内存缓冲区（格式为 PNG）
    let mut buffer = Cursor::new(Vec::new());
    gray_img.write_to(&mut buffer, ImageOutputFormat::Png)
        .map_err(|e| JsValue::from_str(&format!("导出图片失败: {}", e)))?;

    Ok(buffer.into_inner())
}
