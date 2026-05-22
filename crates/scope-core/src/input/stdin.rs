use std::io::{self, BufRead};
use std::sync::mpsc;
use std::thread;

use super::SampleFrame;
use super::parse_csv_line;

/// Spawn a background thread that reads CSV lines from stdin and sends
/// parsed SampleFrames through a channel.
pub fn spawn_stdin_reader(has_timestamp: bool) -> mpsc::Receiver<SampleFrame> {
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let stdin = io::stdin();
        let reader = stdin.lock();
        for line in reader.lines() {
            let Ok(line) = line else { break };
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
