use std::sync::Arc;

use win_video::devices::Dimensions;
use serde::Serialize;

/// Capture Types
pub enum CaptureType {
    /// Capture a camera (like your webcam)
    Camera,
    /// Capture the monitor at an index starting from 0
    Monitor(i32)
}

#[derive(Serialize)]
pub struct SerializedDimensions {
    pub width: usize,
    pub height: usize
}

impl SerializedDimensions {
    pub fn new(size: Arc<Dimensions>) -> Self {
        Self {
            width: size.width as usize,
            height: size.height as usize
        }
    }
}