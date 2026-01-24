pub mod captures;
pub mod frame_compressor;
pub mod streamed_resolution;

use async_web::web::Resolution;
use async_web::web::resolution::file_resolution::FileResolution;
use std::sync::Arc;
use tokio::sync::broadcast;

use async_web::resolve;

use async_web::web::{App, resolution::json_resolution::JsonResolution};
use win_video::i_capture::ICapture;

use crate::captures::{CaptureType, SerializedDimensions};
use crate::streamed_resolution::StreamedResolution;

use crate::frame_compressor::compress_frame;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let capture_type = get_user_capture_type();

    println!("Initializing capture component now...");

    let capture = capture_type.activate()?;

    let dimensions = Arc::new(capture.get_dimensions()?);

    //let (compressed_sender,  compressed_receiver) = mpsc::channel::<Vec<u8>>(buffer);
    //let compressed_receiver_ref = Arc::new(Mutex::new(compressed_receiver));

    let (compressed_sender, _) = broadcast::channel::<Vec<u8>>(100);

    let compressed_sender_clone = Arc::new(compressed_sender);

    //start receiving uncompressed data
    spawn_frame_capture(capture.clone());
    spawn_frame_compressor(capture.clone(), compressed_sender_clone.clone());

    println!("Components initialized\nStarting web server...");

    let host_address = local_ip_address::local_ip()?;
    let server_socket = format!("{host_address:?}:80");

    //create the web app for sending data...
    let mut app = App::bind(100, &server_socket).await?;

    route_app(
        &mut app,
        compressed_sender_clone.clone(),
        dimensions.clone(),
    )
    .await;

    let _ = app.start();

    println!("Now hosting on http://{server_socket}");

    loop {
        let _ = prompt("Press enter to quit...");
        break;
    }

    let _ = app.close().await;

    Ok(())
}

/// # Route App
///
/// Adds routing to the web app, providing content from the content folder, changing the home page, and setting up the streamed resolutions.
async fn route_app(
    app: &mut App,
    broad_tx: Arc<broadcast::Sender<Vec<u8>>>,
    dimensions: Arc<win_video::devices::Dimensions>,
) -> () {
    //home page for serving the streamables
    app.add_or_change_route(
        "/",
        async_web::web::Method::GET,
        None,
        resolve!(_req, {
            FileResolution::new("content/stream.html").resolve()
        }),
    )
    .await
    .expect("Failed to change home page.");

    app.add_or_panic(
        "/content/{file}",
        async_web::web::Method::GET,
        None,
        resolve!(req, {
            let file = {
                let req_lock = req.lock().await;

                let file: &String = req_lock.variables.get("file").unwrap();

                file.clone()
            };

            let path = format!("content/{file}");

            FileResolution::new(&path).resolve()
        }),
    )
    .await;

    let dimensions_clone = dimensions.clone();
    app.add_or_panic(
        "/stream/dimensions",
        async_web::web::Method::GET,
        None,
        resolve!(_req, moves[dimensions_clone], {
            match JsonResolution::serialize(SerializedDimensions::from_dimensions(
                dimensions_clone.clone(),
            )) {
                Ok(serialized) => serialized.resolve(),
                Err(err_r) => err_r.resolve(),
            }
        }),
    )
    .await;

    let broad_tx_clone = broad_tx.clone();
    //streamed POST for the content of the device
    app.add_or_panic(
        "/stream",
        async_web::web::Method::POST,
        None,
        resolve!(_req, moves[broad_tx_clone], {
            let rx = broad_tx_clone.subscribe();

            StreamedResolution::from_receiver(rx).resolve()
        }),
    )
    .await;
}

/// # Spawn Frame Capture
///
/// Spawns a tokio task that starts and awaits the capture function of the device.
fn spawn_frame_capture(capture: Arc<dyn ICapture<CaptureOutput = Vec<u8>>>) {
    tokio::spawn(async move {
        match capture.start_capturing().await {
            Err(e) => eprintln!("{e}"),
            _ => {}
        };
    });
}

/// # Spawn Compressor
///
/// Spawns a separate task that compresses incoming frames of the device and sends them to the broadcast channel
///
/// Note: `This should be called with the spawn_frame_capture (does not matter the order)`
fn spawn_frame_compressor(
    capture: Arc<dyn ICapture<CaptureOutput = Vec<u8>>>,
    compressed_frames: Arc<broadcast::Sender<Vec<u8>>>,
) {
    let rx = capture.clone_receiver();
    let dimensions = capture.get_dimensions().expect("Could not get dimensions.");

    tokio::spawn(async move {
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
                let _ = compressed_frames.send(packet);
            }
        }
    });
}

/// # get user capture type
///
/// Retrieves the user's preferred capture type.
fn get_user_capture_type() -> CaptureType {
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
                capture = Some(CaptureType::Monitor(user_request_monitor_index()));
            }
            _ => {
                println!("Invalid choice, please choose again from the following\n");
                continue;
            }
        }
    }

    capture.unwrap()
}

/// # User Request Monitor index
///
/// Retrieves the user's preferred monitor index. This is called within the `get_user_capture_type` function if the answer proceeds with Monitor
fn user_request_monitor_index() -> i32 {
    let mut monitor_index = None;

    while let None = monitor_index {
        let m_count = unsafe { win_video::devices::get_monitor_count() };

        let monitor = match prompt(&format!(
            "Choose a monitor to share (from 1 to {}): ",
            m_count
        )) {
            Err(m_e) => {
                println!("Failed to choose monitor: {m_e}");
                continue;
            }
            Ok(m_choice) => m_choice.trim().to_lowercase().parse::<i32>(),
        };

        let index = match monitor {
            Ok(i) => {
                if i <= 0 {
                    print!("Invalid index provided.");
                    continue;
                }

                Some(i - 1)
            }
            Err(m_e) => {
                println!("Failed to parse answer: {m_e}");
                continue;
            }
        };

        monitor_index = index;
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
