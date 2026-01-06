pub mod capture_helper;
pub mod streamed_resolution;

use std::sync::Arc;
use tokio::sync::broadcast;

use async_web::web::{
    App,
    resolution::{
        empty_resolution::EmptyResolution, file_text_resolution::FileTextResolution,
        json_resolution::JsonResolution,
    },
};
use tokio::task::JoinHandle;
use win_video::{
    devices::{Cameras, Monitor},
    i_capture::ICapture,
};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};

use crate::streamed_resolution::StreamedResolution;
use crate::{
    capture_helper::{CaptureType, SerializedDimensions},
    streamed_resolution::compress_frame,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let capture_type = get_capture_type_from_user();

    println!("Initializing capture component now...");

    let capture = initialize_capture(capture_type)?;

    let dimensions = Arc::new(capture.get_dimensions()?);

    //let (compressed_sender,  compressed_receiver) = mpsc::channel::<Vec<u8>>(buffer);
    //let compressed_receiver_ref = Arc::new(Mutex::new(compressed_receiver));

    let (compressed_sender, _) = broadcast::channel::<Vec<u8>>(100);

    let compressed_sender_clone = Arc::new(compressed_sender);

    //start receiving uncompressed data
    start_capturing(capture.clone());
    start_receiving(capture.clone(), compressed_sender_clone.clone());

    println!("Components initialized\nStarting web server...");

    //create the web app for sending data...
    let mut app = App::bind(100, "10.0.0.83:5074").await?;

    init_app(
        &mut app,
        compressed_sender_clone.clone(),
        dimensions.clone(),
    )
    .await;
    let server_thread = app.start().await;

    println!("Server started");

    let _ = server_thread.await;

    Ok(())
}

//add routing and start the process of sharing data...
async fn init_app(
    app: &mut App,
    broad_tx: Arc<broadcast::Sender<Vec<u8>>>,
    dimensions: Arc<win_video::devices::Dimensions>,
) -> () {
    //home page for serving the streamables
    app.add_or_change_route(
        "/",
        async_web::web::Method::GET,
        None,
        Arc::new(|_| Box::pin(async move { FileTextResolution::new("stream.html") })),
    )
    .await
    .expect("Failed to change home page.");

    let dimensions_clone = dimensions.clone();
    app.add_or_panic(
        "/stream/dimensions",
        async_web::web::Method::GET,
        None,
        Arc::new(move |_| {
            let dimensions = dimensions_clone.clone();

            Box::pin(async move {
                let resolved = JsonResolution::new(SerializedDimensions::new(dimensions.clone()));

                if resolved.is_err() {
                    return EmptyResolution::new(500);
                }

                let resolved = resolved.unwrap();

                resolved.into_resolution()
            })
        }),
    )
    .await;

    //streamed POST for the content of the device
    app.add_or_panic(
        "/stream",
        async_web::web::Method::POST,
        None,
        Arc::new(move |_| {
            let tx = broad_tx.clone();

            Box::pin(async move {
                println!("Creating new resolution stream");

                let rx = tx.subscribe();

                let resolution = StreamedResolution::new(rx);

                resolution
            })
        }),
    )
    .await;
}

fn start_capturing(capture: Arc<dyn ICapture<CaptureOutput = Vec<u8>>>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let capture_result = capture.start_capturing().await;

        if let Err(e) = capture_result {
            println!("Capture stopped: {e}");
        }
    })
}

fn start_receiving(
    capture: Arc<dyn ICapture<CaptureOutput = Vec<u8>>>,
    tx: Arc<broadcast::Sender<Vec<u8>>>,
) -> JoinHandle<()> {
    let rx = capture.clone_receiver();
    let dimensions = capture.get_dimensions().expect("Could not get dimensions.");

    let handle = tokio::spawn(async move {
        loop {
            let data = {
                let mut guard = rx.lock().await;
                guard.recv().await
            };

            if let None = data {
                break; //done receiving data
            }

            let raw_data = data.unwrap();

            let (width, height) = (dimensions.width, dimensions.height);

            let compressed =
                tokio::task::spawn_blocking(move || compress_frame(raw_data, width, height))
                    .await
                    .unwrap_or_default();

            if !compressed.is_empty() {
                let len = compressed.len() as u32;

                // Create a single packet: [4 bytes length] + [JPEG bytes]
                let mut packet = Vec::with_capacity(4 + compressed.len());
                packet.extend_from_slice(&len.to_le_bytes()); // Little Endian length
                packet.extend_from_slice(&compressed);

                //send the compressed data
                let _ = tx.send(packet);
            }
        }
    });

    handle
}

/*

let compressed_tx_clone = compressed_tx.clone();
    let compressed_rx = Arc::new(Mutex::new(compressed_rx));
    let dimensions_clone = dimensions.clone();
    tokio::spawn(async move {
        loop {


        }
    });

 */

fn initialize_capture(
    capture_type: CaptureType,
) -> Result<Arc<dyn ICapture<CaptureOutput = Vec<u8>>>, Box<dyn std::error::Error>> {
    let capture;

    match capture_type {
        CaptureType::Camera => unsafe {
            let result = CoInitializeEx(None, COINIT_MULTITHREADED);

            if result != windows::Win32::Foundation::S_OK {
                return Err("Failed to CoIntialize for camera.".into());
            }

            println!("CoInitialize done");

            let video_devices = Cameras::new()?;

            println!("Video devices aggregated");

            let device = video_devices.activate_device(
                video_devices.devices[0],
                Some(win_video::devices::camera::Output::RGB32),
            )?;

            println!("Activated device.");

            capture = device as Arc<dyn ICapture<CaptureOutput = Vec<u8>>>;
        },
        CaptureType::Monitor(m) => unsafe {
            let monitor = Monitor::from_monitor(m as u32)?;

            capture = monitor as Arc<dyn ICapture<CaptureOutput = Vec<u8>>>;
        },
    }

    Ok(capture)
}

fn get_capture_type_from_user() -> CaptureType {
    let mut capture: Option<CaptureType> = None;

    while let None = capture {
        let answer = prompt("Choose capture type: \r\n   - (1) Camera\r\n   - (2) Monitor");

        if let Err(e) = answer {
            println!("Invalid input: {e}");
            continue;
        }

        let answer = answer.unwrap().trim().to_lowercase();

        if answer.is_empty() || answer.len() <= 0 || answer.len() > 1 {
            println!("Invalid input! Please follow the prompt\n");
            continue;
        }

        let answer = answer.chars().next();

        if answer.is_none() {
            println!("No answer provided!");
            continue;
        }

        let answer = answer.unwrap();

        match answer {
            '1' => {
                capture = Some(CaptureType::Camera);
            }
            '2' => {
                capture = Some(CaptureType::Monitor(request_monitor()));
            }
            _ => {
                println!("Invalid choice, please choose again from the following\n");
                continue;
            }
        }
    }

    capture.unwrap()
}

fn request_monitor() -> i32 {
    let mut monitor_index = None;

    while let None = monitor_index {
      
        let m_count;

        unsafe  {
            m_count = win_video::devices::get_monitor_count();
        }

        let monitor = prompt(&format!("Choose a monitor to share (from 1 to {}): ", m_count));

        if let Err(m_e) = monitor {
            println!("Failed to choose monitor: {m_e}");
            continue;
        }

        let monitor = monitor.unwrap().trim().to_lowercase();

        let monitor_index_parse = monitor.parse::<i32>();

        if let Err(m_e) = monitor_index_parse {
            println!("Failed to parse answer: {m_e}");
            continue;
        }

        let index = monitor_index_parse.unwrap();

        if index <= 0 {
            println!("Invalid index provided.");
            continue;
        }

        monitor_index = Some(index - 1);
    }

    monitor_index.unwrap()
}

/// Prompt the user with a question and get an aswer.
fn prompt(question: &str) -> std::io::Result<String> {
    println!("{question}");

    let mut answer = String::new();

    std::io::stdin().read_line(&mut answer)?;

    Ok(answer)
}
