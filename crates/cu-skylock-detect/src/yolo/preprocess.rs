//! Image preprocessing for YOLOv8: an NV12 frame resized into the model's planar RGB input.

/// Target input size for the model (width and height)
pub const MODEL_INPUT_SIZE: usize = 640;

/// The model input size for a `width` x `height` frame: the longer side becomes
/// `MODEL_INPUT_SIZE`, and the shorter keeps the aspect ratio, rounded down to a multiple of 32.
/// A 640x360 frame goes in as 640x352.
pub fn model_input_dims(width: usize, height: usize) -> (usize, usize) {
    calculate_resize_dims(width, height, MODEL_INPUT_SIZE)
}

/// Writes an NV12 frame, resized to `model_w` x `model_h`, into `dst` as planar RGB
/// (`3 x model_h x model_w`, values from 0 to 1), the layout YOLOv8 takes. Output pixel
/// `(x, y)` samples the frame at `(x * width / model_w, y * height / model_h)`, as the pose
/// example's resize did: luma bilinearly, chroma from the nearest sample. It doesn't allocate.
///
/// # Panics
/// If `src` is shorter than an NV12 frame of `stride` x `height`, or `dst` is shorter than
/// `3 * model_w * model_h`.
pub fn nv12_to_planar_rgb(
    src: &[u8],
    (width, height, stride): (usize, usize, usize),
    dst: &mut [f32],
    (model_w, model_h): (usize, usize),
) {
    let (y_plane, uv_plane) = src.split_at(stride * height);
    let plane = model_w * model_h;
    let (r_out, rest) = dst.split_at_mut(plane);
    let (g_out, rest) = rest.split_at_mut(plane);
    let b_out = &mut rest[..plane];
    let x_ratio = width as f32 / model_w as f32;
    let y_ratio = height as f32 / model_h as f32;
    for y in 0..model_h {
        let sy = y as f32 * y_ratio;
        let y0 = sy as usize;
        let y1 = (y0 + 1).min(height - 1);
        let fy = sy - y0 as f32;
        let row0 = &y_plane[y0 * stride..][..width];
        let row1 = &y_plane[y1 * stride..][..width];
        let uv_row = &uv_plane[(y0 / 2) * stride..][..width];
        let out = y * model_w;
        for x in 0..model_w {
            let sx = x as f32 * x_ratio;
            let x0 = sx as usize;
            let x1 = (x0 + 1).min(width - 1);
            let fx = sx - x0 as f32;
            let top = f32::from(row0[x0]) * (1.0 - fx) + f32::from(row0[x1]) * fx;
            let bottom = f32::from(row1[x0]) * (1.0 - fx) + f32::from(row1[x1]) * fx;
            let luma = top * (1.0 - fy) + bottom * fy;
            let uv = x0 & !1;
            let u = f32::from(uv_row[uv]) - 128.0;
            let v = f32::from(uv_row[uv + 1]) - 128.0;
            r_out[out + x] = (luma + 1.402 * v).clamp(0.0, 255.0) / 255.0;
            g_out[out + x] = (luma - 0.344 * u - 0.714 * v).clamp(0.0, 255.0) / 255.0;
            b_out[out + x] = (luma + 1.772 * u).clamp(0.0, 255.0) / 255.0;
        }
    }
}

/// Calculate resize dimensions to fit within target size while maintaining aspect ratio
/// Dimensions are rounded down to multiples of 32 for YOLO compatibility
fn calculate_resize_dims(width: usize, height: usize, target: usize) -> (usize, usize) {
    if width > height {
        let new_h = (height * target) / width;
        (target, (new_h / 32) * 32)
    } else {
        let new_w = (width * target) / height;
        ((new_w / 32) * 32, target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An NV12 frame with luma from `luma(x, y)` and one chroma pair everywhere.
    fn nv12(
        width: usize,
        height: usize,
        luma: impl Fn(usize, usize) -> u8,
        (u, v): (u8, u8),
    ) -> Vec<u8> {
        let mut frame = Vec::with_capacity(width * height * 3 / 2);
        for y in 0..height {
            frame.extend((0..width).map(|x| luma(x, y)));
        }
        for _ in 0..height / 2 {
            frame.extend((0..width / 2).flat_map(|_| [u, v]));
        }
        frame
    }

    #[test]
    fn nv12_converts_to_planar_rgb() {
        let (width, height) = (8, 4);
        let mut dst = vec![0.0; 3 * width * height];
        // Neutral chroma: every channel equals luma.
        let gray = nv12(width, height, |_, _| 128, (128, 128));
        nv12_to_planar_rgb(&gray, (width, height, width), &mut dst, (width, height));
        assert!(dst.iter().all(|&c| (c - 128.0 / 255.0).abs() < 1e-6));
        // Strong blue: B clips at 1 and R at 0.
        let blue = nv12(width, height, |_, _| 80, (240, 16));
        nv12_to_planar_rgb(&blue, (width, height, width), &mut dst, (width, height));
        let plane = width * height;
        assert!(dst[..plane].iter().all(|&r| r == 0.0));
        assert!(dst[2 * plane..].iter().all(|&b| b == 1.0));
    }

    #[test]
    fn nv12_resize_samples_luma_bilinearly() {
        // Luma rises by 10 per column; halving the width samples every other column.
        let (width, height) = (8, 2);
        let frame = nv12(width, height, |x, _| 10 * x as u8, (128, 128));
        let mut dst = vec![0.0; 3 * 4 * 2];
        nv12_to_planar_rgb(&frame, (width, height, width), &mut dst, (4, 2));
        let red: Vec<f32> = dst[..4].iter().map(|r| r * 255.0).collect();
        for (got, want) in red.iter().zip([0.0, 20.0, 40.0, 60.0]) {
            assert!((got - want).abs() < 1e-3, "{got} != {want}");
        }
        // A padded stride reads the same pixels.
        let mut padded = Vec::new();
        for row in frame.chunks(width) {
            padded.extend_from_slice(row);
            padded.extend_from_slice(&[255; 8]);
        }
        let mut dst_padded = vec![0.0; 3 * 4 * 2];
        nv12_to_planar_rgb(&padded, (width, height, 16), &mut dst_padded, (4, 2));
        assert_eq!(dst, dst_padded);
    }

    #[test]
    fn a_640x360_frame_goes_in_as_640x352() {
        assert_eq!(model_input_dims(640, 360), (640, 352));
    }

    #[test]
    fn test_resize_dims() {
        // Landscape image
        let (w, h) = calculate_resize_dims(1920, 1080, 640);
        assert_eq!(w, 640);
        assert!(h <= 640);
        assert_eq!(h % 32, 0);

        // Portrait image
        let (w, h) = calculate_resize_dims(1080, 1920, 640);
        assert!(w <= 640);
        assert_eq!(h, 640);
        assert_eq!(w % 32, 0);

        // Square image
        let (w, h) = calculate_resize_dims(640, 640, 640);
        assert_eq!(w, 640);
        assert_eq!(h, 640);
    }
}
