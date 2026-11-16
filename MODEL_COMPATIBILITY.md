# Model compatibility

Verified 2026-11-08 on Mac Studio, Apple M5 Max, 64 GB unified memory, 40-core GPU, Metal 4, macOS 27.0.1.

## Wan 2.2 TI2V-5B MLX Q8 — text-to-video VERIFIED

Checkpoint: `Anes1032/Wan2.2-TI2V-5B-mlx-q8`, 19.58 GB. Worker: `mlx-video` commit `87db56a51758fefb748a359b90a5283bb8ba4837` in isolated Python 3.12. The installed entry point is `mlx_video.models.wan_2.generate`; an older upstream README module path did not match the installed layout.

| Setting / result | Measured value |
| --- | --- |
| Generation | 704 × 1280, 121 frames, 24 fps |
| Duration | 5.0417 seconds |
| Diffusion | 40 steps, seed 42, Q8 transformer |
| Wall time | 1,200.55 seconds (~20 minutes) |
| Denoising | 1,003.3 seconds |
| VAE decoding | 182.8 seconds |
| Tiling | Auto: 512 px spatial, 64 frames temporal |
| MLX reported peak | 55,919,660,232 bytes (55.9 GB / 52.1 GiB) |
| Encoded output | H.264, 704 × 1280, 24 fps; full FFmpeg decode passed |
| Finished test export | Exact 5.00 seconds, 1080 × 1920, H.264/AAC, Kokoro narration |

The finished export is resized/cropped from 704 × 1280, **not native 1080p generation**. The first bootstrap worker was interrupted before inference; the second attempt completed. A real MCP request created the durable job. Output appeared in managed assets and was then assigned to a test episode, voiced, approved, and exported through MCP.

The benchmark depicts a film reel. Frames at the start and midpoint were visually inspected. Technical validation is not proof of cinematic quality, identity consistency, or lip synchronization. A later human image-to-video benchmark is documented below. Peak memory is close to this Mac's practical limit; only one inference worker runs at a time. Lower-memory tiling needs a separate quality/performance evaluation.

Portable metadata, prompt, parameters, hashes and telemetry: `benchmarks/wan-m5-max-v002.json`. Model/source licenses: downloaded checkpoint includes Apache 2.0 LICENSE and attribution NOTICE; retain them with the model. Do not assume this licenses unrelated training/reference assets.

Sources:
- https://github.com/Blaizzy/mlx-video/blob/main/mlx_video/models/wan_2/README.md
- https://huggingface.co/Anes1032/Wan2.2-TI2V-5B-mlx-q8

## Kokoro ONNX INT8 — English speech VERIFIED

`kokoro-onnx 0.6.1`; `kokoro-v1.0.int8.onnx` (114.1 MB), `voices-v1.0.bin` (28.2 MB). Downloaded from upstream `model-files-v1.1` release. Voice `af_heart`, speed 1.0, “Every great drama begins with a secret.” produced 2.542 seconds of 24 kHz audio in ~0.26 seconds of synthesis. Process startup/model loading is additional. Actual production integration generated a WAV, mixed it into a video, and validated the export.

Multiple persistent voice IDs and speed controls are supported. No arbitrary voice cloning, emotion control, or lip synchronization is claimed. ONNX adapter license: MIT; upstream Kokoro model card declares Apache 2.0 and identifies training-data attribution. Retain applicable model notices when distributing model files.

Sources:
- https://github.com/thewh1teagle/kokoro-onnx
- https://huggingface.co/hexgrad/Kokoro-82M

## Not yet verified

SDXL, multi-episode identity consistency, multiple simultaneous speakers, and local music/SFX generation remain unverified. The additional verified/experimental paths are documented below.

## Human image-to-video benchmark — VERIFIED

The primary quality test now uses an adult fictional hotel manager (“Mara Vale”), generated locally with FLUX.2 and animated from that reference through the real MCP queue.

- 704 × 1280, 121 frames, 24 fps, 40 steps, seed 42.
- Wan wall time: **1,368.26 seconds (~22.8 minutes)**; reported peak MLX memory: **55,962,717,376 bytes**.
- Identity, eyes, hands, clothing and background inspected at the beginning, midpoint and end. The source and all outputs are retained. A single clip does not establish multi-episode consistency.
- Technical validation passed; output is 5.04 seconds of actual human animation.

## FLUX.2 Klein 4B Q4 — generation and reference editing VERIFIED

Checkpoint: `Runpod/FLUX.2-klein-4B-mflux-4bit` (4.62 GB), `mflux 0.24.0`, isolated image Python environment. Model card declares Apache 2.0. Original still-image generation at 704 × 1280 / 4 steps / seed 42 took **8.39 seconds**, reporting **5.36 GB** peak MLX memory.

Durable MCP queue checks also passed:
- Hotel location, 1024 × 704: **4.49 seconds**, 5.29 GB peak.
- Reference-guided character expression revision, 704 × 1280: **9.17 seconds**, 6.34 GB peak.

Generated references remain proposals until approved. Approval cannot overwrite a newer library reference silently; rejecting an active proposal restores its prior reference. Originals are preserved.

Sources: https://github.com/mflux-community/mflux and https://huggingface.co/Runpod/FLUX.2-klein-4B-mflux-4bit

## MuseTalk MLX Q4 — technical path VERIFIED, visual quality EXPERIMENTAL

Checkpoint: `mlx-community/MuseTalk-1.5-q4` (1.51 GB), `musetalk-mlx` with a local MediaPipe face landmarker. Actual generated video + Kokoro dialogue passed through MCP to a local synchronized H.264/AAC output.

- One visible adult face, 121 frames at 24 fps.
- Second benchmark: **14.35 seconds**, **4.27 GB** peak MLX memory (first run ~19.88 seconds).
- A first take had a visible crop edge and was rejected. The revised compositor feathers to zero at crop boundaries and restores the original video after speech ends.
- Face/mouth frames visually inspected. Lower-face detail remains softer than the source; a later retained Ben take has now been scored (see the diagnostic below), without resolving visual softness. Review playback before approving a take. Multiple simultaneous speakers and large head rotations are unsupported by this adapter.
- Originals, source hashes, model request, telemetry, and rejected takes are preserved. Lip-sync output does not replace a selected shot automatically.

Source: https://github.com/xocialize/musetalk-mlx. Its code is MIT; dependency weights retain their individual licenses. Complete third-party model notice review remains required before redistributing model files.

Portable results for these checks: `benchmarks/people-m5-max-v002.json`.

## MuseTalk precision diagnostic — no sampled quality fix

The FP16 UNet (`ad54104a0129121fe2ea67471250c0656c985284`, 1.70 GB) was verified against its published SHA-256. Shared VAE/Whisper files have matching published hashes. At Ben frame 30, all four tested crop geometries still have soft mouths; FP16/Q4 mean absolute differences are 0.46–0.54 byte per color channel. This does not establish equivalence on other inputs or temporal synchronization. The configured Q4 adapter remains unchanged. See `benchmarks/ben-lipsync-precision-v002.json`.

## LTX 2.3 native image + audio — isolated experimental evaluation

Two actual local tests used the exact user-approved Ben reference and retained Kokoro line, without prompt enhancement or local text authorship. The text encoder only computes embeddings for the assistant-authored visual prompt.

- Source: `dgrauet/ltx-2-mlx`, commit `747b30ef10e92a011e85b2aa0f6c4ee86ced5218`, installed in `.venv-ltx-eval`.
- Video pack: `dgrauet/ltx-2.3-mlx-q8`, revision `69f8f11996b7b2ee54e82d0db7a5acad14b8685d`. Selected required components: 29.75 GB, hash-verified.
- Prompt encoder: `mlx-community/gemma-3-12b-it-4bit`, revision `86cc6a8dedbc456dd0e4af01a9d09f396f77e558`, 8.07 GB, hash-verified.
- Distilled audio-conditioned generation with an image anchor, 704 × 1280, 73 frames at 24 fps, 8 + 3 steps, seed 186, block streaming and a 40 GB MLX limit.
- First prompt: 83.44 seconds wall time. Immediate-speech prompt revision: 78.89 seconds. Each measured 26,756,547,622 bytes peak MLX memory (24.92 GiB).
- Full FFmpeg decode passed. Sampled frames retain recognizable identity, show head/shoulder movement and clearer coherent lower-face detail than the rejected MuseTalk sample. The immediate-speech revision changes mouth shapes earlier; gesture is limited.
- Convincing synchronization is **not established**. Subsequent independent diagnostic scoring is documented below; no multi-episode identity test or listening approval. Aligned waveform correlation of 0.99949 measures retained audio, not lip sync.
- The two isolated takes remain experimental and unapproved. A subsequent Rust/UI/MCP adapter integration is documented below; neither backend availability nor technical export grants acting-quality approval.

Evidence: `benchmarks/ltx-ben-native-a2v-v002.json`, `benchmarks/ltx-ben-immediate-a2v-v002.json`, `benchmarks/ltx-evaluation-requirements-v002.lock`. The isolated evaluation uses the default database's worker lock to prevent overlapping inference. Upstream maturity notes: https://github.com/dgrauet/ltx-2-mlx/blob/main/docs/PIPELINE_MATURITY.md.

## Native performance adapter — technical integration verified, take rejected

`generate_performance` and the dialogue studio's experimental acting control now use the same Rust service. It requires one generated speech line, a linked cast reference (or explicit MCP reference), and installed offline components. Requests retain settings, source hashes and timing. Attempt-local input copies, 8n+1 frame limits, a 40 GB MLX bound, 30-minute timeout, cancellation and manual review protect the workflow. Speech or timing changes block stale native selection/export.

The real MCP test generated 704 × 1280 / 81 frames in 90.27 seconds with 26.76 GB peak MLX memory. Queued cancellation/retry, producer disconnect/reconnect, copied-input hashes and unselected/unreviewed completion passed. Cancellation during actual denoising also passed: step 1/8 was recorded, then the worker PID exited and no asset was registered. The optional runtime is a separate environment; its core wheel matches all 94 pinned source/resource files.

**The resulting MCP take was rejected after visual review:** it holds the reference smile through approximately 1.3 seconds and begins visible articulation around 1.5 seconds, after much of the recorded line. Gesture remains limited. A valid video file is not an acceptable dialogue performance, and the adapter remains explicitly experimental.

A separate disposable MCP export test verifies native footage plus current dialogue, captions and 1080 × 1920 / 24 fps MP4 output. Measured speech RMS ratio 0.9984 and waveform cosine 0.99962 confirm one audio mix; these are **not lip-sync scores**. This fixture never changes the source take's rejected review. Evidence: `benchmarks/native-performance-mcp-v002.json`; optional pinned dependencies: `workers/requirements-performance.lock`.

## Retained-take synchronization diagnostic

The upstream SyncNet network at commit `907c0b579c2e2d83f0eae1b2ac9e720cde4e5623` reproduced its reference offset (3 frames), distance (5.352 versus 5.353) and raw confidence (10.087 versus 10.021). The official S3FD pipeline then cropped the first three seconds of each retained Ben take at 25 fps. All four first-frame face crops were visually checked. Original media hashes were unchanged.

| Retained take | Offset, frames | Raw confidence | Minimum distance |
|---|---:|---:|---:|
| Integrated native, rejected | -15 (search edge) | 1.509 | 13.030 |
| Immediate-speech native, unapproved | 4 | 0.710 | 12.114 |
| First native, unapproved | -15 (search edge) | 0.980 | 12.153 |
| MuseTalk, rejected | 0 | 5.075 | 8.427 |

These short clips provide weak native sync evidence. Raw confidence is not a probability or an acting score, and low-confidence/search-edge offsets are not reliable retiming instructions. MuseTalk's stronger synchronization result does not repair visible mouth softness. No approval changed. Calibration covers the scoring network's reference control, not a universal generated-video acceptance threshold. See `benchmarks/retained-sync-evaluation-v002.json` and the isolated environment pins. No new video generation was needed.

Evaluation drivers are preserved under `benchmarks/evaluation/`. They use the hash-verified weights, source checkout and workstation paths in ignored `benchmarks/local/` state files; they are developer diagnostics, not a consumer installer. Run calibration before scoring. Every run retains a new output directory, holds the application worker lock and leaves original media unchanged. The half-anchor driver is a controlled single-variable comparison against the retained immediate-speech take, not a production setting.

### Controlled image-anchor comparison

A single half-strength first-frame anchor test retained the immediate-speech baseline’s original image, speech, prompt, seed, resolution and steps. It took 79.40 seconds / 26.76 GB peak MLX. Five face samples per second show earlier smile release and a visible head turn, but mouth activity persists after the source speech. SyncNet raw confidence remains weak at 0.844 (offset -13 frames, minimum distance 10.535), so this is rejected as a synchronization solution. The production default is unchanged; original takes/reference/audio remain intact. This one sample does not isolate all causes or establish an acceptance threshold. See `benchmarks/ltx-ben-half-anchor-v002.json`.

## Guided dev A2V comparison — retained, not approved

One isolated comparison used the pinned public dev transformer and original distilled refinement LoRA (28.20 GB additional weights, verified SHA-256), sharing the already verified pack components. The backend used 30 guided stage-one steps (video CFG 3, STG 1, rescale 0.7, modality scale 3) and three refinement steps. Audio remains frozen in the installed implementation. This is a different transformer/schedule/guidance path, not a CFG-only ablation.

With the same approved Ben image, source line, prompt, seed, dimensions and frame count as the immediate-speech distilled baseline, it took 446.23 seconds and 28,064,417,158 bytes peak MLX. Sampled frames show changing eyebrows/mouth, a head turn and late arm lift; the moving hand leaves the frame. Some transient face softness is visible. No seam is apparent in those samples.

Calibrated SyncNet on the official face crop returned raw confidence 2.642, offset -10 frames at 25 fps, minimum embedding distance 10.867 (matched distilled baseline: confidence 0.710, offset 4). These short-clip diagnostics do not establish convincing synchronization, and no retiming or approval was applied. Full playback/listening and multi-episode identity acceptance remain absent. Production configuration is unchanged. Non-inference FFmpeg tests ran concurrently, so wall time is not an isolated speed comparison. Evidence: `benchmarks/ltx-ben-guided-v002.json`, `benchmarks/ltx-guided-components-v002.json`.

Native speech conditioning audit: `benchmarks/native-audio-conditioning-v002.json` compares the installed MLX preprocessor to the pinned [official Lightricks AudioProcessor](https://github.com/Lightricks/LTX-2/blob/2362161611a61154d342e02724fb8fe58efd455d/packages/ltx-core/src/ltx_core/model/audio_vae/ops.py) on the exact retained Leah conditioning WAV. Mean/max absolute log-mel error is 0.0006911/0.0012398. Actual audio tokens differ from silence and remain frozen in both pipeline hooks. This narrows the investigation; it does not establish full encoder parity, effective cross-modal attention or synchronized acting. The adapter now records PCM signal levels and sample count in `conditioned-audio-audit.json`, and rejects digitally silent windows before loading MLX. Signal presence is not voice activity detection. Reproduce with `.venv-ltx-eval/bin/python benchmarks/evaluation/check-native-audio-conditioning.py`; requires installed evaluation environments, the pinned local checkpoint and retained Leah audio. No video render is performed.
