use std::sync::mpsc;
use super::{SampleFrame, parse_csv_line};

/// Spawn an async task that connects to a TCP address and reads newline-delimited
/// CSV data, sending parsed SampleFrames through a channel.
pub fn spawn_tcp_reader(
    addr: String,
    has_timestamp: bool,
    rt: &tokio::runtime::Runtime,
) -> mpsc::Receiver<SampleFrame> {
    let (tx, rx) = mpsc::channel();

    rt.spawn(async move {
        use tokio::io::AsyncBufReadExt;
        use tokio::io::BufReader;

        let stream = match tokio::net::TcpStream::connect(&addr).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("TCP connect error to {addr}: {e}");
                return;
            }
        };

        let reader = BufReader::new(stream);
        let mut lines = reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some(frame) = parse_csv_line(trimmed, has_timestamp) {
                if tx.send(frame).is_err() {
                    break;
                }
            }
        }
    });

    rx
}
