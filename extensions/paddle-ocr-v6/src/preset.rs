//! PP-OCRv6 preset builders for usls::Config.
//!
//! Self-maintained alternatives to usls's `ppocr_det_v5_mobile()` /
//! `ppocr_rec_v5_mobile()`. PP-OCRv6 has different preprocessing
//! requirements from v5 — these builders encode the v6-specific
//! overrides discovered during spec analysis:
//!
//! - det: box_thresh (0.40 tiny / 0.45 others), unclip_ratio=1.4,
//!        BGR input (swap_rgb=true)
//! - rec: normalize=false (v6 trained on raw [0,255] pixels, unlike
//!        v5 which expected normalized [0,1]), BGR input, dynamic
//!        width opt=320..3200
//!
//! Failure to apply any one of these overrides silently produces
//! garbage recognition output (boxes around nothing, or rec producing
//! mojibake). The unit tests pin each override.

use crate::tier::Tier;
use usls::Config;

/// Det model filename for a given tier, e.g. "ppocr-v6-tiny-det.onnx".
pub fn det_filename(tier: Tier) -> String {
    format!("ppocr-v6-{}-det.onnx", tier.filename_segment())
}

/// Rec model filename for a given tier.
pub fn rec_filename(tier: Tier) -> String {
    format!("ppocr-v6-{}-rec.onnx", tier.filename_segment())
}

/// Dictionary filename for a given tier.
/// Tiny uses a separate (smaller, no Japanese) dictionary.
pub fn dict_filename(tier: Tier) -> &'static str {
    match tier {
        Tier::Tiny => "ppocrv6_tiny_dict.txt",
        _ => "ppocrv6_dict.txt",
    }
}

/// Build a `usls::Config` for PP-OCRv6 detection (DB).
///
/// Caller is responsible for adding `.with_device_all(...)` and `.commit()`.
pub fn ppocr_det_v6(tier: Tier, models_dir: &std::path::Path) -> Config {
    // PP-OCRv6 tiny det was trained with box_thresh=0.40; small/medium
    // use 0.45. Wrong box_thresh → either too many false-positive boxes
    // or missing real text.
    let box_thresh: f32 = match tier {
        Tier::Tiny => 0.40,
        _ => 0.45,
    };
    let det_path = models_dir.join(det_filename(tier));
    let det_path_str = det_path.to_string_lossy().to_string();

    Config::db()
        .with_model_file(&det_path_str)
        .with_class_confs(&[box_thresh])
        // v6 YAML uses unclip_ratio=1.4 (usls default is 1.5). Affects
        // polygon expansion when converting mask → box.
        .with_db_unclip_ratio(1.4)
        // v6 det trained on BGR; usls default pipeline produces RGB.
        .with_swap_rgb(true)
}

/// Build a `usls::Config` for PP-OCRv6 recognition (SVTR).
///
/// Caller is responsible for adding `.with_device_all(...)` and `.commit()`.
pub fn ppocr_rec_v6(tier: Tier, models_dir: &std::path::Path) -> Config {
    let rec_path = models_dir.join(rec_filename(tier));
    let dict_path = models_dir.join(dict_filename(tier));
    let rec_path_str = rec_path.to_string_lossy().to_string();
    let dict_path_str = dict_path.to_string_lossy().to_string();

    Config::svtr()
        .with_model_file(&rec_path_str)
        .with_vocab_txt(&dict_path_str)
        // Rec input shape: [batch, channel=3, height=320, width∈(320,960,3200)]
        // Width is dynamic to handle text crops of varying length.
        .with_model_ixx(0, 3, (320, 960, 3200).into())
        // CRITICAL: v6 rec was trained WITHOUT NormalizeImage in the
        // preprocessing pipeline — it expects raw [0,255] pixels.
        // usls::Config::svtr() defaults to normalize=true (which is
        // correct for v4/v5). Leaving normalize on produces values
        // in [0,1] which the v6 model has never seen → garbage output.
        .with_normalize(false)
        .with_swap_rgb(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filenames_det() {
        assert_eq!(det_filename(Tier::Tiny), "ppocr-v6-tiny-det.onnx");
        assert_eq!(det_filename(Tier::Small), "ppocr-v6-small-det.onnx");
        assert_eq!(det_filename(Tier::Medium), "ppocr-v6-medium-det.onnx");
    }

    #[test]
    fn test_filenames_rec() {
        assert_eq!(rec_filename(Tier::Tiny), "ppocr-v6-tiny-rec.onnx");
        assert_eq!(rec_filename(Tier::Medium), "ppocr-v6-medium-rec.onnx");
    }

    #[test]
    fn test_dict_filename() {
        assert_eq!(dict_filename(Tier::Tiny), "ppocrv6_tiny_dict.txt");
        assert_eq!(dict_filename(Tier::Small), "ppocrv6_dict.txt");
        assert_eq!(dict_filename(Tier::Medium), "ppocrv6_dict.txt");
    }

    #[test]
    fn test_det_preset_tiny_box_thresh() {
        // Tiny uses 0.40; others use 0.45. class_confs carries box_thresh.
        let cfg = ppocr_det_v6(Tier::Tiny, std::path::Path::new("/tmp"));
        assert_eq!(cfg.class_confs, vec![0.40]);
        assert_eq!(cfg.db_unclip_ratio, Some(1.4));
        assert_eq!(cfg.swap_rgb, Some(true));
    }

    #[test]
    fn test_det_preset_medium_box_thresh() {
        let cfg = ppocr_det_v6(Tier::Medium, std::path::Path::new("/tmp"));
        assert_eq!(cfg.class_confs, vec![0.45]);
        assert_eq!(cfg.swap_rgb, Some(true));
    }

    #[test]
    fn test_det_preset_model_file_path() {
        let cfg = ppocr_det_v6(Tier::Tiny, std::path::Path::new("/opt/models"));
        assert!(cfg.model.file.ends_with("ppocr-v6-tiny-det.onnx"));
        assert!(cfg.model.file.contains("/opt/models"));
    }

    #[test]
    fn test_rec_preset_constructs() {
        // Don't actually load the model — just verify Config builds
        // and carries the critical v6 overrides.
        let cfg = ppocr_rec_v6(Tier::Small, std::path::Path::new("/tmp"));
        // swap_rgb at Config level
        assert_eq!(cfg.swap_rgb, Some(true));
        // Processor-level normalize was flipped to false (svtr default is true)
        assert!(!cfg.processor.normalize, "rec processor.normalize must be false for v6");
        // vocab path was set
        assert_eq!(
            cfg.processor.vocab_txt.as_deref(),
            Some("/tmp/ppocrv6_dict.txt")
        );
    }

    #[test]
    fn test_rec_preset_model_file_path() {
        let cfg = ppocr_rec_v6(Tier::Medium, std::path::Path::new("/models"));
        assert!(cfg.model.file.ends_with("ppocr-v6-medium-rec.onnx"));
    }
}
