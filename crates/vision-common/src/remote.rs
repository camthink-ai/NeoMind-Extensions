//! Remote inference over HTTP — for models that run outside this process
//! (rkllm3-server on Rockchip boards, PaddleOCR-VL FastAPI, any
//! OpenAI-compatible VLM endpoint). Ported from video-vlm/vlm.rs.

use std::time::Duration;

use base64::Engine;
use image::DynamicImage;
use serde_json::{json, Value};

#[derive(Debug, thiserror::Error)]
pub enum RemoteError {
    #[error("http: {0}")]
    Http(String),
    #[error("image: {0}")]
    Image(String),
    #[error("protocol: {0}")]
    Protocol(String),
}

/// Configuration for a remote engine instance.
#[derive(Debug, Clone)]
pub struct RemoteEngine {
    /// Base endpoint, e.g. `http://127.0.0.1:8080`.
    pub endpoint: String,
    /// Model name reported to the server (e.g. `LFM2.5-VL-3B.rknn`).
    pub model: String,
    /// Optional square letterbox before sending — some board servers
    /// (`--img-width 512 --img-height 512`) resize without keeping aspect
    /// ratio, which squashes the picture unless we pad first.
    pub letterbox: Option<u32>,
    pub timeout_secs: u64,
}

impl Default for RemoteEngine {
    fn default() -> Self {
        Self {
            endpoint: "http://127.0.0.1:8080".into(),
            model: "default".into(),
            letterbox: None,
            timeout_secs: 120,
        }
    }
}

impl RemoteEngine {
    pub fn new(endpoint: &str, model: &str) -> Self {
        Self {
            endpoint: endpoint.trim_end_matches('/').to_string(),
            model: model.to_string(),
            ..Default::default()
        }
    }

    /// Encode an image as a base64 data URL, optionally letterboxed square.
    pub fn encode_image(&self, img: &DynamicImage, jpeg_quality: u8) -> Result<String, RemoteError> {
        let prepared;
        let borrowed: &DynamicImage = if let Some(size) = self.letterbox {
            prepared = DynamicImage::ImageRgb8(crate::image::fit_adaptive(img, size, size).0);
            &prepared
        } else {
            img
        };
        let jpeg = crate::image::encode_jpeg(borrowed, jpeg_quality)
            .map_err(|e| RemoteError::Image(e.to_string()))?;
        Ok(format!(
            "data:image/jpeg;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(jpeg)
        ))
    }

    /// OpenAI-compatible `/v1/chat/completions` with one image + one prompt.
    /// Returns the assistant message content.
    pub fn chat(&self, image_data_url: &str, prompt: &str, max_tokens: u32) -> Result<String, RemoteError> {
        let url = format!("{}/v1/chat/completions", self.endpoint);
        let body = json!({
            "model": self.model,
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "image_url", "image_url": { "url": image_data_url } },
                    { "type": "text", "text": prompt },
                ],
            }],
            "max_tokens": max_tokens,
            "stream": false,
        });
        let resp = self.post_json(&url, &body)?;
        resp.get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| RemoteError::Protocol(format!("no choices[0].message.content in response")))
    }

    /// Generic JSON POST; returns the parsed JSON body.
    pub fn post_json(&self, url: &str, body: &Value) -> Result<Value, RemoteError> {
        let resp = ureq::post(url)
            .timeout(Duration::from_secs(self.timeout_secs))
            .send_json(body.clone())
            .map_err(|e| RemoteError::Http(e.to_string()))?;
        let status = resp.status();
        if status >= 400 {
            let body = resp.into_string().unwrap_or_default();
            return Err(RemoteError::Http(format!("HTTP {status}: {body}")));
        }
        resp.into_json()
            .map_err(|e| RemoteError::Http(format!("bad JSON response: {e}")))
    }

    /// Generic GET; for health probes.
    pub fn get_json(&self, url: &str) -> Result<Value, RemoteError> {
        let resp = ureq::get(url)
            .timeout(Duration::from_secs(10))
            .call()
            .map_err(|e| RemoteError::Http(e.to_string()))?;
        let status = resp.status();
        if status >= 400 {
            return Err(RemoteError::Http(format!("HTTP {status} for {url}")));
        }
        resp.into_json()
            .map_err(|e| RemoteError::Http(format!("bad JSON response: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView as _;
    use image::RgbImage;

    #[test]
    fn chat_request_shape() {
        // No server needed: build the same payload the method sends.
        let engine = RemoteEngine::new("http://x", "m");
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(10, 10, image::Rgb([1, 2, 3])));
        let data_url = engine.encode_image(&img, 60).unwrap();
        assert!(data_url.starts_with("data:image/jpeg;base64,"));

        // build the request body exactly as chat() does and check shape
        let body = json!({
            "model": engine.model,
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "image_url", "image_url": { "url": data_url } },
                    { "type": "text", "text": "describe" },
                ],
            }],
            "max_tokens": 256u32,
            "stream": false,
        });
        assert_eq!(body["messages"][0]["content"][0]["type"], "image_url");
        assert_eq!(body["stream"], false);
    }

    #[test]
    fn letterbox_squares_the_image() {
        let engine = RemoteEngine {
            letterbox: Some(32),
            ..Default::default()
        };
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(16, 8, image::Rgb([255, 0, 0])));
        let url = engine.encode_image(&img, 80).unwrap();
        let (_, bytes) = crate::image::decode_data_url(&url).unwrap();
        let back = crate::image::decode(&bytes).unwrap();
        assert_eq!(back.dimensions(), (32, 32));
    }

    #[test]
    fn endpoint_trailing_slash_trimmed() {
        let e = RemoteEngine::new("http://h:1/", "m");
        assert_eq!(e.endpoint, "http://h:1");
    }
}
