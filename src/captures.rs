use std::sync::Arc;

use serde::Serialize;
use win_video::{devices::{Cameras, Dimensions, Monitor}, i_capture::ICapture};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};

/// The capture types available for the program.
pub enum CaptureType {
    /// Capture a camera (like your webcam)
    Camera,
    /// Capture the monitor at an index starting from 0
    Monitor(i32),
}

impl CaptureType {
    /// # Activate Capture device
    ///
    /// Takes a capture device type and activates it using the win_video library.
    ///
    /// Returns the device activated.
    /// 
    /// The function also has the chance of returning an err for the following reasons:
    /// CoInitializeEx failed,
    /// No video devices 
    /// No valid monitor devices
    /// Monitor index out of range
    /// And other window errors.
    pub fn activate(self) -> Result<Arc<dyn ICapture<CaptureOutput = Vec<u8>>>, Box<dyn std::error::Error>> {
        let capture;

        match self {
            CaptureType::Camera => unsafe {
                if CoInitializeEx(None, COINIT_MULTITHREADED) != windows::Win32::Foundation::S_OK {
                    return Err("Failed to CoIntialize for camera.".into());
                }

                let video_devices = Cameras::new()?;

                if video_devices.devices.len() == 0 {
                    return Err("No camera devices to capture.".into());
                }

                println!("Activating device (this may take a second)...");

                let device = video_devices.activate_device(
                    video_devices.devices[0],
                    Some(win_video::devices::camera::Output::RGB32),
                )?;

                capture = device as Arc<dyn ICapture<CaptureOutput = Vec<u8>>>;
            },
            CaptureType::Monitor(m) => unsafe {
                capture = Monitor::from_monitor(m as u32)? as Arc<dyn ICapture<CaptureOutput = Vec<u8>>>;
            },
        }

        Ok(capture)
    }
}

/// Rest API Json for capture dimensions.
#[derive(Serialize)]
pub struct SerializedDimensions {
    /// width of device.
    pub width: usize,
    /// height of device
    pub height: usize,
}

impl SerializedDimensions {
    /// Converts a dimensions reference toa serialized API dimension.
    pub fn from_dimensions(size: Arc<Dimensions>) -> Self {
        Self {
            width: size.width as usize,
            height: size.height as usize,
        }
    }
}
