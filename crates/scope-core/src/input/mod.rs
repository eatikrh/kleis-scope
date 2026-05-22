pub mod stdin;
pub mod websocket;
pub mod tcp;
pub mod file;

/// A parsed frame of samples: one value per channel.
#[derive(Debug, Clone)]
pub struct SampleFrame {
    pub timestamp: Option<f64>,
    pub values: Vec<f32>,
}

/// Parse a CSV line into a SampleFrame.
/// Format: `[timestamp,] ch0, ch1, ch2, ...`
/// If `has_timestamp` is true, the first column is the timestamp.
pub fn parse_csv_line(line: &str, has_timestamp: bool) -> Option<SampleFrame> {
    let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
    if parts.is_empty() {
        return None;
    }

    let (timestamp, value_parts) = if has_timestamp && parts.len() > 1 {
        let ts = parts[0].parse::<f64>().ok();
        (ts, &parts[1..])
    } else {
        (None, parts.as_slice())
    };

    let values: Vec<f32> = value_parts
        .iter()
        .filter_map(|s| s.parse::<f32>().ok())
        .collect();

    if values.is_empty() {
        return None;
    }

    Some(SampleFrame { timestamp, values })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_csv() {
        let frame = parse_csv_line("1.5, 2.3, -0.7", false).unwrap();
        assert!(frame.timestamp.is_none());
        assert_eq!(frame.values.len(), 3);
        assert!((frame.values[0] - 1.5).abs() < 1e-6);
    }

    #[test]
    fn parse_csv_with_timestamp() {
        let frame = parse_csv_line("0.001, 1.5, 2.3", true).unwrap();
        assert!((frame.timestamp.unwrap() - 0.001).abs() < 1e-9);
        assert_eq!(frame.values.len(), 2);
    }

    #[test]
    fn parse_empty_line() {
        assert!(parse_csv_line("", false).is_none());
    }

    #[test]
    fn parse_garbage() {
        assert!(parse_csv_line("hello, world", false).is_none());
    }
}
