
/// Processes raw thumbnail bytes (JPG/PNG), downsamples to exactly W x 2H,
/// and returns a 2D RGB matrix where matrix[row][col] is (r, g, b).
pub fn process_album_art(bytes: &[u8], width: u32, height: u32) -> Option<Vec<Vec<(u8, u8, u8)>>> {
    let img = image::load_from_memory(bytes).ok()?;
    
    // We downsample to exactly W width and 2H height.
    // Each terminal cell contains 2 vertical pixels represented by a single half-block character.
    let target_width = width;
    let target_height = height * 2;
    
    let resized = image::imageops::resize(
        &img,
        target_width,
        target_height,
        image::imageops::FilterType::Lanczos3
    );
    
    let mut matrix = vec![vec![(0u8, 0u8, 0u8); target_width as usize]; target_height as usize];
    
    for y in 0..target_height as usize {
        for x in 0..target_width as usize {
            let pixel = resized.get_pixel(x as u32, y as u32);
            matrix[y][x] = (pixel[0], pixel[1], pixel[2]);
        }
    }
    
    Some(matrix)
}
