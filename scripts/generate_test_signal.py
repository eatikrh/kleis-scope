#!/usr/bin/env python3
"""Generate a 2-channel test signal for kleis-scope.

Usage:
    python3 scripts/generate_test_signal.py | ./target/release/kleis-scope --stdin --channels 2 --rate 10000
"""
import math

SAMPLE_RATE = 10000
DURATION_SECONDS = 10
NUM_SAMPLES = SAMPLE_RATE * DURATION_SECONDS

for i in range(NUM_SAMPLES):
    t = i / SAMPLE_RATE
    ch1 = math.sin(2 * math.pi * 100 * t)
    ch2 = 0.5 * math.sin(2 * math.pi * 250 * t + 0.3)
    print(f"{ch1:.6f},{ch2:.6f}")
