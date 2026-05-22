#!/usr/bin/env python3
"""Lorenz strange attractor — streams x,z as CSV to stdout for kleis-scope XY mode.

Usage:
    python3 scripts/lorenz_attractor.py | ./target/release/kleis-scope --stdin --channels 2 --rate 5000

Then check the XY checkbox in the toolbar. Adjust Ch1/Ch2 V/div to ~10.
"""
import sys
import time

# Lorenz parameters
SIGMA = 10.0
RHO = 28.0
BETA = 8.0 / 3.0

# Integration
DT = 0.0002          # RK4 step size
STEPS_PER_SAMPLE = 4  # integrate multiple sub-steps per output
OUTPUT_RATE = 5000    # samples/sec output to scope
SLEEP = 1.0 / OUTPUT_RATE

# Initial condition (off-attractor so we see the transient)
x, y, z = 1.0, 1.0, 1.0

def lorenz(x, y, z):
    dx = SIGMA * (y - x)
    dy = x * (RHO - z) - y
    dz = x * y - BETA * z
    return dx, dy, dz

def rk4_step(x, y, z, dt):
    k1x, k1y, k1z = lorenz(x, y, z)
    k2x, k2y, k2z = lorenz(x + 0.5*dt*k1x, y + 0.5*dt*k1y, z + 0.5*dt*k1z)
    k3x, k3y, k3z = lorenz(x + 0.5*dt*k2x, y + 0.5*dt*k2y, z + 0.5*dt*k2z)
    k4x, k4y, k4z = lorenz(x + dt*k3x, y + dt*k3y, z + dt*k3z)
    x += dt * (k1x + 2*k2x + 2*k3x + k4x) / 6.0
    y += dt * (k1y + 2*k2y + 2*k3y + k4y) / 6.0
    z += dt * (k1z + 2*k2z + 2*k3z + k4z) / 6.0
    return x, y, z

try:
    while True:
        for _ in range(STEPS_PER_SAMPLE):
            x, y, z = rk4_step(x, y, z, DT)
        # Output x → Ch1 (X axis), z → Ch2 (Y axis)
        # Shift z down by 25 so the attractor is centered around 0
        print(f"{x:.6f},{z - 25.0:.6f}", flush=True)
        time.sleep(SLEEP)
except (BrokenPipeError, KeyboardInterrupt):
    sys.exit(0)
