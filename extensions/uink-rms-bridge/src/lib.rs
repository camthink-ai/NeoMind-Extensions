//! NeoMind Uink-RMS Bridge Extension
//!
//! Bridges Uink-RMS e-paper display platform with NeoMind device system.
//!
//! Features:
//! - JWT authentication with automatic refresh token renewal
//! - Device template registration (uink_epaper)
//! - Batch device sync from RMS to NeoMind
//! - Telemetry collection via dedicated telemetry endpoint
//! - Image push to e-paper displays (multipart/form-data)
//! - Content-to-image conversion (text, markdown → image → push)
//!
//! # Uink-RMS API v1.0.1 Compliance
//!
//! - Auth: POST /api/v1/login (email+password), POST /api/v1/auth/refresh
//! - Devices: GET /api/v1/devices (paginated), GET /api/v1/devices/{id}/telemetry
//! - Image: POST /api/v1/devices/{id}/image (multipart/form-data)
//!
//! # Architecture
//!
//! Uses sync HTTP client (ureq) to avoid Tokio runtime issues in dynamic libraries.
//! Metrics are cached in atomic types for synchronous produce_metrics() access.

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use async_trait::async_trait;
use chrono::Utc;
use image::{ImageBuffer, Rgb};
use imageproc::drawing::draw_text_mut;
use neomind_extension_sdk::{
    json, CapabilityContext, Extension, ExtensionCommand, ExtensionError, ExtensionMetricValue,
    ExtensionMetadata, MetricDataType, MetricDescriptor, ParameterDefinition, ParamMetricValue,
    Result,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::sync::atomic::{AtomicI64, Ordering};

// ============================================================================
// RMS API Types (v1.0.1 compliant)
// ============================================================================

/// POST /api/v1/login request
#[derive(Debug, Clone, Serialize)]
struct RmsLoginRequest {
    email: String,
    password: String,
}

/// POST /api/v1/login response
#[derive(Debug, Clone, Deserialize)]
struct RmsLoginResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    refresh_expires_in: Option<i64>,
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    email: Option<String>,
}

/// POST /api/v1/auth/refresh response
#[derive(Debug, Clone, Deserialize)]
struct RmsRefreshResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    refresh_expires_in: Option<i64>,
}

/// GET /api/v1/devices response — device info
#[derive(Debug, Clone, Deserialize)]
struct RmsDeviceInfo {
    device_id: String,
    #[serde(default)]
    sn: Option<String>,
    name: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    online_status: Option<String>,
    #[serde(default)]
    activation_status: Option<String>,
    #[serde(default)]
    alarm_status: Option<String>,
    #[serde(default)]
    firmware_version: Option<String>,
    #[serde(default)]
    hardware_version: Option<String>,
    #[serde(default)]
    last_sync_at: Option<i64>,
    #[serde(default)]
    created_at: Option<i64>,
    #[serde(default)]
    updated_at: Option<i64>,
}

/// GET /api/v1/devices response
#[derive(Debug, Clone, Deserialize)]
struct RmsDeviceListResponse {
    data: Vec<RmsDeviceInfo>,
    pagination: Option<RmsPagination>,
}

#[derive(Debug, Clone, Deserialize)]
struct RmsPagination {
    #[allow(dead_code)]
    page: i64,
    #[allow(dead_code)]
    limit: i64,
    total: i64,
}

/// GET /api/v1/devices/{id}/telemetry response
#[derive(Debug, Clone, Deserialize)]
struct RmsTelemetryResponse {
    telemetry: RmsTelemetryData,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct RmsTelemetryData {
    #[serde(default)]
    battery: Option<i64>,
    #[serde(default)]
    temperature: Option<f64>,
    #[serde(default)]
    signal_strength: Option<i64>,
    #[serde(default)]
    refresh_count: Option<i64>,
}

/// POST /api/v1/devices/{id}/image response (Swagger: SendImageToDeviceResponse)
#[derive(Debug, Clone, Deserialize)]
struct RmsImageResponse {
    #[serde(default)]
    image_url: Option<String>,
    #[serde(default)]
    width: Option<i64>,
    #[serde(default)]
    height: Option<i64>,
    #[serde(default)]
    dither_algorithm: Option<String>,
    #[serde(default)]
    resize_mode: Option<String>,
    #[serde(default)]
    padding_color: Option<String>,
}

// ============================================================================
// Display Size Mapping
// ============================================================================

/// Known UINK model → (width, height) resolution mapping
fn model_to_resolution(model: &str) -> Option<(u32, u32)> {
    let model_upper = model.to_uppercase();
    // Common UINK e-paper models
    match model_upper.as_str() {
        m if m.contains("2.13") => Some((250, 122)),
        m if m.contains("2.9") => Some((296, 128)),
        m if m.contains("4.2") => Some((400, 300)),
        m if m.contains("5.65") => Some((600, 448)),
        m if m.contains("5.83") => Some((648, 480)),
        m if m.contains("7.5") => Some((800, 480)),
        m if m.contains("10.2") => Some((960, 640)),
        m if m.contains("13.3") => Some((960, 680)),
        _ => None,
    }
}

/// Default display resolution when model is unknown
const DEFAULT_RESOLUTION: (u32, u32) = (800, 480);

// ============================================================================
// System Font Loading
// ============================================================================

/// Font search paths for macOS and Linux (CJK-capable fonts first)
const FONT_PATHS: &[&str] = &[
    // macOS — PingFang (Chinese + Latin)
    "/System/Library/Fonts/PingFang.ttc",
    "/System/Library/Fonts/Supplemental/PingFang.ttc",
    // macOS — STHeiti (华文黑体)
    "/System/Library/Fonts/STHeiti Light.ttc",
    // macOS — Hiragino Sans GB (冬青黑体)
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    // macOS — fallback Latin
    "/System/Library/Fonts/Helvetica.ttc",
    // macOS — SF Mono (monospace, .ttf)
    "/System/Library/Fonts/SFNSMono.ttf",
    // Linux — Noto Sans CJK (multiple distro paths)
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
    // Linux — Noto Sans SC (single-language .ttf, lighter)
    "/usr/share/fonts/truetype/noto/NotoSansSC-Regular.ttf",
    "/usr/share/fonts/opentype/noto/NotoSansSC-Regular.otf",
    // Linux — DejaVu (Latin only, .ttf fallback)
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
];

/// Load a system font for text rendering. Returns font data bytes.
fn load_system_font_data() -> Result<Vec<u8>> {
    for path in FONT_PATHS {
        if let Ok(data) = std::fs::read(path) {
            eprintln!("[uink-rms-bridge] Loaded font: {}", path);
            return Ok(data);
        }
    }
    Err(ExtensionError::ExecutionFailed(
        "No suitable system font found. Install CJK fonts (PingFang/Noto Sans CJK)".to_string(),
    ))
}

// ============================================================================
// Text → Image Rendering
// ============================================================================

// ============================================================================
// Markdown Structured Rendering
// ============================================================================

/// A styled text block produced by markdown parsing
enum TextBlock {
    Heading { level: u8, text: String },
    Paragraph { parts: Vec<TextPart> },
}

/// Inline text styling within a paragraph
enum TextPart {
    Plain(String),
    Bold(String),
    Code(String),
}

/// Parse markdown into structured text blocks for rendering
fn parse_markdown(md: &str) -> Vec<TextBlock> {
    use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    let parser = Parser::new_ext(md, opts);

    let mut blocks: Vec<TextBlock> = Vec::new();
    let mut in_heading: Option<u8> = None;
    let mut heading_text = String::new();
    let mut in_paragraph = false;
    let mut paragraph_parts: Vec<TextPart> = Vec::new();
    let mut in_strong = false;
    let mut strong_text = String::new();
    let mut in_code = false;
    let mut code_text = String::new();
    let mut in_list_item = false;

    for event in parser {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                in_heading = Some(level as u8);
                heading_text.clear();
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(level) = in_heading.take() {
                    blocks.push(TextBlock::Heading {
                        level,
                        text: heading_text.trim().to_string(),
                    });
                }
            }
            Event::Start(Tag::Paragraph) => {
                in_paragraph = true;
                paragraph_parts.clear();
            }
            Event::End(TagEnd::Paragraph) => {
                in_paragraph = false;
                if !paragraph_parts.is_empty() {
                    blocks.push(TextBlock::Paragraph {
                        parts: std::mem::take(&mut paragraph_parts),
                    });
                }
            }
            Event::Start(Tag::Strong) => {
                in_strong = true;
                strong_text.clear();
            }
            Event::End(TagEnd::Strong) => {
                in_strong = false;
                let text = std::mem::take(&mut strong_text);
                if !text.is_empty() {
                    if let Some(TextBlock::Heading { .. }) = in_heading.and_then(|_| blocks.last()) {
                        // ignore: heading doesn't support inline styles in this renderer
                    } else if in_heading.is_some() {
                        // just append to heading text
                        heading_text.push_str(&text);
                    } else {
                        paragraph_parts.push(TextPart::Bold(text));
                    }
                }
            }
            Event::Start(Tag::CodeBlock(_)) => {
                // Fenced code blocks — treat content as plain text
                in_code = true;
                code_text.clear();
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code = false;
                let text = std::mem::take(&mut code_text);
                if !text.is_empty() {
                    if in_heading.is_some() {
                        heading_text.push_str(&text);
                    } else {
                        paragraph_parts.push(TextPart::Code(text));
                    }
                }
            }
            Event::Start(Tag::List(_)) => {}
            Event::Start(Tag::Item) => {
                in_list_item = true;
                // Start a paragraph-like block for the list item
                in_paragraph = true;
                paragraph_parts.clear();
                // Add bullet prefix
                paragraph_parts.push(TextPart::Plain("• ".to_string()));
            }
            Event::End(TagEnd::Item) => {
                in_list_item = false;
                in_paragraph = false;
                if !paragraph_parts.is_empty() {
                    blocks.push(TextBlock::Paragraph {
                        parts: std::mem::take(&mut paragraph_parts),
                    });
                }
            }
            Event::Text(t) => {
                if in_code {
                    code_text.push_str(&t);
                } else if in_strong {
                    strong_text.push_str(&t);
                } else if let Some(_) = in_heading {
                    heading_text.push_str(&t);
                } else {
                    paragraph_parts.push(TextPart::Plain(t.into_string()));
                }
            }
            Event::Code(c) => {
                if in_heading.is_some() {
                    heading_text.push_str(&c);
                } else {
                    paragraph_parts.push(TextPart::Code(c.into_string()));
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if in_heading.is_some() {
                    heading_text.push(' ');
                } else if in_paragraph {
                    // Add a space within paragraph (line break in source)
                    paragraph_parts.push(TextPart::Plain(" ".to_string()));
                }
            }
            _ => {}
        }
    }

    blocks
}

/// Word-wrap a single line of text to fit within `max_width_px` pixels.
/// Handles both CJK characters (can break anywhere) and Latin words.
fn wrap_line<F: Font>(
    line: &str,
    font: &F,
    scale: PxScale,
    max_width_px: f32,
) -> Vec<String> {
    if line.is_empty() {
        return vec![String::new()];
    }

    let scaled_font = font.as_scaled(scale);
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0.0f32;

    for ch in line.chars() {
        let glyph = scaled_font.scaled_glyph(ch);
        let advance = scaled_font.h_advance(glyph.id);

        if current_width + advance > max_width_px && !current.is_empty() {
            lines.push(current.clone());
            current.clear();
            current_width = 0.0;
        }

        current.push(ch);
        current_width += advance;
    }

    if !current.is_empty() {
        lines.push(current);
    }

    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}

/// Render text to a PNG image buffer with word wrapping and CJK support.
fn render_text_to_image(
    text: &str,
    width: u32,
    height: u32,
    font_data: &[u8],
) -> Result<Vec<u8>> {
    let font = load_font(font_data)?;

    // Calculate font size based on display dimensions
    let line_height = (height as f32 / 20.0).min(48.0).max(16.0);
    let font_size = line_height * 0.75;
    let scale = PxScale::from(font_size);

    // Use generous margin (8% of width) to prevent character clipping on e-paper
    let margin_x = (width as f32 * 0.08).max(20.0) as u32;
    let margin_y = (height as f32 * 0.06).max(16.0) as u32;
    let text_width = width - margin_x * 2;

    let mut img = ImageBuffer::<Rgb<u8>, Vec<u8>>::from_pixel(
        width, height, Rgb([255, 255, 255]),
    );

    let mut wrapped_lines = Vec::new();
    for line in text.split('\n') {
        let wrapped = wrap_line(line, &font, scale, text_width as f32);
        wrapped_lines.extend(wrapped);
    }

    let text_color = Rgb([0, 0, 0]);
    let max_lines = ((height - margin_y * 2) as f32 / line_height).floor() as usize;

    for (i, line) in wrapped_lines.iter().take(max_lines).enumerate() {
        let y = margin_y as i32 + (i as f32 * line_height).round() as i32;
        if y + line_height as i32 > height as i32 - margin_y as i32 {
            break;
        }
        draw_text_mut(&mut img, text_color, margin_x as i32, y, scale, &font, line);
    }

    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)
        .map_err(|e| ExtensionError::ExecutionFailed(format!("PNG encode failed: {}", e)))?;

    Ok(buf)
}

/// Load font data using FontRef (handles TTC files by loading index 0)
fn load_font(font_data: &[u8]) -> Result<FontRef<'_>> {
    FontRef::try_from_slice(font_data)
        .map_err(|e| ExtensionError::ExecutionFailed(format!("Font parse failed: {}", e)))
}

/// Render markdown to a PNG image buffer with structured formatting.
/// Headings use different font sizes, bold text is rendered heavier.
fn render_markdown_to_image(
    md: &str,
    width: u32,
    height: u32,
    font_data: &[u8],
) -> Result<Vec<u8>> {
    let font = load_font(font_data)?;
    let blocks = parse_markdown(md);

    let margin_x = (width as f32 * 0.08).max(20.0) as u32;
    let margin_y = (height as f32 * 0.06).max(16.0) as u32;
    let text_width = width - margin_x * 2;
    let base_font_size = (height as f32 / 20.0).min(48.0).max(16.0) * 0.75;

    let mut img = ImageBuffer::<Rgb<u8>, Vec<u8>>::from_pixel(
        width, height, Rgb([255, 255, 255]),
    );

    let text_color = Rgb([0, 0, 0]);
    let _code_bg = Rgb([240, 240, 240]);
    let mut y_pos = margin_y as f32;

    for block in &blocks {
        if y_pos as u32 + margin_y > height {
            break; // Out of vertical space
        }

        match block {
            TextBlock::Heading { level, text } => {
                // H1 = 2.0x base, decreasing by 0.2 per level
                let size_mult = 2.0f32.max(2.0 - (*level as f32 - 1.0) * 0.2);
                let font_size = base_font_size * size_mult;
                let line_height = font_size * 1.35;
                let scale = PxScale::from(font_size);

                let wrapped = wrap_line(text, &font, scale, text_width as f32);
                for line in &wrapped {
                    if y_pos as u32 + line_height as u32 > height - margin_y {
                        break;
                    }
                    draw_text_mut(
                        &mut img, text_color,
                        margin_x as i32, y_pos as i32,
                        scale, &font, line,
                    );
                    y_pos += line_height;
                }
                // Extra spacing after heading
                y_pos += base_font_size * 0.3;
            }
            TextBlock::Paragraph { parts } => {
                let font_size = base_font_size;
                let line_height = font_size * 1.35;
                let scale = PxScale::from(font_size);

                // Build runs: (text, is_bold)
                let mut runs: Vec<(String, bool)> = Vec::new();
                for part in parts {
                    match part {
                        TextPart::Plain(s) => runs.push((s.clone(), false)),
                        TextPart::Bold(s) => runs.push((s.clone(), true)),
                        TextPart::Code(s) => {
                            // Render code with a background box (simplified: just prefix with `)
                            runs.push((format!("`{}`", s), false))
                        }
                    }
                }

                // Flatten runs into wrapped lines, preserving bold markers
                // We render character by character to handle bold via double-strike
                let full_text: String = runs.iter().map(|(t, bold)| {
                    if *bold { format!("**{}**", t) } else { t.clone() }
                }).collect();

                // Simplified: render as plain text with bold indicated by larger weight
                // Since ab_glyph doesn't support bold variants, we double-strike bold text
                let mut plain_text = String::new();
                let mut bold_spans: Vec<(usize, usize)> = Vec::new(); // (start, end) in plain_text
                let mut bold_start = 0usize;
                let mut in_bold_marker = false;
                let chars: Vec<char> = full_text.chars().collect();
                let mut i = 0;
                while i < chars.len() {
                    if i + 1 < chars.len() && chars[i] == '*' && chars[i + 1] == '*' {
                        if in_bold_marker {
                            // End of bold
                            bold_spans.push((bold_start, plain_text.len()));
                            in_bold_marker = false;
                        } else {
                            // Start of bold
                            bold_start = plain_text.len();
                            in_bold_marker = true;
                        }
                        i += 2;
                    } else {
                        plain_text.push(chars[i]);
                        i += 1;
                    }
                }

                let wrapped = wrap_line(&plain_text, &font, scale, text_width as f32);

                // Calculate character offsets for each wrapped line
                let mut char_offset = 0usize;
                for line in &wrapped {
                    if y_pos as u32 + line_height as u32 > height - margin_y {
                        break;
                    }

                    // Draw code background for any code portions (simplified: skip for now)
                    // Just draw text, with double-strike for bold characters
                    draw_text_mut(
                        &mut img, text_color,
                        margin_x as i32, y_pos as i32,
                        scale, &font, line,
                    );

                    // Double-strike bold characters (offset by 1px for bold effect)
                    let line_start = char_offset;
                    let line_end = char_offset + line.chars().count();
                    for (bs, be) in &bold_spans {
                        let overlap_start = (*bs).max(line_start);
                        let overlap_end = (*be).min(line_end);
                        if overlap_start < overlap_end {
                            // Extract the bold substring for this line
                            let bold_char_start = overlap_start.saturating_sub(line_start);
                            let bold_char_end = overlap_end.saturating_sub(line_start);
                            let bold_substring: String = line.chars()
                                .skip(bold_char_start)
                                .take(bold_char_end - bold_char_start)
                                .collect();

                            // Calculate x offset for the bold portion
                            let prefix: String = line.chars().take(bold_char_start).collect();
                            // Use actual measurement
                            let scaled = font.as_scaled(scale);
                            let mut x_pos = margin_x as f32;
                            for ch in prefix.chars() {
                                let glyph = scaled.scaled_glyph(ch);
                                x_pos += scaled.h_advance(glyph.id);
                            }

                            // Draw bold text with slight offset for thickness
                            draw_text_mut(
                                &mut img, text_color,
                                (x_pos + 0.8) as i32, y_pos as i32,
                                scale, &font, &bold_substring,
                            );
                        }
                    }

                    char_offset += line.chars().count();
                    y_pos += line_height;
                }
                // Small spacing between paragraphs
                y_pos += base_font_size * 0.15;
            }
        }
    }

    let mut buf = Vec::new();
    img.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)
        .map_err(|e| ExtensionError::ExecutionFailed(format!("PNG encode failed: {}", e)))?;

    Ok(buf)
}

// ============================================================================
// Extension State
// ============================================================================

/// Per-device cached metrics
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
struct DeviceMetrics {
    battery: i64,
    temperature: f64,
    signal_strength: i64,
    refresh_count: i64,
    online: bool,
    last_updated: i64,
}

/// GET /devices/{id}/display response (Swagger: DisplaySlotInfosResponse)
#[derive(Debug, Clone, Deserialize)]
struct RmsDisplayResponse {
    #[serde(default)]
    data: Vec<RmsDisplayInfo>,
}

/// Display info for a single display slot (Swagger: DisplayInfo)
#[derive(Debug, Clone, Deserialize)]
struct RmsDisplayInfo {
    #[serde(default)]
    preview_url: Option<String>,
    #[serde(default)]
    preview_thumbnail_url: Option<String>,
    #[serde(default)]
    is_pending: Option<bool>,
    #[serde(default)]
    pending_preview_url: Option<String>,
    #[serde(default)]
    pending_preview_thumbnail_url: Option<String>,
    #[serde(default)]
    image_id: Option<i64>,
    #[serde(default)]
    refresh_count: Option<i64>,
}

/// Extension configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UinkConfig {
    pub server_region: String,
    pub custom_server_url: String,
    pub email: String,
    pub password: String,
    #[serde(default = "default_sync_interval")]
    pub sync_interval_secs: u64,
    #[serde(default = "default_poll_interval")]
    pub poll_interval_secs: u64,
}

fn default_sync_interval() -> u64 { 300 }
fn default_poll_interval() -> u64 { 60 }

impl Default for UinkConfig {
    fn default() -> Self {
        Self {
            server_region: "China".to_string(),
            custom_server_url: String::new(),
            email: String::new(),
            password: String::new(),
            sync_interval_secs: 300,
            poll_interval_secs: 60,
        }
    }
}

impl UinkConfig {
    fn api_base_url(&self) -> String {
        match self.server_region.as_str() {
            "China" => "https://cn.rms.uink.com".to_string(),
            "Europe" => "https://eu.rms.uink.com".to_string(),
            _ => self.custom_server_url.trim_end_matches('/').to_string(),
        }
    }
}

pub struct UinkRmsBridge {
    config: RwLock<UinkConfig>,
    access_token: RwLock<Option<String>>,
    refresh_token: RwLock<Option<String>>,
    token_expiry: AtomicI64,
    device_metrics: RwLock<HashMap<String, DeviceMetrics>>,
    device_ids: RwLock<Vec<String>>,
    neo_to_rms_id: RwLock<HashMap<String, String>>,
    rms_device_names: RwLock<HashMap<String, String>>,
    /// Cached display sizes: RMS device_id → (width, height)
    display_sizes: RwLock<HashMap<String, (u32, u32)>>,
    /// Cached font data (loaded once)
    cached_font: RwLock<Option<Vec<u8>>>,
    last_sync_ts: AtomicI64,
    last_poll_ts: AtomicI64,
    /// Timestamp of last login failure (for backoff)
    last_login_failure_ts: AtomicI64,
    template_registered: AtomicI64, // 0 = not registered, 1 = registered
    total_sync_count: AtomicI64,
    total_push_count: AtomicI64,
    total_error_count: AtomicI64,
    last_error: RwLock<Option<String>>,
}

impl UinkRmsBridge {
    pub fn new() -> Self {
        Self {
            config: RwLock::new(UinkConfig::default()),
            access_token: RwLock::new(None),
            refresh_token: RwLock::new(None),
            token_expiry: AtomicI64::new(0),
            device_metrics: RwLock::new(HashMap::new()),
            device_ids: RwLock::new(Vec::new()),
            neo_to_rms_id: RwLock::new(HashMap::new()),
            rms_device_names: RwLock::new(HashMap::new()),
            display_sizes: RwLock::new(HashMap::new()),
            cached_font: RwLock::new(None),
            last_sync_ts: AtomicI64::new(0),
            last_poll_ts: AtomicI64::new(0),
            last_login_failure_ts: AtomicI64::new(0),
            template_registered: AtomicI64::new(0),
            total_sync_count: AtomicI64::new(0),
            total_push_count: AtomicI64::new(0),
            total_error_count: AtomicI64::new(0),
            last_error: RwLock::new(None),
        }
    }

    /// Get cached font data, loading from system on first access
    fn get_font_data(&self) -> Result<Vec<u8>> {
        {
            let cached = self.cached_font.read();
            if let Some(data) = cached.as_ref() {
                return Ok(data.clone());
            }
        }
        let data = load_system_font_data()?;
        *self.cached_font.write() = Some(data.clone());
        Ok(data)
    }

    /// Get display size for a device, falling back to model mapping then default
    fn get_display_size(&self, rms_device_id: &str) -> (u32, u32) {
        let sizes = self.display_sizes.read();
        sizes.get(rms_device_id).copied().unwrap_or(DEFAULT_RESOLUTION)
    }

    // ========================================================================
    // Authentication
    // ========================================================================

    fn ensure_token(&self) -> Result<()> {
        let now = Utc::now().timestamp();
        let expiry = self.token_expiry.load(Ordering::SeqCst);
        if expiry - now > 120 && self.access_token.read().is_some() {
            return Ok(());
        }

        // Backoff: if login failed recently, wait at least 5 minutes before retrying
        let last_failure = self.last_login_failure_ts.load(Ordering::SeqCst);
        if last_failure > 0 && now - last_failure < 300 {
            return Err(ExtensionError::ExecutionFailed(format!(
                "Login retry backoff ({}s remaining)",
                300 - (now - last_failure)
            )));
        }

        if self.refresh_token.read().is_some() {
            if self.refresh().is_ok() {
                self.last_login_failure_ts.store(0, Ordering::SeqCst);
                return Ok(());
            }
        }
        let result = self.login();
        if result.is_err() {
            self.last_login_failure_ts.store(now, Ordering::SeqCst);
        } else {
            self.last_login_failure_ts.store(0, Ordering::SeqCst);
        }
        result
    }

    fn login(&self) -> Result<()> {
        let config = self.config.read();
        let base_url = config.api_base_url();
        if base_url.is_empty() {
            return Err(ExtensionError::ExecutionFailed("RMS server not configured".into()));
        }
        let url = format!("{}/api/v1/login", base_url);
        let body = RmsLoginRequest {
            email: config.email.clone(),
            password: config.password.clone(),
        };
        let response: RmsLoginResponse = ureq::post(&url)
            .send_json(&body)
            .map_err(|e| { self.total_error_count.fetch_add(1, Ordering::SeqCst); ExtensionError::ExecutionFailed(format!("Login failed: {}", e)) })?
            .into_json()
            .map_err(|e| { ExtensionError::ExecutionFailed(format!("Parse login response: {}", e)) })?;

        *self.access_token.write() = Some(response.access_token);
        *self.refresh_token.write() = response.refresh_token;
        let expires_in = response.expires_in.unwrap_or(3600);
        self.token_expiry.store(Utc::now().timestamp() + expires_in - 120, Ordering::SeqCst);
        eprintln!("[uink-rms-bridge] Logged in as {} (token expires in {}s)", response.email.as_deref().unwrap_or("unknown"), expires_in);
        Ok(())
    }

    fn refresh(&self) -> Result<()> {
        let rt = self.refresh_token.read().clone().ok_or_else(|| ExtensionError::ExecutionFailed("No refresh token".into()))?;
        let config = self.config.read();
        let url = format!("{}/api/v1/auth/refresh", config.api_base_url());
        let body = json!({ "refresh_token": rt });
        let response: RmsRefreshResponse = ureq::post(&url)
            .send_json(&body)
            .map_err(|e| { eprintln!("[uink-rms-bridge] Token refresh failed: {}", e); ExtensionError::ExecutionFailed(format!("Token refresh failed: {}", e)) })?
            .into_json().map_err(|e| ExtensionError::ExecutionFailed(format!("Parse refresh response: {}", e)))?;
        *self.access_token.write() = Some(response.access_token);
        *self.refresh_token.write() = response.refresh_token;
        let expires_in = response.expires_in.unwrap_or(3600);
        self.token_expiry.store(Utc::now().timestamp() + expires_in - 120, Ordering::SeqCst);
        eprintln!("[uink-rms-bridge] Token refreshed (expires in {}s)", expires_in);
        Ok(())
    }

    fn auth_header(&self) -> Result<String> {
        self.ensure_token()?;
        let token = self.access_token.read().clone().ok_or_else(|| ExtensionError::ExecutionFailed("No auth token".into()))?;
        Ok(format!("Bearer {}", token))
    }

    // ========================================================================
    // Device Sync
    // ========================================================================

    fn fetch_rms_devices(&self) -> Result<Vec<RmsDeviceInfo>> {
        let config = self.config.read();
        let base_url = config.api_base_url();
        if base_url.is_empty() {
            return Err(ExtensionError::ExecutionFailed("RMS server not configured".into()));
        }
        let auth = self.auth_header()?;
        let mut all_devices = Vec::new();
        let mut page = 1;
        loop {
            let url = format!("{}/api/v1/devices?page={}&limit=50", base_url, page);
            let response: RmsDeviceListResponse = ureq::get(&url)
                .set("Authorization", &auth).call()
                .map_err(|e| ExtensionError::ExecutionFailed(format!("Fetch devices failed: {}", e)))?
                .into_json().map_err(|e| ExtensionError::ExecutionFailed(format!("Parse device list: {}", e)))?;
            let total = response.pagination.as_ref().map(|p| p.total).unwrap_or(response.data.len() as i64);
            all_devices.extend(response.data);
            if (all_devices.len() as i64) >= total || page > 100 { break; }
            page += 1;
        }
        Ok(all_devices)
    }

    // ========================================================================
    // Telemetry
    // ========================================================================

    fn fetch_device_telemetry(&self, rms_device_id: &str) -> Result<RmsTelemetryData> {
        let config = self.config.read();
        let auth = self.auth_header()?;
        let url = format!("{}/api/v1/devices/{}/telemetry", config.api_base_url(), rms_device_id);
        let response: RmsTelemetryResponse = ureq::get(&url)
            .set("Authorization", &auth).call()
            .map_err(|e| ExtensionError::ExecutionFailed(format!("Fetch telemetry for {} failed: {}", rms_device_id, e)))?
            .into_json().map_err(|e| ExtensionError::ExecutionFailed(format!("Parse telemetry: {}", e)))?;
        Ok(response.telemetry)
    }

    fn poll_all_telemetry(&self) -> Result<()> {
        let ctx = CapabilityContext::default();
        let rms_ids: Vec<String> = self.neo_to_rms_id.read().values().cloned().collect();
        if rms_ids.is_empty() { return Ok(()); }
        let now = Utc::now().timestamp_millis();
        let mut metrics = self.device_metrics.write();
        let online_map: HashMap<String, bool> = metrics.iter().map(|(k, v)| (k.clone(), v.online)).collect();

        for rms_id in &rms_ids {
            let neo_device_id = format!("uink-{}", rms_id);
            match self.fetch_device_telemetry(rms_id) {
                Ok(telemetry) => {
                    let battery = telemetry.battery.unwrap_or(-1);
                    let temperature = telemetry.temperature.unwrap_or(0.0);
                    let signal_strength = telemetry.signal_strength.unwrap_or(-1);
                    let refresh_count = telemetry.refresh_count.unwrap_or(0);
                    let online = online_map.get(rms_id).copied().unwrap_or(false);

                    // Update internal cache
                    let dm = DeviceMetrics {
                        battery,
                        temperature,
                        signal_strength,
                        refresh_count,
                        online,
                        last_updated: now,
                    };
                    metrics.insert(rms_id.clone(), dm);

                    // Write telemetry to NeoMind device metrics via capability
                    let write_metric = |name: &str, value: &serde_json::Value| {
                        let result = ctx.invoke_capability("device_metrics_write", &json!({
                            "device_id": neo_device_id,
                            "metric": name,
                            "value": value,
                            "timestamp": now,
                        }));
                        if !result.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
                            let err = result.get("error").and_then(|v| v.as_str()).unwrap_or("unknown");
                            eprintln!("[uink-rms-bridge] Failed to write metric {} for {}: {}", name, neo_device_id, err);
                        }
                    };

                    write_metric("battery", &json!(battery));
                    write_metric("temperature", &json!(temperature));
                    write_metric("signal_strength", &json!(signal_strength));
                    write_metric("refresh_count", &json!(refresh_count));

                    // Fetch current screen preview
                    match self.fetch_device_display(rms_id) {
                        Ok(display) => {
                            if let Some(slot) = display.data.first() {
                                if let Some(ref url) = slot.preview_url {
                                    write_metric("preview_url", &json!(url));
                                }
                                if let Some(ref url) = slot.preview_thumbnail_url {
                                    write_metric("preview_thumbnail_url", &json!(url));
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("[uink-rms-bridge] Display fetch failed for {}: {}", rms_id, e);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[uink-rms-bridge] Telemetry failed for {}: {}", rms_id, e);
                    self.total_error_count.fetch_add(1, Ordering::SeqCst);
                }
            }
        }
        self.last_poll_ts.store(Utc::now().timestamp(), Ordering::SeqCst);
        Ok(())
    }

    // ========================================================================
    // Image Download
    // ========================================================================

    /// Download an image from a URL and return the raw bytes
    fn download_image(&self, url: &str) -> Result<Vec<u8>> {
        let response = ureq::get(url)
            .timeout(std::time::Duration::from_secs(30))
            .call()
            .map_err(|e| ExtensionError::ExecutionFailed(format!("Failed to download image: {}", e)))?;

        let mut data = Vec::new();
        response.into_reader()
            .read_to_end(&mut data)
            .map_err(|e| ExtensionError::ExecutionFailed(format!("Failed to read image data: {}", e)))?;

        Ok(data)
    }

    // ========================================================================
    // Image Push (multipart/form-data)
    // ========================================================================

    fn push_image_to_device(
        &self,
        rms_device_id: &str,
        image_data: &[u8],
        dither_algorithm: Option<&str>,
        resize_mode: Option<&str>,
        padding_color: Option<&str>,
    ) -> Result<RmsImageResponse> {
        let config = self.config.read();
        let auth = self.auth_header()?;
        let url = format!("{}/api/v1/devices/{}/image", config.api_base_url(), rms_device_id);

        let boundary = "----NeoMindUinkBoundary7MA4YWxkTrZu0gW";
        let mut body = Vec::new();

        // Image file part
        body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
        body.extend_from_slice(format!("Content-Disposition: form-data; name=\"image\"; filename=\"image.png\"\r\n").as_bytes());
        body.extend_from_slice("Content-Type: image/png\r\n\r\n".as_bytes());
        body.extend_from_slice(image_data);
        body.extend_from_slice("\r\n".as_bytes());

        // Swagger: dither_algorithm
        if let Some(v) = dither_algorithm {
            body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
            body.extend_from_slice("Content-Disposition: form-data; name=\"dither_algorithm\"\r\n\r\n".as_bytes());
            body.extend_from_slice(v.as_bytes());
            body.extend_from_slice("\r\n".as_bytes());
        }
        // Swagger: resize_mode (fit, cover, fill)
        if let Some(v) = resize_mode {
            body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
            body.extend_from_slice("Content-Disposition: form-data; name=\"resize_mode\"\r\n\r\n".as_bytes());
            body.extend_from_slice(v.as_bytes());
            body.extend_from_slice("\r\n".as_bytes());
        }
        // Swagger: padding_color (hex, e.g. FFFFFF)
        if let Some(v) = padding_color {
            body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
            body.extend_from_slice("Content-Disposition: form-data; name=\"padding_color\"\r\n\r\n".as_bytes());
            body.extend_from_slice(v.as_bytes());
            body.extend_from_slice("\r\n".as_bytes());
        }
        body.extend_from_slice(format!("--{}--\r\n", boundary).as_bytes());

        let response: RmsImageResponse = ureq::post(&url)
            .set("Authorization", &auth)
            .set("Content-Type", &format!("multipart/form-data; boundary={}", boundary))
            .send_bytes(&body)
            .map_err(|e| ExtensionError::ExecutionFailed(format!("Push image failed: {}", e)))?
            .into_json().map_err(|e| ExtensionError::ExecutionFailed(format!("Parse push response: {}", e)))?;

        // Cache display size from response
        if let (Some(w), Some(h)) = (response.width, response.height) {
            if w > 0 && h > 0 {
                self.display_sizes.write().insert(rms_device_id.to_string(), (w as u32, h as u32));
            }
        }

        self.total_push_count.fetch_add(1, Ordering::SeqCst);
        Ok(response)
    }

    /// Push raw image directly to device (POST /devices/{id}/image/raw)
    fn push_raw_image_to_device(
        &self,
        rms_device_id: &str,
        image_data: &[u8],
    ) -> Result<RmsImageResponse> {
        let config = self.config.read();
        let auth = self.auth_header()?;
        let url = format!("{}/api/v1/devices/{}/image/raw", config.api_base_url(), rms_device_id);

        let boundary = "----NeoMindUinkBoundary7MA4YWxkTrZu0gW";
        let mut body = Vec::new();

        body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
        body.extend_from_slice(format!("Content-Disposition: form-data; name=\"image\"; filename=\"image.png\"\r\n").as_bytes());
        body.extend_from_slice("Content-Type: image/png\r\n\r\n".as_bytes());
        body.extend_from_slice(image_data);
        body.extend_from_slice("\r\n".as_bytes());
        body.extend_from_slice(format!("--{}--\r\n", boundary).as_bytes());

        let response: RmsImageResponse = ureq::post(&url)
            .set("Authorization", &auth)
            .set("Content-Type", &format!("multipart/form-data; boundary={}", boundary))
            .send_bytes(&body)
            .map_err(|e| ExtensionError::ExecutionFailed(format!("Push raw image failed: {}", e)))?
            .into_json().map_err(|e| ExtensionError::ExecutionFailed(format!("Parse push response: {}", e)))?;

        if let (Some(w), Some(h)) = (response.width, response.height) {
            if w > 0 && h > 0 {
                self.display_sizes.write().insert(rms_device_id.to_string(), (w as u32, h as u32));
            }
        }

        self.total_push_count.fetch_add(1, Ordering::SeqCst);
        Ok(response)
    }

    /// Get device display info (GET /devices/{id}/display)
    fn fetch_device_display(&self, rms_device_id: &str) -> Result<RmsDisplayResponse> {
        let config = self.config.read();
        let auth = self.auth_header()?;
        let url = format!("{}/api/v1/devices/{}/display", config.api_base_url(), rms_device_id);
        let response: RmsDisplayResponse = ureq::get(&url)
            .set("Authorization", &auth)
            .call()
            .map_err(|e| ExtensionError::ExecutionFailed(format!("Fetch display for {} failed: {}", rms_device_id, e)))?
            .into_json()
            .map_err(|e| ExtensionError::ExecutionFailed(format!("Parse display response: {}", e)))?;
        Ok(response)
    }
}

impl Default for UinkRmsBridge {
    fn default() -> Self { Self::new() }
}

// ============================================================================
// Extension Trait Implementation
// ============================================================================

#[async_trait]
impl Extension for UinkRmsBridge {
    fn metadata(&self) -> &ExtensionMetadata {
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {
            ExtensionMetadata::new("uink-rms-bridge", "Uink-RMS Bridge", "0.1.0")
                .with_description("Bridge extension for Uink-RMS e-paper displays with content-to-image conversion, device registration, and telemetry")
                .with_author("NeoMind Team")
                .with_config_parameters(vec![
                    ParameterDefinition {
                        name: "server_region".to_string(),
                        display_name: "Server".to_string(),
                        description: "Uink-RMS server region".to_string(),
                        param_type: MetricDataType::Enum {
                            options: vec!["China".to_string(), "Europe".to_string(), "Custom".to_string()],
                        },
                        required: true,
                        default_value: Some(ParamMetricValue::String("China".to_string())),
                        min: None, max: None,
                        options: vec!["China".to_string(), "Europe".to_string(), "Custom".to_string()],
                    },
                    ParameterDefinition {
                        name: "custom_server_url".to_string(),
                        display_name: "Custom Server URL".to_string(),
                        description: "Only needed when Server is set to Custom (e.g., https://your-server.com)".to_string(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: None,
                        min: None, max: None,
                        options: vec![],
                    },
                    ParameterDefinition {
                        name: "email".to_string(),
                        display_name: "Email".to_string(),
                        description: "RMS API login email address".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None, max: None,
                        options: vec![],
                    },
                    ParameterDefinition {
                        name: "password".to_string(),
                        display_name: "Password".to_string(),
                        description: "RMS API login password".to_string(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None, max: None,
                        options: vec![],
                    },
                    ParameterDefinition {
                        name: "sync_interval_secs".to_string(),
                        display_name: "Sync Interval (seconds)".to_string(),
                        description: "Device sync interval in seconds".to_string(),
                        param_type: MetricDataType::Integer,
                        required: false,
                        default_value: Some(ParamMetricValue::Integer(300)),
                        min: Some(30.0), max: Some(3600.0),
                        options: vec![],
                    },
                    ParameterDefinition {
                        name: "poll_interval_secs".to_string(),
                        display_name: "Poll Interval (seconds)".to_string(),
                        description: "Telemetry poll interval in seconds".to_string(),
                        param_type: MetricDataType::Integer,
                        required: false,
                        default_value: Some(ParamMetricValue::Integer(60)),
                        min: Some(10.0), max: Some(600.0),
                        options: vec![],
                    },
                ])
        })
    }

    fn metrics(&self) -> Vec<MetricDescriptor> {
        vec![
            MetricDescriptor { name: "sync_count".into(), display_name: "Sync Count".into(), data_type: MetricDataType::Integer, unit: "count".into(), min: Some(0.0), max: None, required: false },
            MetricDescriptor { name: "push_count".into(), display_name: "Push Count".into(), data_type: MetricDataType::Integer, unit: "count".into(), min: Some(0.0), max: None, required: false },
            MetricDescriptor { name: "device_count".into(), display_name: "Device Count".into(), data_type: MetricDataType::Integer, unit: "count".into(), min: Some(0.0), max: None, required: false },
            MetricDescriptor { name: "error_count".into(), display_name: "Error Count".into(), data_type: MetricDataType::Integer, unit: "count".into(), min: Some(0.0), max: None, required: false },
        ]
    }

    fn commands(&self) -> Vec<ExtensionCommand> {
        vec![
            ExtensionCommand {
                name: "sync_devices".into(),
                display_name: "Sync Devices".into(),
                description: "Sync Uink devices from RMS to NeoMind (registers template + devices)".into(),
                payload_template: String::new(),
                parameters: vec![],
                fixed_values: Default::default(),
                samples: vec![json!({})],
                parameter_groups: vec![],
            },
            ExtensionCommand {
                name: "list_devices".into(),
                display_name: "List Devices".into(),
                description: "List all synced Uink e-paper devices with their IDs, names, model, and online status. Use device_id from the result as target for push_content/push_image commands.".into(),
                payload_template: String::new(),
                parameters: vec![],
                fixed_values: Default::default(),
                samples: vec![json!({})],
                parameter_groups: vec![],
            },
            ExtensionCommand {
                name: "push_content".into(),
                display_name: "Push Content".into(),
                description: "Push text, markdown, or image content to an e-paper display. Text/MD is auto-converted to image.".into(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device_id".into(),
                        display_name: "Device ID".into(),
                        description: "Target device ID (e.g., uink-dev_abc123)".into(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None, max: None,
                        options: vec![],
                    },
                    ParameterDefinition {
                        name: "content_type".into(),
                        display_name: "Content Type".into(),
                        description: "Type of content to push".into(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None, max: None,
                        options: vec!["text".into(), "markdown".into(), "image".into()],
                    },
                    ParameterDefinition {
                        name: "content".into(),
                        display_name: "Content".into(),
                        description: "Text/MD content, or base64-encoded image data (when content_type is image)".into(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None, max: None,
                        options: vec![],
                    },
                    ParameterDefinition {
                        name: "dither_algorithm".into(),
                        display_name: "Dither Algorithm".into(),
                        description: "Dithering algorithm".into(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: None,
                        min: None, max: None,
                        options: vec!["ordered".into(), "floyd-steinberg".into(), "atkinson".into(), "burkes".into(), "sierra".into(), "stucki".into(), "jarvis-judice-ninke".into(), "threshold".into()],
                    },
                    ParameterDefinition {
                        name: "resize_mode".into(),
                        display_name: "Resize Mode".into(),
                        description: "Resize strategy: fit (default), cover, fill".into(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: None,
                        min: None, max: None,
                        options: vec!["fit".into(), "cover".into(), "fill".into()],
                    },
                    ParameterDefinition {
                        name: "padding_color".into(),
                        display_name: "Padding Color".into(),
                        description: "Hex background color for fit mode padding (e.g. FFFFFF)".into(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: None,
                        min: None, max: None,
                        options: vec![],
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![
                    json!({"device_id": "uink-dev_abc123", "content_type": "text", "content": "Hello World"}),
                    json!({"device_id": "uink-dev_abc123", "content_type": "markdown", "content": "# Title\n\n- item 1\n- item 2"}),
                ],
                parameter_groups: vec![],
            },
            ExtensionCommand {
                name: "push_image".into(),
                display_name: "Push Image".into(),
                description: "Push an image to an e-paper display. Pass image_url (recommended) or image_base64.".into(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device_id".into(),
                        display_name: "Device ID".into(),
                        description: "Target device ID".into(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None, max: None,
                        options: vec![],
                    },
                    ParameterDefinition {
                        name: "image_base64".into(),
                        display_name: "Image Data".into(),
                        description: "Base64-encoded image data, OR pass image_url instead".into(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: None,
                        min: None, max: None,
                        options: vec![],
                    },
                    ParameterDefinition {
                        name: "image_url".into(),
                        display_name: "Image URL".into(),
                        description: "Image URL to download and push (alternative to image_base64)".into(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: None,
                        min: None, max: None,
                        options: vec![],
                    },
                    ParameterDefinition {
                        name: "dither_algorithm".into(),
                        display_name: "Dither Algorithm".into(),
                        description: "Dithering algorithm".into(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: None,
                        min: None, max: None,
                        options: vec!["ordered".into(), "floyd-steinberg".into(), "atkinson".into(), "burkes".into(), "sierra".into(), "stucki".into(), "jarvis-judice-ninke".into(), "threshold".into()],
                    },
                    ParameterDefinition {
                        name: "resize_mode".into(),
                        display_name: "Resize Mode".into(),
                        description: "Resize strategy: fit (default), cover, fill".into(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: None,
                        min: None, max: None,
                        options: vec!["fit".into(), "cover".into(), "fill".into()],
                    },
                    ParameterDefinition {
                        name: "padding_color".into(),
                        display_name: "Padding Color".into(),
                        description: "Hex background color for fit mode padding (e.g. FFFFFF)".into(),
                        param_type: MetricDataType::String,
                        required: false,
                        default_value: None,
                        min: None, max: None,
                        options: vec![],
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![
                    json!({"device_id": "uink-dev_abc123", "image_url": "https://example.com/image.png"}),
                    json!({"device_id": "uink-dev_abc123", "image_base64": "<base64-encoded-image-data>"}),
                ],
                parameter_groups: vec![],
            },
            ExtensionCommand {
                name: "get_display_size".into(),
                display_name: "Get Display Size".into(),
                description: "Get the display resolution for a device".into(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device_id".into(),
                        display_name: "Device ID".into(),
                        description: "Target device ID".into(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None, max: None,
                        options: vec![],
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![json!({"device_id": "uink-dev_abc123"})],
                parameter_groups: vec![],
            },
            ExtensionCommand {
                name: "refresh_auth".into(),
                display_name: "Refresh Auth".into(),
                description: "Force refresh the JWT authentication token".into(),
                payload_template: String::new(),
                parameters: vec![],
                fixed_values: Default::default(),
                samples: vec![json!({})],
                parameter_groups: vec![],
            },
            ExtensionCommand {
                name: "get_display".into(),
                display_name: "Get Display".into(),
                description: "Get the current display content and pending content for a device".into(),
                payload_template: String::new(),
                parameters: vec![
                    ParameterDefinition {
                        name: "device_id".into(),
                        display_name: "Device ID".into(),
                        description: "Target device ID".into(),
                        param_type: MetricDataType::String,
                        required: true,
                        default_value: None,
                        min: None, max: None,
                        options: vec![],
                    },
                ],
                fixed_values: Default::default(),
                samples: vec![json!({"device_id": "uink-dev_abc123"})],
                parameter_groups: vec![],
            },
        ]
    }

    async fn execute_command(&self, command: &str, args: &serde_json::Value) -> Result<serde_json::Value> {
        match command {
            "configure" => {
                // Handle configure dispatched via execute_command (used during reload)
                let mut cfg = self.config.write();
                if let Some(v) = args.get("server_region").and_then(|v| v.as_str()) { cfg.server_region = v.to_string(); }
                if let Some(v) = args.get("custom_server_url").and_then(|v| v.as_str()) { cfg.custom_server_url = v.trim_end_matches('/').to_string(); }
                if let Some(v) = args.get("email").and_then(|v| v.as_str()) { cfg.email = v.to_string(); }
                if let Some(v) = args.get("password").and_then(|v| v.as_str()) { cfg.password = v.to_string(); }
                if let Some(v) = args.get("sync_interval_secs").and_then(|v| v.as_u64()) { cfg.sync_interval_secs = v; }
                if let Some(v) = args.get("poll_interval_secs").and_then(|v| v.as_u64()) { cfg.poll_interval_secs = v; }
                drop(cfg);
                *self.access_token.write() = None;
                *self.refresh_token.write() = None;
                self.token_expiry.store(0, Ordering::SeqCst);
                self.template_registered.store(0, Ordering::SeqCst);
                self.last_sync_ts.store(0, Ordering::SeqCst);
                eprintln!("[uink-rms-bridge] Configuration applied via execute_command");
                Ok(json!({"success": true}))
            }
            "sync_devices" => self.cmd_sync_devices(args).await,
            "list_devices" => self.cmd_list_devices(),
            "push_content" => self.cmd_push_content(args).await,
            "push_image" => self.cmd_push_image(args).await,
            "refresh_status" => self.cmd_refresh_status(args),
            "get_display_size" => self.cmd_get_display_size(args),
            "get_display" => self.cmd_get_display(args),
            "refresh_auth" => self.cmd_refresh_auth().await,
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }
    }

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {
        let now_ts = Utc::now().timestamp();
        let config = self.config.read();
        let configured = !config.api_base_url().is_empty() && !config.email.is_empty();
        let sync_interval = config.sync_interval_secs as i64;
        let poll_interval = config.poll_interval_secs as i64;
        drop(config);

        // Auto-sync: register template + sync devices periodically
        if configured {
            let should_sync = self.template_registered.load(Ordering::SeqCst) == 0
                || (now_ts - self.last_sync_ts.load(Ordering::SeqCst)) >= sync_interval;

            if should_sync {
                if let Err(e) = self.auto_sync() {
                    eprintln!("[uink-rms-bridge] Auto-sync failed: {}", e);
                    *self.last_error.write() = Some(format!("Auto-sync: {}", e));
                    self.total_error_count.fetch_add(1, Ordering::SeqCst);
                } else {
                    *self.last_error.write() = None;
                }
            }
        }

        // Poll telemetry if devices are registered
        if configured && (now_ts - self.last_poll_ts.load(Ordering::SeqCst)) >= poll_interval {
            if let Err(e) = self.poll_all_telemetry() {
                *self.last_error.write() = Some(format!("{}", e));
            } else if self.last_error.read().as_ref().map_or(true, |e| e.starts_with("Auto-sync")) {
                *self.last_error.write() = None;
            }
        }

        let now = Utc::now().timestamp_millis();
        let result = vec![
            ExtensionMetricValue { name: "sync_count".into(), value: ParamMetricValue::Integer(self.total_sync_count.load(Ordering::SeqCst)), timestamp: now },
            ExtensionMetricValue { name: "push_count".into(), value: ParamMetricValue::Integer(self.total_push_count.load(Ordering::SeqCst)), timestamp: now },
            ExtensionMetricValue { name: "device_count".into(), value: ParamMetricValue::Integer(self.device_ids.read().len() as i64), timestamp: now },
            ExtensionMetricValue { name: "error_count".into(), value: ParamMetricValue::Integer(self.total_error_count.load(Ordering::SeqCst)), timestamp: now },
        ];

        // Per-device telemetry is now written via device_metrics_write capability
        // in poll_all_telemetry(), so we don't emit them as extension metrics anymore.
        Ok(result)
    }

    async fn configure(&mut self, config: &serde_json::Value) -> Result<()> {
        let mut cfg = self.config.write();
        if let Some(v) = config.get("server_region").and_then(|v| v.as_str()) { cfg.server_region = v.to_string(); }
        if let Some(v) = config.get("custom_server_url").and_then(|v| v.as_str()) { cfg.custom_server_url = v.trim_end_matches('/').to_string(); }
        if let Some(v) = config.get("email").and_then(|v| v.as_str()) { cfg.email = v.to_string(); }
        if let Some(v) = config.get("password").and_then(|v| v.as_str()) { cfg.password = v.to_string(); }
        if let Some(v) = config.get("sync_interval_secs").and_then(|v| v.as_u64()) { cfg.sync_interval_secs = v; }
        if let Some(v) = config.get("poll_interval_secs").and_then(|v| v.as_u64()) { cfg.poll_interval_secs = v; }
        drop(cfg);
        *self.access_token.write() = None;
        *self.refresh_token.write() = None;
        self.token_expiry.store(0, Ordering::SeqCst);
        // Reset sync state so auto-sync runs immediately with new config
        self.template_registered.store(0, Ordering::SeqCst);
        self.last_sync_ts.store(0, Ordering::SeqCst);
        eprintln!("[uink-rms-bridge] Configuration updated");
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any { self }
}

// ============================================================================
// Command Implementations
// ============================================================================

impl UinkRmsBridge {
    /// Auto-sync: register template (once) + fetch & register devices from RMS.
    /// Called from produce_metrics (synchronous context).
    fn auto_sync(&self) -> Result<()> {
        let ctx = CapabilityContext::default();

        // Register template once
        if self.template_registered.load(Ordering::SeqCst) == 0 {
            let template_json = json!({
                "device_type": "uink_epaper",
                "name": "Uink E-Paper Display",
                "description": "Uink electronic paper display device",
                "categories": ["display", "e-paper"],
                "metrics": [
                    { "name": "battery", "display_name": "Battery Level", "data_type": "Integer", "unit": "%", "min": 0, "max": 100 },
                    { "name": "temperature", "display_name": "Temperature", "data_type": "Float", "unit": "°C" },
                    { "name": "signal_strength", "display_name": "Signal Strength", "data_type": "Integer", "unit": "dBm" },
                    { "name": "refresh_count", "display_name": "Refresh Count", "data_type": "Integer", "unit": "count" },
                    { "name": "online_status", "display_name": "Online Status", "data_type": "String" },
                    { "name": "last_sync", "display_name": "Last Sync", "data_type": "String" },
                    { "name": "sn", "display_name": "Serial Number", "data_type": "String" },
                    { "name": "model", "display_name": "Device Model", "data_type": "String" },
                    { "name": "activation_status", "display_name": "Activation Status", "data_type": "String" },
                    { "name": "alarm_status", "display_name": "Alarm Status", "data_type": "String" },
                    { "name": "firmware_version", "display_name": "Firmware Version", "data_type": "String" },
                    { "name": "hardware_version", "display_name": "Hardware Version", "data_type": "String" },
                    { "name": "preview_url", "display_name": "Screen Preview", "data_type": "String" },
                    { "name": "preview_thumbnail_url", "display_name": "Screen Preview Thumbnail", "data_type": "String" }
                ],
                "commands": [
                    { "name": "push_content", "display_name": "Push Content", "description": "Push text/markdown content (auto-converted to image)",
                      "parameters": [
                        { "name": "content_type", "display_name": "Content Type", "data_type": "String", "required": true,
                          "description": "text or markdown" },
                        { "name": "content", "display_name": "Content", "data_type": "String", "required": true }
                      ]
                    },
                    { "name": "push_image", "display_name": "Push Image", "description": "Push an image to the e-paper display",
                      "parameters": [
                        { "name": "image_url", "display_name": "Image URL", "data_type": "String", "required": true },
                        { "name": "dither_algorithm", "display_name": "Dither Algorithm", "data_type": "String", "required": false },
                        { "name": "resize_mode", "display_name": "Resize Mode", "data_type": "String", "required": false },
                        { "name": "padding_color", "display_name": "Padding Color", "data_type": "String", "required": false }
                      ]
                    },
                    { "name": "refresh_status", "display_name": "Refresh Status", "description": "Trigger a status refresh" }
                ]
            });
            let result = ctx.invoke_capability("device_template_register", &template_json);
            if result.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
                eprintln!("[uink-rms-bridge] Template registered");
                self.template_registered.store(1, Ordering::SeqCst);
            } else {
                let err = result.get("error").and_then(|v| v.as_str()).unwrap_or("unknown");
                eprintln!("[uink-rms-bridge] Template registration failed: {}", err);
                return Err(ExtensionError::ExecutionFailed(format!("Template registration failed: {}", err)));
            }
        }

        // Fetch devices from RMS
        let devices = self.fetch_rms_devices()?;
        let mut registered = 0;
        let mut skipped = 0;

        for device in &devices {
            let neo_device_id = format!("uink-{}", device.device_id);
            let device_json = json!({
                "device_id": neo_device_id,
                "name": device.name,
                "device_type": "uink_epaper",
            });
            let result = ctx.invoke_capability("device_register", &device_json);
            if result.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
                registered += 1;
            } else {
                let err = result.get("error").and_then(|v| v.as_str()).unwrap_or("unknown");
                eprintln!("[uink-rms-bridge] Device {} register skipped: {}", neo_device_id, err);
                skipped += 1;
            }

            // Update ID mappings
            self.neo_to_rms_id.write().insert(neo_device_id.clone(), device.device_id.clone());
            self.rms_device_names.write().insert(device.device_id.clone(), device.name.clone());

            // Cache display size from model name
            if let Some(model) = &device.model {
                if let Some(size) = model_to_resolution(model) {
                    self.display_sizes.write().insert(device.device_id.clone(), size);
                }
            }

            // Update online status
            let online = device.online_status.as_deref() == Some("online");
            let mut metrics = self.device_metrics.write();
            let dm = metrics.entry(device.device_id.clone()).or_default();
            dm.online = online;
            dm.last_updated = Utc::now().timestamp_millis();
            drop(metrics);

            // Write online_status and last_sync to NeoMind device metrics
            let now_ms = Utc::now().timestamp_millis();
            if let Some(ref status) = device.online_status {
                let _ = ctx.invoke_capability("device_metrics_write", &json!({
                    "device_id": neo_device_id,
                    "metric": "online_status",
                    "value": status,
                    "timestamp": now_ms,
                }));
            }
            if let Some(ts) = device.last_sync_at {
                let ts_secs = ts / 1000;
                let formatted = chrono::DateTime::from_timestamp(ts_secs, 0)
                    .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                    .unwrap_or_else(|| ts.to_string());
                let _ = ctx.invoke_capability("device_metrics_write", &json!({
                    "device_id": neo_device_id,
                    "metric": "last_sync",
                    "value": formatted,
                    "timestamp": now_ms,
                }));
            }

            // Write additional device info metrics
            if let Some(ref v) = device.sn {
                let _ = ctx.invoke_capability("device_metrics_write", &json!({
                    "device_id": neo_device_id, "metric": "sn", "value": v, "timestamp": now_ms,
                }));
            }
            if let Some(ref v) = device.model {
                let _ = ctx.invoke_capability("device_metrics_write", &json!({
                    "device_id": neo_device_id, "metric": "model", "value": v, "timestamp": now_ms,
                }));
            }
            if let Some(ref v) = device.activation_status {
                let _ = ctx.invoke_capability("device_metrics_write", &json!({
                    "device_id": neo_device_id, "metric": "activation_status", "value": v, "timestamp": now_ms,
                }));
            }
            if let Some(ref v) = device.alarm_status {
                let _ = ctx.invoke_capability("device_metrics_write", &json!({
                    "device_id": neo_device_id, "metric": "alarm_status", "value": v, "timestamp": now_ms,
                }));
            }
            if let Some(ref v) = device.firmware_version {
                let _ = ctx.invoke_capability("device_metrics_write", &json!({
                    "device_id": neo_device_id, "metric": "firmware_version", "value": v, "timestamp": now_ms,
                }));
            }
            if let Some(ref v) = device.hardware_version {
                let _ = ctx.invoke_capability("device_metrics_write", &json!({
                    "device_id": neo_device_id, "metric": "hardware_version", "value": v, "timestamp": now_ms,
                }));
            }
        }

        // Update device ID list
        {
            let mut device_ids = self.device_ids.write();
            device_ids.clear();
            for device in &devices {
                device_ids.push(format!("uink-{}", device.device_id));
            }
        }

        self.total_sync_count.fetch_add(1, Ordering::SeqCst);
        self.last_sync_ts.store(Utc::now().timestamp(), Ordering::SeqCst);

        eprintln!("[uink-rms-bridge] Auto-sync: {} registered, {} skipped, {} total",
            registered, skipped, devices.len());
        Ok(())
    }

    async fn cmd_sync_devices(&self, _args: &serde_json::Value) -> Result<serde_json::Value> {
        let ctx = CapabilityContext::default();

        // Register device type template
        let template_json = json!({
            "device_type": "uink_epaper",
            "name": "Uink E-Paper Display",
            "description": "Uink electronic paper display device",
            "categories": ["display", "e-paper"],
            "metrics": [
                { "name": "battery", "display_name": "Battery Level", "data_type": "Integer", "unit": "%", "min": 0, "max": 100 },
                { "name": "temperature", "display_name": "Temperature", "data_type": "Float", "unit": "°C" },
                { "name": "signal_strength", "display_name": "Signal Strength", "data_type": "Integer", "unit": "dBm" },
                { "name": "refresh_count", "display_name": "Refresh Count", "data_type": "Integer", "unit": "count" },
                { "name": "online_status", "display_name": "Online Status", "data_type": "String" },
                { "name": "last_sync", "display_name": "Last Sync", "data_type": "String" },
                { "name": "sn", "display_name": "Serial Number", "data_type": "String" },
                { "name": "model", "display_name": "Device Model", "data_type": "String" },
                { "name": "activation_status", "display_name": "Activation Status", "data_type": "String" },
                { "name": "alarm_status", "display_name": "Alarm Status", "data_type": "String" },
                { "name": "firmware_version", "display_name": "Firmware Version", "data_type": "String" },
                { "name": "hardware_version", "display_name": "Hardware Version", "data_type": "String" },
                { "name": "preview_url", "display_name": "Screen Preview", "data_type": "String" },
                { "name": "preview_thumbnail_url", "display_name": "Screen Preview Thumbnail", "data_type": "String" }
            ],
            "commands": [
                { "name": "push_image", "display_name": "Push Image", "description": "Push an image to the e-paper display",
                  "parameters": [
                    { "name": "image_url", "display_name": "Image URL", "data_type": "String", "required": true },
                    { "name": "dither_algorithm", "display_name": "Dither Algorithm", "data_type": "String", "required": false },
                    { "name": "resize_mode", "display_name": "Resize Mode", "data_type": "String", "required": false },
                    { "name": "padding_color", "display_name": "Padding Color", "data_type": "String", "required": false }
                  ]
                },
                { "name": "refresh_status", "display_name": "Refresh Status", "description": "Trigger a status refresh" }
            ]
        });

        let template_result = ctx.invoke_capability("device_template_register", &template_json);
        if template_result.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
            eprintln!("[uink-rms-bridge] Template registered");
            self.template_registered.store(1, Ordering::SeqCst);
        } else {
            let err = template_result.get("error").and_then(|v| v.as_str()).unwrap_or("unknown");
            eprintln!("[uink-rms-bridge] Template registration failed: {}", err);
        }

        // Fetch devices from RMS
        let devices = self.fetch_rms_devices()?;
        let mut registered = 0;
        let mut skipped = 0;

        for device in &devices {
            let neo_device_id = format!("uink-{}", device.device_id);
            let device_json = json!({
                "device_id": neo_device_id,
                "name": device.name,
                "device_type": "uink_epaper",
            });
            let result = ctx.invoke_capability("device_register", &device_json);
            if result.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
                registered += 1;
            } else {
                let err = result.get("error").and_then(|v| v.as_str()).unwrap_or("unknown");
                eprintln!("[uink-rms-bridge] Device {} register skipped: {}", neo_device_id, err);
                skipped += 1;
            }

            // Update ID mappings
            self.neo_to_rms_id.write().insert(neo_device_id.clone(), device.device_id.clone());
            self.rms_device_names.write().insert(device.device_id.clone(), device.name.clone());

            // Cache display size from model name
            if let Some(model) = &device.model {
                if let Some(size) = model_to_resolution(model) {
                    self.display_sizes.write().insert(device.device_id.clone(), size);
                }
            }

            // Update online status
            let online = device.online_status.as_deref() == Some("online");
            let mut metrics = self.device_metrics.write();
            let dm = metrics.entry(device.device_id.clone()).or_default();
            dm.online = online;
            dm.last_updated = Utc::now().timestamp_millis();
            drop(metrics);

            // Write online_status and last_sync to NeoMind device metrics
            let now_ms = Utc::now().timestamp_millis();
            if let Some(ref status) = device.online_status {
                let _ = ctx.invoke_capability("device_metrics_write", &json!({
                    "device_id": neo_device_id,
                    "metric": "online_status",
                    "value": status,
                    "timestamp": now_ms,
                }));
            }
            if let Some(ts) = device.last_sync_at {
                let ts_secs = ts / 1000;
                let formatted = chrono::DateTime::from_timestamp(ts_secs, 0)
                    .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                    .unwrap_or_else(|| ts.to_string());
                let _ = ctx.invoke_capability("device_metrics_write", &json!({
                    "device_id": neo_device_id,
                    "metric": "last_sync",
                    "value": formatted,
                    "timestamp": now_ms,
                }));
            }

            // Write additional device info metrics
            if let Some(ref v) = device.sn {
                let _ = ctx.invoke_capability("device_metrics_write", &json!({
                    "device_id": neo_device_id, "metric": "sn", "value": v, "timestamp": now_ms,
                }));
            }
            if let Some(ref v) = device.model {
                let _ = ctx.invoke_capability("device_metrics_write", &json!({
                    "device_id": neo_device_id, "metric": "model", "value": v, "timestamp": now_ms,
                }));
            }
            if let Some(ref v) = device.activation_status {
                let _ = ctx.invoke_capability("device_metrics_write", &json!({
                    "device_id": neo_device_id, "metric": "activation_status", "value": v, "timestamp": now_ms,
                }));
            }
            if let Some(ref v) = device.alarm_status {
                let _ = ctx.invoke_capability("device_metrics_write", &json!({
                    "device_id": neo_device_id, "metric": "alarm_status", "value": v, "timestamp": now_ms,
                }));
            }
            if let Some(ref v) = device.firmware_version {
                let _ = ctx.invoke_capability("device_metrics_write", &json!({
                    "device_id": neo_device_id, "metric": "firmware_version", "value": v, "timestamp": now_ms,
                }));
            }
            if let Some(ref v) = device.hardware_version {
                let _ = ctx.invoke_capability("device_metrics_write", &json!({
                    "device_id": neo_device_id, "metric": "hardware_version", "value": v, "timestamp": now_ms,
                }));
            }
        }

        // Update device ID list
        {
            let mut device_ids = self.device_ids.write();
            device_ids.clear();
            for device in &devices {
                device_ids.push(format!("uink-{}", device.device_id));
            }
        }

        self.total_sync_count.fetch_add(1, Ordering::SeqCst);
        self.last_sync_ts.store(Utc::now().timestamp(), Ordering::SeqCst);

        Ok(json!({
            "success": true,
            "registered": registered,
            "skipped": skipped,
            "total_devices": devices.len(),
        }))
    }

    async fn cmd_push_content(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_id = args.get("device_id").and_then(|v| v.as_str())
            .ok_or_else(|| ExtensionError::InvalidArguments("Missing device_id".into()))?;
        let content_type = args.get("content_type").and_then(|v| v.as_str())
            .ok_or_else(|| ExtensionError::InvalidArguments("Missing content_type".into()))?;
        let content = args.get("content").and_then(|v| v.as_str())
            .ok_or_else(|| ExtensionError::InvalidArguments("Missing content".into()))?;

        // Resolve RMS device ID
        let rms_id = self.resolve_rms_id(device_id)?;

        // Convert content to image data based on type
        let image_data = match content_type {
            "image" => {
                // Decode base64 image
                use base64::Engine;
                base64::engine::general_purpose::STANDARD.decode(content)
                    .map_err(|e| ExtensionError::InvalidArguments(format!("Invalid base64: {}", e)))?
            }
            "text" => {
                let (w, h) = self.get_display_size(&rms_id);
                let font_data = self.get_font_data()?;
                render_text_to_image(content, w, h, &font_data)?
            }
            "markdown" | "md" => {
                let (w, h) = self.get_display_size(&rms_id);
                let font_data = self.get_font_data()?;
                render_markdown_to_image(content, w, h, &font_data)?
            }
            other => {
                return Err(ExtensionError::InvalidArguments(format!(
                    "Unsupported content_type: {}. Use: text, markdown, image", other
                )));
            }
        };

        if image_data.is_empty() {
            return Err(ExtensionError::ExecutionFailed("Generated image is empty".into()));
        }
        if image_data.len() > 10 * 1024 * 1024 {
            return Err(ExtensionError::ExecutionFailed(format!(
                "Image too large: {} bytes (max 10MB)", image_data.len()
            )));
        }

        let algorithm = args.get("dither_algorithm").and_then(|v| v.as_str());
        let resize_mode = args.get("resize_mode").and_then(|v| v.as_str());
        let padding_color = args.get("padding_color").and_then(|v| v.as_str());

        let result = self.push_image_to_device(&rms_id, &image_data, algorithm, resize_mode, padding_color)?;

        Ok(json!({
            "success": true,
            "device_id": device_id,
            "rms_device_id": rms_id,
            "content_type": content_type,
            "image_size_bytes": image_data.len(),
            "display_size": { "width": result.width, "height": result.height },
            "image_url": result.image_url,
        }))
    }

    async fn cmd_push_image(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_id = args.get("device_id").and_then(|v| v.as_str())
            .ok_or_else(|| ExtensionError::InvalidArguments("Missing device_id".into()))?;

        // Accept both image_base64 (direct data) and image_url (download first)
        let image_data = if let Some(b64) = args.get("image_base64").and_then(|v| v.as_str()) {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD.decode(b64)
                .map_err(|e| ExtensionError::InvalidArguments(format!("Invalid base64: {}", e)))?
        } else if let Some(url) = args.get("image_url").and_then(|v| v.as_str()) {
            self.download_image(url)?
        } else {
            return Err(ExtensionError::InvalidArguments(
                "Missing image_base64 or image_url".into()
            ));
        };

        if image_data.is_empty() {
            return Err(ExtensionError::InvalidArguments("Image data is empty".into()));
        }
        if image_data.len() > 10 * 1024 * 1024 {
            return Err(ExtensionError::ExecutionFailed(format!(
                "Image too large: {} bytes (max 10MB)", image_data.len()
            )));
        }

        let rms_id = self.resolve_rms_id(device_id)?;
        let algorithm = args.get("dither_algorithm").and_then(|v| v.as_str());
        let resize_mode = args.get("resize_mode").and_then(|v| v.as_str());
        let padding_color = args.get("padding_color").and_then(|v| v.as_str());

        let result = if algorithm.is_none() && resize_mode.is_none() && padding_color.is_none() {
            // No processing params → use raw endpoint
            self.push_raw_image_to_device(&rms_id, &image_data)?
        } else {
            self.push_image_to_device(&rms_id, &image_data, algorithm, resize_mode, padding_color)?
        };

        Ok(json!({
            "success": true,
            "device_id": device_id,
            "rms_device_id": rms_id,
            "image_url": result.image_url,
            "width": result.width,
            "height": result.height,
        }))
    }

    fn cmd_get_display_size(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_id = args.get("device_id").and_then(|v| v.as_str())
            .ok_or_else(|| ExtensionError::InvalidArguments("Missing device_id".into()))?;
        let rms_id = self.resolve_rms_id(device_id)?;
        let (w, h) = self.get_display_size(&rms_id);
        Ok(json!({
            "success": true,
            "device_id": device_id,
            "rms_device_id": rms_id,
            "width": w,
            "height": h,
        }))
    }

    fn cmd_get_display(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_id = args.get("device_id").and_then(|v| v.as_str())
            .ok_or_else(|| ExtensionError::InvalidArguments("Missing device_id".into()))?;
        let rms_id = self.resolve_rms_id(device_id)?;
        let display = self.fetch_device_display(&rms_id)?;
        Ok(json!({
            "success": true,
            "device_id": device_id,
            "rms_device_id": rms_id,
            "slots": display.data.iter().map(|slot| json!({
                "image_id": slot.image_id,
                "preview_url": slot.preview_url,
                "preview_thumbnail_url": slot.preview_thumbnail_url,
                "is_pending": slot.is_pending,
                "pending_preview_url": slot.pending_preview_url,
                "pending_preview_thumbnail_url": slot.pending_preview_thumbnail_url,
                "refresh_count": slot.refresh_count,
            })).collect::<Vec<_>>(),
        }))
    }

    async fn cmd_refresh_auth(&self) -> Result<serde_json::Value> {
        *self.access_token.write() = None;
        *self.refresh_token.write() = None;
        self.token_expiry.store(0, Ordering::SeqCst);
        self.login()?;
        Ok(json!({ "success": true, "message": "Authentication refreshed" }))
    }

    fn cmd_list_devices(&self) -> Result<serde_json::Value> {
        let ids = self.device_ids.read();
        let names = self.rms_device_names.read();
        let metrics = self.device_metrics.read();

        let devices: Vec<serde_json::Value> = ids.iter().map(|neo_id| {
            let rms_id = neo_id.strip_prefix("uink-").unwrap_or(neo_id);
            let name = names.get(rms_id).cloned().unwrap_or_default();
            let online = metrics.get(rms_id).map(|m| m.online).unwrap_or(false);
            json!({
                "device_id": neo_id,
                "name": name,
                "online": online,
            })
        }).collect();

        Ok(json!({
            "success": true,
            "count": devices.len(),
            "devices": devices,
        }))
    }

    /// Refresh device status by re-fetching telemetry from RMS
    fn cmd_refresh_status(&self, args: &serde_json::Value) -> Result<serde_json::Value> {
        let device_id = args.get("device_id").and_then(|v| v.as_str())
            .ok_or_else(|| ExtensionError::InvalidArguments("Missing device_id".into()))?;
        let rms_id = self.resolve_rms_id(device_id)?;
        let telemetry = self.fetch_device_telemetry(&rms_id)?;

        // Update cached metrics
        let mut metrics = self.device_metrics.write();
        let dm = metrics.entry(rms_id.clone()).or_default();
        dm.last_updated = Utc::now().timestamp_millis();
        dm.battery = telemetry.battery.unwrap_or(dm.battery);
        dm.signal_strength = telemetry.signal_strength.unwrap_or(dm.signal_strength);
        dm.temperature = telemetry.temperature.unwrap_or(dm.temperature);
        dm.refresh_count = telemetry.refresh_count.unwrap_or(dm.refresh_count);
        drop(metrics);

        Ok(json!({
            "success": true,
            "device_id": device_id,
            "battery": telemetry.battery,
            "signal_strength": telemetry.signal_strength,
            "temperature": telemetry.temperature,
        }))
    }

    /// Resolve NeoMind device_id to RMS device_id
    fn resolve_rms_id(&self, device_id: &str) -> Result<String> {
        let neo_map = self.neo_to_rms_id.read();
        neo_map.get(device_id).cloned()
            .or_else(|| device_id.strip_prefix("uink-").map(|s| s.to_string()))
            .ok_or_else(|| ExtensionError::InvalidArguments(
                format!("Device {} not found. Run sync_devices first.", device_id)
            ))
    }
}

// ============================================================================
// FFI Export
// ============================================================================

neomind_extension_sdk::neomind_export!(UinkRmsBridge);

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_metadata() {
        let ext = UinkRmsBridge::new();
        assert_eq!(ext.metadata().id, "uink-rms-bridge");
    }

    #[test]
    fn test_commands_count() {
        let ext = UinkRmsBridge::new();
        let commands = ext.commands();
        assert_eq!(commands.len(), 7);
        let names: Vec<&str> = commands.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"sync_devices"));
        assert!(names.contains(&"list_devices"));
        assert!(names.contains(&"push_content"));
        assert!(names.contains(&"push_image"));
        assert!(names.contains(&"get_display_size"));
        assert!(names.contains(&"refresh_auth"));
        assert!(names.contains(&"get_display"));
    }

    #[test]
    fn test_config_parameters() {
        let ext = UinkRmsBridge::new();
        let params = ext.metadata().config_parameters.as_ref().unwrap();
        assert_eq!(params.len(), 6);
        assert_eq!(params[0].name, "server_region");
        assert_eq!(params[1].name, "custom_server_url");
        assert_eq!(params[2].name, "email");
        assert_eq!(params[3].name, "password");
        assert!(params[0].options.contains(&"China".to_string()));
        assert!(params[0].options.contains(&"Europe".to_string()));
    }

    #[test]
    fn test_api_base_url() {
        let mut cfg = UinkConfig::default();
        cfg.server_region = "China".into();
        assert_eq!(cfg.api_base_url(), "https://cn.rms.uink.com");
        cfg.server_region = "Europe".into();
        assert_eq!(cfg.api_base_url(), "https://eu.rms.uink.com");
        cfg.server_region = "Custom".into();
        cfg.custom_server_url = "https://my.server/".into();
        assert_eq!(cfg.api_base_url(), "https://my.server");
    }

    #[test]
    fn test_model_to_resolution() {
        assert_eq!(model_to_resolution("UINK-7.5"), Some((800, 480)));
        assert_eq!(model_to_resolution("UINK-2.9"), Some((296, 128)));
        assert_eq!(model_to_resolution("UINK-4.2"), Some((400, 300)));
        assert_eq!(model_to_resolution("UINK-10.2"), Some((960, 640)));
        assert_eq!(model_to_resolution("UNKNOWN"), None);
    }

    #[test]
    fn test_parse_markdown() {
        let md = "# Title\n\nParagraph\n\n- item 1\n- item 2";
        let blocks = parse_markdown(md);
        assert!(blocks.len() >= 3, "Expected at least 3 blocks, got {}", blocks.len());

        // First block should be heading
        match &blocks[0] {
            TextBlock::Heading { level, text } => {
                assert_eq!(*level, 1);
                assert_eq!(text, "Title");
            }
            _ => panic!("Expected heading block"),
        }

        // Second block should be paragraph
        match &blocks[1] {
            TextBlock::Paragraph { parts } => {
                assert!(!parts.is_empty());
            }
            _ => panic!("Expected paragraph block"),
        }
    }

    #[test]
    fn test_parse_markdown_bold() {
        let md = "Hello **world** and `code`";
        let blocks = parse_markdown(md);
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            TextBlock::Paragraph { parts } => {
                assert!(parts.iter().any(|p| matches!(p, TextPart::Bold(_))), "Should have bold part");
                assert!(parts.iter().any(|p| matches!(p, TextPart::Code(_))), "Should have code part");
            }
            _ => panic!("Expected paragraph block"),
        }
    }

    #[test]
    fn test_parse_markdown_headings() {
        let md = "# H1\n## H2\n### H3";
        let blocks = parse_markdown(md);
        assert_eq!(blocks.len(), 3);
        match &blocks[0] {
            TextBlock::Heading { level, text } => { assert_eq!(*level, 1); assert_eq!(text, "H1"); }
            _ => panic!("Expected heading"),
        }
        match &blocks[1] {
            TextBlock::Heading { level, text } => { assert_eq!(*level, 2); assert_eq!(text, "H2"); }
            _ => panic!("Expected heading"),
        }
        match &blocks[2] {
            TextBlock::Heading { level, text } => { assert_eq!(*level, 3); assert_eq!(text, "H3"); }
            _ => panic!("Expected heading"),
        }
    }

    #[test]
    fn test_api_types_deserialization() {
        let login: RmsLoginResponse = serde_json::from_value(json!({
            "access_token": "tok", "refresh_token": "ref", "expires_in": 3600
        })).unwrap();
        assert_eq!(login.access_token, "tok");

        let devices: RmsDeviceListResponse = serde_json::from_value(json!({
            "data": [{"device_id": "d1", "name": "Screen", "model": "UINK-7.5", "online_status": "online"}],
            "pagination": {"page": 1, "limit": 20, "total": 1}
        })).unwrap();
        assert_eq!(devices.data[0].device_id, "d1");
    }

    #[test]
    fn test_default_config() {
        let cfg = UinkConfig::default();
        assert_eq!(cfg.server_region, "China");
        assert_eq!(cfg.sync_interval_secs, 300);
    }

    #[test]
    fn test_produce_metrics_without_config() {
        let ext = UinkRmsBridge::new();
        let metrics = ext.produce_metrics().unwrap();
        assert!(metrics.len() >= 4);
    }
}
