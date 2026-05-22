use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use super::{SampleFrame, parse_csv_line};

/// Spawn a background thread that tails a file (like `tail -f`), reading new
/// CSV lines as they are appended and sending parsed SampleFrames.
pub fn spawn_file_reader(
    path: PathBuf,
    has_timestamp: bool,
) -> mpsc::Receiver<SampleFrame> {
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let file = match File::open(&path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("File open error {}: {e}", path.display());
                return;
            }
        };
        let mut reader = BufReader::new(file);

        // Seek to end to only read new data
        if reader.seek(SeekFrom::End(0)).is_err() {
            eprintln!("Failed to seek to end of {}", path.display());
        }

        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => {
                    thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Ok(_) => {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() && !trimmed.starts_with('#') {
                        if let Some(frame) = parse_csv_line(trimmed, has_timestamp) {
                            if tx.send(frame).is_err() {
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("File read error: {e}");
                    break;
                }
            }
        }
    });

    rx
}
