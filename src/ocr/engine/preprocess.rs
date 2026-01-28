use image::DynamicImage;

pub(super) fn preprocess_for_ocr_variants(image: DynamicImage, scale: u32) -> Vec<DynamicImage> {
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    let mut luma = image::GrayImage::new(width, height);

    for (x, y, pixel) in rgba.enumerate_pixels() {
        let [r, g, b, a] = pixel.0;
        let alpha = a as f32 / 255.0;
        let r = (r as f32 * alpha + 255.0 * (1.0 - alpha)).round() as u8;
        let g = (g as f32 * alpha + 255.0 * (1.0 - alpha)).round() as u8;
        let b = (b as f32 * alpha + 255.0 * (1.0 - alpha)).round() as u8;
        let value = (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32).round() as u8;
        luma.put_pixel(x, y, image::Luma([value]));
    }

    let resized = if scale > 1 {
        image::imageops::resize(
            &luma,
            width.saturating_mul(scale),
            height.saturating_mul(scale),
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        luma
    };

    let stretched = contrast_stretch(&resized);
    let threshold = (0.65 * 255.0) as u8;
    let bin = binarize(&stretched, threshold);
    vec![
        DynamicImage::ImageLuma8(bin),
        DynamicImage::ImageLuma8(stretched),
    ]
}

pub(super) fn ocr_scale(width: u32) -> u32 {
    let max_width = 6000u32;
    let mut scale = 3u32;
    while width.saturating_mul(scale) > max_width && scale > 1 {
        scale -= 1;
    }
    scale.max(1)
}

fn contrast_stretch(image: &image::GrayImage) -> image::GrayImage {
    let mut min = 255u8;
    let mut max = 0u8;
    for pixel in image.pixels() {
        let value = pixel[0];
        min = min.min(value);
        max = max.max(value);
    }

    if max <= min {
        return image.clone();
    }

    let scale = 255.0 / (max as f32 - min as f32);
    let mut output = image.clone();
    for pixel in output.pixels_mut() {
        let value = pixel[0];
        let stretched = ((value.saturating_sub(min)) as f32 * scale).round() as u8;
        pixel[0] = stretched;
    }
    output
}

fn binarize(image: &image::GrayImage, threshold: u8) -> image::GrayImage {
    let mut output = image.clone();
    for pixel in output.pixels_mut() {
        pixel[0] = if pixel[0] > threshold { 255 } else { 0 };
    }
    output
}
