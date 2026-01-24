use image::{ColorType, ImageEncoder, codecs::jpeg::JpegEncoder};
use rayon::prelude::*; // Import Rayon traits

pub fn compress_frame(raw_bgra: Vec<u8>, width: u32, height: u32) -> Vec<u8> {
    let mut compressed = Vec::new();

    let expected_len = (width * height * 4) as usize;
    if raw_bgra.len() != expected_len {
        return Vec::new();
    }

    // 1. Pre-allocate exact size with 0s (Much faster than pushing)
    let mut rgb_data = vec![0u8; (width * height * 3) as usize];

    // 2. Parallel BGRA -> RGB Conversion (The FPS Fix)
    // We process 4-byte chunks of input (BGRA) and 3-byte chunks of output (RGB) in parallel
    rgb_data
        .par_chunks_exact_mut(3)
        .zip(raw_bgra.par_chunks_exact(4))
        .for_each(|(rgb, bgra)| {
            rgb[0] = bgra[2]; // R
            rgb[1] = bgra[1]; // G
            rgb[2] = bgra[0]; // B
        });

    // 3. Encode
    // Setting quality to 60-70 is usually a sweet spot for streaming speed vs quality
    let encoder = JpegEncoder::new_with_quality(&mut compressed, 70);

    match encoder.write_image(&rgb_data, width, height, ColorType::Rgb8.into()) {
        Ok(_) => {}
        Err(e) => {
            eprintln!("JPEG Encoding error: {:?}", e);
            return Vec::new();
        }
    }

    compressed
}