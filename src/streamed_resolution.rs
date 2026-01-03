use std::sync::Arc;

use async_stream::stream;
use async_web::web::{Resolution, resolution::get_status_header};
use enc_video::devices::DeviceSize;
use image::{ColorType, ImageEncoder, codecs::jpeg::JpegEncoder};
use tokio::sync::{Mutex, mpsc::Receiver};

pub struct StreamedResolution {
    rx: Arc<Mutex<Receiver<Vec<u8>>>>,
    dimensions: Arc<DeviceSize>,
}

impl StreamedResolution {
    /// create a new streamed resolution from a receiver.
    pub fn new(
        rx: Arc<Mutex<Receiver<Vec<u8>>>>,
        dimensions: Arc<DeviceSize>,
    ) -> Box<dyn Resolution + Send> {
        let res = Self { rx, dimensions };

        Box::new(res)
    }
}

impl Resolution for StreamedResolution {
    fn get_headers(&self) -> std::pin::Pin<Box<dyn Future<Output = Vec<String>> + Send + '_>> {
        Box::pin(async move { vec![get_status_header(200)] })
    }

    fn get_content(&self) -> std::pin::Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>> {
        let rx = self.rx.clone();
        let dimensions = self.dimensions.clone();

        let content_stream = stream! {
            loop {
                let data: Option<Vec<u8>> = {
                    let mut rx = rx.lock().await;
                    rx.recv().await
                };

                if data.is_none() {
                    break;
                }

               let raw_data = data.unwrap();
               let (width, height) = (dimensions.width, dimensions.height);

                let compressed = tokio::task::spawn_blocking(move || {
                    compress_frame(raw_data, width, height)
                }).await.unwrap_or_default();

                if !compressed.is_empty() {
                    let len = compressed.len() as u32;

                    // Create a single packet: [4 bytes length] + [JPEG bytes]
                    let mut packet = Vec::with_capacity(4 + compressed.len());
                    packet.extend_from_slice(&len.to_le_bytes()); // Little Endian length
                    packet.extend_from_slice(&compressed);

                    yield packet;
                }
            }
        };

        Box::pin(content_stream)
    }
}

use rayon::prelude::*; // Import Rayon traits

fn compress_frame(raw_bgra: Vec<u8>, width: u32, height: u32) -> Vec<u8> {
    let mut compressed = Vec::new();

    let expected_len = (width * height * 4) as usize;
    if raw_bgra.len() != expected_len {
        return Vec::new();
    }

    // 1. Pre-allocate exact size with 0s (Much faster than pushing)
    let mut rgb_data = vec![0u8; (width * height * 3) as usize];

    // 2. Parallel BGRA -> RGB Conversion (The FPS Fix)
    // We process 4-byte chunks of input (BGRA) and 3-byte chunks of output (RGB) in parallel
    rgb_data.par_chunks_exact_mut(3)
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