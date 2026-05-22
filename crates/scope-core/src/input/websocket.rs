use std::sync::mpsc;
use super::{SampleFrame, parse_csv_line};

/// Spawn an async task that connects to a WebSocket URL and sends parsed
/// SampleFrames through a channel. Requires a tokio runtime.
///
/// The WebSocket server sends one CSV line per text message.
pub fn spawn_ws_reader(
    url: String,
    has_timestamp: bool,
    rt: &tokio::runtime::Runtime,
) -> mpsc::Receiver<SampleFrame> {
    let (tx, rx) = mpsc::channel();

    rt.spawn(async move {
        use futures_util::StreamExt;

        let connect = tokio_tungstenite::connect_async(&url).await;
        let (ws_stream, _) = match connect {
            Ok(pair) => pair,
            Err(e) => {
                eprintln!("WebSocket connect error: {e}");
                return;
            }
        };

        let (_write, mut read) = ws_stream.split();
        while let Some(msg) = read.next().await {
            match msg {
                Ok(tokio_tungstenite::tungstenite::Message::Text(text)) => {
                    for line in text.lines() {
                        if let Some(frame) = parse_csv_line(line, has_timestamp) {
                            if tx.send(frame).is_err() {
                                return;
                            }
                        }
                    }
                }
                Ok(tokio_tungstenite::tungstenite::Message::Close(_)) => break,
                Err(e) => {
                    eprintln!("WebSocket read error: {e}");
                    break;
                }
                _ => {}
            }
        }
    });

    rx
}
