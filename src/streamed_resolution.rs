use std::sync::Arc;

use async_stream::stream;
use async_web::web::{Resolution, resolution::get_status_header};
use tokio::sync::{Mutex, broadcast::Receiver};

/// # Streamed Resolution
///
/// Represents a streamed broadcast from a subscriber of the broadcast channel.
pub struct StreamedResolution {
    //broadcast channel
    rx: Arc<Mutex<Receiver<Vec<u8>>>>,
}

impl StreamedResolution {
    /// create a new streamed resolution from a receiver.
    pub fn from_receiver(rx: Receiver<Vec<u8>>) -> Self {
        Self {
            rx: Arc::new(Mutex::new(rx)),
        }
    }
}

impl Resolution for StreamedResolution {
    //return 200 header, no other header is applicable
    fn get_headers(&self) -> std::pin::Pin<Box<dyn Future<Output = Vec<String>> + Send + '_>> {
        Box::pin(async move { vec![get_status_header(200)] })
    }

    //get content stream
    fn get_content(&self) -> std::pin::Pin<Box<dyn futures::Stream<Item = Vec<u8>> + Send>> {
        let rx = self.rx.clone();

        Box::pin(stream! {
            loop {

                yield match rx.lock().await.recv().await {
                    Ok(data) => data,
                    _ => continue
                };
            }
        })
    }

    fn resolve(self) -> Box<dyn Resolution + Send + 'static> {
        Box::new(self)
    }
}
