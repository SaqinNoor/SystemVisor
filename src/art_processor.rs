pub fn process_album_art(bytes: &[u8], max_cols: u32, max_rows: u32) -> Option<Vec<Vec<(u8, u8, u8)>>> {
    let img = image::load_from_memory(bytes).ok()?;
    let src_w = img.width();
    let src_h = img.height();
    if src_w == 0 || src_h == 0 {
        return None;
    }

    // Each terminal row = 2 pixel rows (half-block rendering)
    let avail_pixel_w = max_cols;
    let avail_pixel_h = max_rows * 2;

    let src_ratio = src_w as f64 / src_h as f64;
    let avail_ratio = avail_pixel_w as f64 / avail_pixel_h as f64;

    let (pixel_w, pixel_h) = if src_ratio > avail_ratio {
        let w = avail_pixel_w;
        let h = (w as f64 / src_ratio).round() as u32;
        (w, h.max(2))
    } else {
        let h = avail_pixel_h;
        let w = (h as f64 * src_ratio).round() as u32;
        (w.max(2), h)
    };

    // Ensure pixel_h is even (half-block constraint)
    let pixel_h = pixel_h / 2 * 2;

    let pixel_w = pixel_w as u32;
    let pixel_h = pixel_h as u32;

    let resized = image::imageops::resize(&img, pixel_w, pixel_h, image::imageops::FilterType::Lanczos3);

    let mut matrix = vec![vec![(0u8, 0u8, 0u8); pixel_w as usize]; pixel_h as usize];
    for y in 0..pixel_h as usize {
        for x in 0..pixel_w as usize {
            let px = resized.get_pixel(x as u32, y as u32);
            matrix[y][x] = (px[0], px[1], px[2]);
        }
    }

    Some(matrix)
}
