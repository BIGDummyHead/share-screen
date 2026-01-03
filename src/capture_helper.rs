use std::sync::Arc;

use enc_video::devices::DeviceSize;
use serde::Serialize;

/// Capture Types
pub enum CaptureType {
    /// Capture a camera (like your webcam)
    Camera,
    /// Capture the monitor at an index starting from 0
    Monitor(i32)
}

#[derive(Serialize)]
pub struct SerializedDeviceSize {
    pub width: usize,
    pub height: usize
}

impl SerializedDeviceSize {
    pub fn new(size: Arc<DeviceSize>) -> Self {
        Self {
            width: size.width as usize,
            height: size.height as usize
        }
    }
}