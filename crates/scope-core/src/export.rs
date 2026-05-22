use crate::channel::Channel;
use crate::timebase::Timebase;

/// Export the visible window as a CSV string.
/// Header: time, ch1, ch2, ...
pub fn export_csv(channels: &[Channel], timebase: &Timebase) -> String {
    let mut out = String::new();

    // Header
    out.push_str("time");
    for ch in channels {
        if ch.enabled {
            out.push(',');
            out.push_str(&ch.label);
        }
    }
    out.push('\n');

    let max_len = channels
        .iter()
        .filter(|c| c.enabled)
        .map(|c| c.buffer.len())
        .max()
        .unwrap_or(0);

    if max_len == 0 {
        return out;
    }

    let start = timebase.window_start(max_len);
    let vis = timebase.visible_samples().min(max_len.saturating_sub(start));

    for i in 0..vis {
        let t = (start + i) as f64 / timebase.sample_rate;
        out.push_str(&format!("{:.9}", t));
        for ch in channels {
            if ch.enabled {
                let val = ch.buffer.get(start + i).unwrap_or(0.0);
                out.push_str(&format!(",{:.6}", val));
            }
        }
        out.push('\n');
    }

    out
}

/// Export the visible window as Typst source for a line plot.
pub fn export_typst(channels: &[Channel], timebase: &Timebase) -> String {
    let mut out = String::new();

    out.push_str("#import \"@preview/cetz:0.3.4\": canvas, draw\n");
    out.push_str("#import \"@preview/cetz-plot:0.1.1\": plot\n\n");
    out.push_str("#canvas({\n");
    out.push_str("  import draw: *\n");
    out.push_str("  plot.plot(\n");
    out.push_str("    size: (12, 6),\n");
    out.push_str("    x-label: [Time (s)],\n");
    out.push_str("    y-label: [Voltage],\n");
    out.push_str("    {\n");

    let colors = ["green", "yellow", "cyan", "magenta"];

    for (ch_idx, ch) in channels.iter().enumerate() {
        if !ch.enabled || ch.buffer.is_empty() {
            continue;
        }

        let max_len = ch.buffer.len();
        let start = timebase.window_start(max_len);
        let vis = timebase.visible_samples().min(max_len.saturating_sub(start));

        // Downsample to at most 500 points for Typst
        let step = (vis / 500).max(1);

        let color = colors[ch_idx % colors.len()];
        out.push_str(&format!(
            "      plot.add(stroke: {color}, label: [{}], (\n",
            ch.label
        ));

        let mut first = true;
        let mut i = 0;
        while i < vis {
            if let Some(val) = ch.buffer.get(start + i) {
                let t = (start + i) as f64 / timebase.sample_rate;
                if !first {
                    out.push_str(",\n");
                }
                out.push_str(&format!("        ({:.9}, {:.6})", t, val));
                first = false;
            }
            i += step;
        }

        out.push_str("\n      ))\n");
    }

    out.push_str("    }\n");
    out.push_str("  )\n");
    out.push_str("})\n");

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::Channel;
    use crate::timebase::Timebase;

    #[test]
    fn csv_export_basic() {
        let mut ch = Channel::new(0);
        for i in 0..100 {
            ch.push_sample(i as f32 * 0.1);
        }
        let tb = Timebase::new(1000.0);
        let csv = export_csv(&[ch], &tb);
        assert!(csv.starts_with("time,CH1\n"));
        let lines: Vec<&str> = csv.lines().collect();
        assert!(lines.len() > 2);
    }

    #[test]
    fn typst_export_basic() {
        let mut ch = Channel::new(0);
        for i in 0..100 {
            ch.push_sample((i as f32 * 0.1).sin());
        }
        let tb = Timebase::new(1000.0);
        let typst = export_typst(&[ch], &tb);
        assert!(typst.contains("plot.add"));
        assert!(typst.contains("CH1"));
    }
}
