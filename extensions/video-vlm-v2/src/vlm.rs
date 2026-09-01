//! VLM analyzer: calls the on-board LFM2.5-VL (rkllm3-server OpenAI API)
//! with a frame + prompt, returns the natural-language description.
use base64::Engine;
use serde_json::json;

/// VLM analyzer that sends frames to the on-board rkllm3-server.
pub struct VlmAnalyzer {
    endpoint: String,
    http_agent: ureq::Agent,
    model: String,
}

/// Result of a single VLM analysis.
#[derive(Debug, Clone)]
pub struct VlmResult {
    pub text: String,
    pub elapsed_ms: u64,
}

impl Default for VlmAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl VlmAnalyzer {
    pub fn new() -> Self {
        let endpoint = std::env::var("VLM_VIDEO_ENDPOINT")
            .unwrap_or_else(|_| "http://localhost:8080/v1/chat/completions".into());
        let model = std::env::var("VLM_VIDEO_MODEL")
            .unwrap_or_else(|_| "LFM2.5-VL-3B-llm-16k.rknn".into());
        let http_agent = ureq::Agent::new();
        Self { endpoint, http_agent, model }
    }

    /// Analyze a frame (RGB image) with the given prompt. Blocking — call
    /// from a dedicated thread only.
    pub fn analyze(&self, img: &image::RgbImage, prompt: &str, max_tokens: u32) -> Result<VlmResult, String> {
        let t0 = std::time::Instant::now();

        // Letterbox to square BEFORE encoding: the rkllm server resizes the
        // image to its configured square (--img-width 512 --img-height 512)
        // WITHOUT preserving aspect — non-square inputs reach the model
        // distorted (squashed faces/objects, wrong scene geometry). Padding
        // with black bars here makes the server's square resize a no-op for
        // geometry, so the model sees the original proportions.
        let w = img.width().max(1);
        let h = img.height().max(1);
        let side = w.max(h);
        let mut canvas = image::RgbImage::from_pixel(side, side, image::Rgb([0, 0, 0]));
        image::imageops::overlay(&mut canvas, img, ((side - w) / 2) as i64, ((side - h) / 2) as i64);

        // Encode frame to JPEG base64 (quality 80 — good balance for VLM)
        let mut cursor = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(canvas)
            .write_to(&mut cursor, image::ImageFormat::Jpeg)
            .map_err(|e| format!("JPEG encode: {e}"))?;
        let b64 = base64::engine::general_purpose::STANDARD.encode(cursor.into_inner());

        let body = json!({
            "model": self.model,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "image_url", "image_url": {"url": format!("data:image/jpeg;base64,{}", b64)}},
                    {"type": "text", "text": prompt}
                ]
            }],
            "max_tokens": max_tokens,
            "stream": false,
        });

        let resp = self.http_agent
            .post(&self.endpoint)
            .set("Content-Type", "application/json")
            .send_string(&body.to_string())
            .map_err(|e| format!("VLM request: {e}"))?;
        let parsed: serde_json::Value = serde_json::from_str(&resp.into_string().map_err(|e| format!("VLM response read: {e}"))?)
            .map_err(|e| format!("VLM response parse: {e}"))?;

        let text = parsed
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|c| c.first())
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .unwrap_or("")
            .to_string();

        Ok(VlmResult { text, elapsed_ms: t0.elapsed().as_millis() as u64 })
    }
}
