# Model compatibility

Research date: 2026-11-08. Hardware inspected: Mac Studio, Apple M5 Max, 64 GB unified memory, 40-core GPU, Metal 4, macOS 27.0.1.

## Initial candidate: Wan 2.2 TI2V-5B via mlx-video
The upstream project documents a single-model text/image-to-video pipeline and a distinct Wan 2.2 VAE. This is a practical first evaluation candidate before investigating the larger dual-model 14B pipelines.

Source: https://github.com/Blaizzy/mlx-video/blob/main/mlx_video/models/wan_2/README.md

Status: upstream support documented; NOT installed, executed, or verified on this machine. No checkpoint downloaded. Peak memory, render duration, output quality, commercial-use license verification and five-second benchmark remain pending. 64 GB capacity alone is not proof of successful inference. No performance claims are made.

FFmpeg is not installed. Python is available through the system installation; use a dedicated virtual environment for inference. Keep worker dependencies separate from the Rust application.

## Benchmark acceptance
Record exact worker commit, checkpoint revision/hash, license, quantization, prompt, seed, resolution, frame count/fps, wall time, peak unified memory, output hash and ffprobe results. Verify the video visually and play it in the application before marking support verified.
