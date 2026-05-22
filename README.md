# kleis-scope

Standalone oscilloscope that consumes streams of numbers — just like a real scope.

## Quick Start

```bash
# Pipe CSV data from any source
echo "0.5,1.2,-0.3" | cargo run -p scope-native -- --stdin --channels 3 --rate 1000

# Stream from Kleis simulation
kleis simulate_graph --output - | cargo run -p scope-native -- --stdin --channels 2 --rate 10000

# Connect to a WebSocket
cargo run -p scope-native -- --ws ws://localhost:9100 --channels 4 --rate 44100

# Tail a growing file
cargo run -p scope-native -- --file data.csv --channels 2 --rate 1000
```

## Architecture

```
kleis-scope/
  crates/
    scope-core/       # Shared library: ring buffer, trigger, timebase, measurements, FFT
    scope-native/     # Desktop app (egui/wgpu)
    scope-web/        # WASM build (egui + glow)
```

## Input Format

CSV with one sample frame per line:

```
ch0, ch1, ch2          # no timestamp — uses declared sample rate
0.001, 1.5, 2.3, -0.7  # with --has-timestamp: first column is time
```

Lines starting with `#` are ignored.

## Features

- 4-channel display with phosphor-style traces
- Trigger: Auto / Normal / Single, rising/falling edge
- Measurements: Vpp, Vrms, frequency, period, rise/fall time
- Time and voltage cursors with delta readout
- Math channel: add, subtract, multiply, FFT spectrum
- Export: CSV, Typst plot source
- Input backends: stdin, WebSocket, TCP, file tail
