//! Face alignment module using affine transform.
//!
//! Normalizes detected face images to ArcFace's expected 112x112 aligned format
//! using 5-point facial landmarks and least-squares affine estimation.

use image::{DynamicImage, GenericImage, GenericImageView, Rgba};
use image::imageops;
use ndarray::{Array1, Array2};

use crate::FaceBox;

// ============================================================================
// Constants
// ============================================================================

/// ArcFace standard 112x112 reference landmarks (left_eye, right_eye, nose, left_mouth, right_mouth)
pub const REFERENCE_LANDMARKS: [(f32, f32); 5] = [
    (38.2946, 51.6963),  // left eye
    (73.5318, 51.5014),  // right eye
    (56.0252, 71.7366),  // nose
    (41.5493, 92.3655),  // left mouth
    (70.7299, 92.2041),  // right mouth
];

/// Output size for aligned face images (ArcFace standard)
const ALIGNMENT_SIZE: u32 = 112;

// ============================================================================
// Affine Estimation
// ============================================================================

/// Estimate a 2x3 affine transformation matrix from 5 point correspondences.
///
/// Given source points (from detected landmarks) and destination points
/// (reference landmarks), computes the affine matrix that maps src -> dst
/// using least-squares estimation.
///
/// Returns `[[a, b, tx], [c, d, ty]]` such that:
///   dst_x = a * src_x + b * src_y + tx
///   dst_y = c * src_x + d * src_y + ty
pub fn estimate_affine_matrix(
    src: &[(f32, f32); 5],
    dst: &[(f32, f32); 5],
) -> [[f32; 3]; 2] {
    // Build the 10x6 system of equations.
    // For each point pair (src_x, src_y) -> (dst_x, dst_y):
    //   [src_x, src_y, 1, 0, 0, 0] => dst_x
    //   [0, 0, 0, src_x, src_y, 1] => dst_y
    let mut a_mat = Array2::<f32>::zeros((10, 6));
    let mut b_vec = Array1::<f32>::zeros(10);

    for i in 0..5 {
        let (sx, sy) = src[i];
        let (dx, dy) = dst[i];

        let row_x = i * 2;
        let row_y = i * 2 + 1;

        a_mat[[row_x, 0]] = sx;
        a_mat[[row_x, 1]] = sy;
        a_mat[[row_x, 2]] = 1.0;
        // columns 3,4,5 already 0

        // columns 0,1,2 already 0
        a_mat[[row_y, 3]] = sx;
        a_mat[[row_y, 4]] = sy;
        a_mat[[row_y, 5]] = 1.0;

        b_vec[row_x] = dx;
        b_vec[row_y] = dy;
    }

    // Solve using normal equations: x = (A^T A)^-1 A^T b
    let a_t = a_mat.t();
    let ata = a_t.dot(&a_mat);
    let atb = a_t.dot(&b_vec);

    // Solve 6x6 system via Gauss-Jordan elimination (no external linalg needed)
    let n = 6;
    let mut aug = Array2::<f32>::zeros((n, n + 1));
    for i in 0..n {
        for j in 0..n {
            aug[[i, j]] = ata[[i, j]];
        }
        aug[[i, n]] = atb[i];
    }

    // Forward elimination with partial pivoting
    for col in 0..n {
        // Find pivot
        let mut max_row = col;
        let mut max_val = aug[[col, col]].abs();
        for row in (col + 1)..n {
            let val = aug[[row, col]].abs();
            if val > max_val {
                max_val = val;
                max_row = row;
            }
        }

        // Swap rows
        if max_row != col {
            for j in col..=n {
                let tmp = aug[[col, j]];
                aug[[col, j]] = aug[[max_row, j]];
                aug[[max_row, j]] = tmp;
            }
        }

        // Eliminate below
        let pivot = aug[[col, col]];
        if pivot.abs() < 1e-10 {
            continue;
        }
        for row in (col + 1)..n {
            let factor = aug[[row, col]] / pivot;
            for j in col..=n {
                aug[[row, j]] -= factor * aug[[col, j]];
            }
        }
    }

    // Back substitution
    let mut x = Array1::<f32>::zeros(n);
    for i in (0..n).rev() {
        let mut sum = aug[[i, n]];
        for j in (i + 1)..n {
            sum -= aug[[i, j]] * x[j];
        }
        x[i] = sum / aug[[i, i]];
    }

    // x = [a, b, tx, c, d, ty]
    [
        [x[0], x[1], x[2]],
        [x[3], x[4], x[5]],
    ]
}

// ============================================================================
// Face Alignment
// ============================================================================

/// Align a detected face to ArcFace's standard 112x112 format.
///
/// If the `FaceBox` contains landmarks, computes an affine transform from the
/// detected landmarks to the reference landmarks and warps the image accordingly.
/// If no landmarks are available, falls back to a simple resize with a warning.
pub fn align_face(image: &DynamicImage, face_box: &FaceBox) -> DynamicImage {
    match &face_box.landmarks {
        Some(landmarks) if landmarks.len() == 5 => {
            let src_points: [(f32, f32); 5] = [
                (landmarks[0].x as f32, landmarks[0].y as f32),
                (landmarks[1].x as f32, landmarks[1].y as f32),
                (landmarks[2].x as f32, landmarks[2].y as f32),
                (landmarks[3].x as f32, landmarks[3].y as f32),
                (landmarks[4].x as f32, landmarks[4].y as f32),
            ];

            let m = estimate_affine_matrix(&src_points, &REFERENCE_LANDMARKS);
            warp_affine(image, &m, ALIGNMENT_SIZE, ALIGNMENT_SIZE)
        }
        _ => {
            tracing::warn!(
                "No landmarks provided for face alignment, falling back to simple resize"
            );
            DynamicImage::ImageRgba8(imageops::resize(
                image,
                ALIGNMENT_SIZE,
                ALIGNMENT_SIZE,
                imageops::FilterType::Triangle,
            ))
        }
    }
}

/// Apply an affine transform to warp an image to the specified output dimensions.
///
/// Uses inverse mapping: for each output pixel, computes the corresponding
/// source pixel using the inverse affine transform and samples with bilinear
/// interpolation.
fn warp_affine(
    image: &DynamicImage,
    m: &[[f32; 3]; 2],
    out_w: u32,
    out_h: u32,
) -> DynamicImage {
    let (in_w, in_h) = (image.width() as f32, image.height() as f32);

    // Compute inverse affine: 2x3 matrix inverse
    let a = m[0][0];
    let b = m[0][1];
    let c = m[1][0];
    let d = m[1][1];
    let tx = m[0][2];
    let ty = m[1][2];

    let det = a * d - b * c;
    if det.abs() < 1e-10 {
        // Degenerate transform, just resize
        return DynamicImage::ImageRgba8(imageops::resize(image, out_w, out_h, imageops::FilterType::Triangle));
    }

    let inv_det = 1.0 / det;
    // Inverse matrix: [[ d/det, -b/det, (-d*tx + b*ty)/det],
    //                    [-c/det,  a/det, ( c*tx - a*ty)/det]]
    let inv_a = d * inv_det;
    let inv_b = -b * inv_det;
    let inv_c = -c * inv_det;
    let inv_d = a * inv_det;
    let inv_tx = (-d * tx + b * ty) * inv_det;
    let inv_ty = (c * tx - a * ty) * inv_det;

    let mut output = DynamicImage::new_rgba8(out_w, out_h);

    for oy in 0..out_h {
        let oy_f = oy as f32;
        for ox in 0..out_w {
            let ox_f = ox as f32;

            // Map output -> source using inverse affine
            let sx = inv_a * ox_f + inv_b * oy_f + inv_tx;
            let sy = inv_c * ox_f + inv_d * oy_f + inv_ty;

            let pixel = if sx < 0.0 || sy < 0.0 || sx >= in_w || sy >= in_h {
                Rgba([0, 0, 0, 0])
            } else {
                bilinear_sample(image, sx, sy)
            };

            output.put_pixel(ox, oy, pixel);
        }
    }

    output
}

/// Bilinear interpolation sampling at fractional coordinates.
fn bilinear_sample(image: &DynamicImage, x: f32, y: f32) -> Rgba<u8> {
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let x1 = x0 + 1;
    let y1 = y0 + 1;

    let fx = x - x0 as f32;
    let fy = y - y0 as f32;

    let w = image.width() as i32;
    let h = image.height() as i32;

    let get_pixel = |px: i32, py: i32| -> Rgba<u8> {
        if px < 0 || py < 0 || px >= w || py >= h {
            Rgba([0, 0, 0, 0])
        } else {
            image.get_pixel(px as u32, py as u32)
        }
    };

    let p00 = get_pixel(x0, y0);
    let p10 = get_pixel(x1, y0);
    let p01 = get_pixel(x0, y1);
    let p11 = get_pixel(x1, y1);

    let interpolate = |v00: u8, v10: u8, v01: u8, v11: u8| -> u8 {
        let v = (v00 as f32) * (1.0 - fx) * (1.0 - fy)
            + (v10 as f32) * fx * (1.0 - fy)
            + (v01 as f32) * (1.0 - fx) * fy
            + (v11 as f32) * fx * fy;
        v.round() as u8
    };

    Rgba([
        interpolate(p00[0], p10[0], p01[0], p11[0]),
        interpolate(p00[1], p10[1], p01[1], p11[1]),
        interpolate(p00[2], p10[2], p01[2], p11[2]),
        interpolate(p00[3], p10[3], p01[3], p11[3]),
    ])
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FaceBox, Landmark};

    /// Verify estimate_affine_matrix produces a valid 2x3 matrix with finite values.
    #[test]
    fn test_estimate_affine_matrix_produces_valid_2x3() {
        let src: [(f32, f32); 5] = [
            (30.0, 45.0),
            (70.0, 44.0),
            (50.0, 65.0),
            (35.0, 85.0),
            (65.0, 84.0),
        ];
        let dst = REFERENCE_LANDMARKS;

        let m = estimate_affine_matrix(&src, &dst);

        // All values must be finite
        for row in &m {
            for &val in row {
                assert!(
                    val.is_finite(),
                    "Affine matrix contains non-finite value: {}",
                    val
                );
            }
        }

        // The matrix should represent a non-trivial transform (not all zeros)
        let all_zero = m.iter().all(|row| row.iter().all(|&v| v == 0.0));
        assert!(!all_zero, "Affine matrix should not be all zeros");
    }

    /// Verify identity transform: when src and dst points are identical,
    /// the affine matrix should approximate the identity.
    #[test]
    fn test_identity_transform() {
        let points: [(f32, f32); 5] = [
            (38.2946, 51.6963),
            (73.5318, 51.5014),
            (56.0252, 71.7366),
            (41.5493, 92.3655),
            (70.7299, 92.2041),
        ];

        let m = estimate_affine_matrix(&points, &points);

        // Identity matrix: [[1, 0, 0], [0, 1, 0]]
        let tolerance = 1e-2;
        assert!(
            (m[0][0] - 1.0).abs() < tolerance,
            "m[0][0] should be ~1.0, got {}",
            m[0][0]
        );
        assert!(
            m[0][1].abs() < tolerance,
            "m[0][1] should be ~0.0, got {}",
            m[0][1]
        );
        assert!(
            m[0][2].abs() < tolerance,
            "m[0][2] should be ~0.0, got {}",
            m[0][2]
        );
        assert!(
            m[1][0].abs() < tolerance,
            "m[1][0] should be ~0.0, got {}",
            m[1][0]
        );
        assert!(
            (m[1][1] - 1.0).abs() < tolerance,
            "m[1][1] should be ~1.0, got {}",
            m[1][1]
        );
        assert!(
            m[1][2].abs() < tolerance,
            "m[1][2] should be ~0.0, got {}",
            m[1][2]
        );
    }

    /// Verify align_face outputs a 112x112 image when landmarks are provided.
    #[test]
    fn test_align_face_with_landmarks_outputs_112x112() {
        // Create a 200x200 test image
        let img = DynamicImage::new_rgb8(200, 200);

        let face_box = FaceBox {
            x: 20.0,
            y: 20.0,
            width: 100.0,
            height: 120.0,
            confidence: 0.95,
            landmarks: Some(vec![
                Landmark { x: 50.0, y: 60.0 },   // left eye
                Landmark { x: 90.0, y: 58.0 },   // right eye
                Landmark { x: 70.0, y: 85.0 },   // nose
                Landmark { x: 55.0, y: 110.0 },  // left mouth
                Landmark { x: 85.0, y: 108.0 },  // right mouth
            ]),
        };

        let aligned = align_face(&img, &face_box);

        assert_eq!(
            aligned.width(),
            112,
            "Aligned face width should be 112"
        );
        assert_eq!(
            aligned.height(),
            112,
            "Aligned face height should be 112"
        );
    }

    /// Verify align_face degrades gracefully when landmarks is None,
    /// returning a resized version of the input.
    #[test]
    fn test_align_face_without_landmarks_falls_back_to_resize() {
        let img = DynamicImage::new_rgb8(300, 400);

        let face_box = FaceBox {
            x: 50.0,
            y: 50.0,
            width: 150.0,
            height: 180.0,
            confidence: 0.9,
            landmarks: None,
        };

        let aligned = align_face(&img, &face_box);

        // Should still produce 112x112 via simple resize
        assert_eq!(
            aligned.width(),
            112,
            "Fallback aligned face width should be 112"
        );
        assert_eq!(
            aligned.height(),
            112,
            "Fallback aligned face height should be 112"
        );
    }

    /// Verify align_face handles wrong number of landmarks gracefully.
    #[test]
    fn test_align_face_with_wrong_landmark_count_falls_back() {
        let img = DynamicImage::new_rgb8(200, 200);

        // Only 3 landmarks instead of 5
        let face_box = FaceBox {
            x: 20.0,
            y: 20.0,
            width: 100.0,
            height: 120.0,
            confidence: 0.95,
            landmarks: Some(vec![
                Landmark { x: 50.0, y: 60.0 },
                Landmark { x: 90.0, y: 58.0 },
                Landmark { x: 70.0, y: 85.0 },
            ]),
        };

        let aligned = align_face(&img, &face_box);

        assert_eq!(aligned.width(), 112);
        assert_eq!(aligned.height(), 112);
    }

    /// Verify affine transform correctly maps known points.
    #[test]
    fn test_affine_transforms_known_point() {
        // Simple scaling by 2x
        let src: [(f32, f32); 5] = [
            (10.0, 20.0),
            (30.0, 20.0),
            (20.0, 40.0),
            (12.0, 50.0),
            (28.0, 50.0),
        ];
        let dst: [(f32, f32); 5] = [
            (20.0, 40.0),
            (60.0, 40.0),
            (40.0, 80.0),
            (24.0, 100.0),
            (56.0, 100.0),
        ];

        let m = estimate_affine_matrix(&src, &dst);

        // Check that the transform maps the first source point close to the first dst point
        let result_x = m[0][0] * src[0].0 + m[0][1] * src[0].1 + m[0][2];
        let result_y = m[1][0] * src[0].0 + m[1][1] * src[0].1 + m[1][2];

        let tolerance = 1e-2;
        assert!(
            (result_x - dst[0].0).abs() < tolerance,
            "Transformed x should be {}, got {}",
            dst[0].0,
            result_x
        );
        assert!(
            (result_y - dst[0].1).abs() < tolerance,
            "Transformed y should be {}, got {}",
            dst[0].1,
            result_y
        );
    }
}
