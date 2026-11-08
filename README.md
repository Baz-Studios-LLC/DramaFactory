# DramaFactory · v001

A local-first drama production studio, built in Rust. Mac first, portable services, MIT licensed.

The first foundation includes a desktop studio, persistent series and episode development, screenplay editing, render request storage and a stdio MCP interface. Video inference and automatic updates are **not operational yet**. Render requests explicitly remain blocked until a verified worker is integrated.

## Run
Install Rust and Xcode Command Line Tools, then:

```sh
cargo run
cargo test
./scripts/bundle-macos.sh
open target/DramaFactory.app
```

No Node or web runtime is required. The local app bundle is unsigned and unnotarized. An Apple developer account is not needed to build locally; distribution signing will follow later.

## MCP
Build with `cargo build --release`. Configure an MCP client to launch the absolute path to `target/release/dramafactory` with argument `--mcp`. The desktop and MCP use the same platform data directory and SQLite database. No network listener is opened.

Available tools: `dramafactory_create_series`, `dramafactory_list_series`, `dramafactory_create_episode`, `dramafactory_list_episodes`, `dramafactory_save_screenplay`, `dramafactory_queue_render`, `dramafactory_list_jobs`, `dramafactory_cancel_job`.

The current protocol implementation is deliberately small; maintained SDK adoption and full lifecycle validation remain on the roadmap.

## Releases and updates
Public release labels are v001, v002, v003, incrementing by whole numbers. Internal Cargo semver is not the user-facing version. GitHub home: https://github.com/Baz-Studios-LLC/DramaFactory.

Planned update flow: background GitHub release checks, signature verification using a bundled public key, staged downloads, install after exit with no active render, and recovery on failed replacement. macOS Developer ID/notarization is separate from cryptographic update signing. Background release discovery and a manual check button are implemented. Silent installation remains disabled until a signature verifier and release packaging are implemented. Never publish signing private keys.

See ROADMAP.md and MODEL_COMPATIBILITY.md for remaining work and verified capabilities.
