# DramaFactory

Rust-native, Mac-first creative production application. Visual quality is a primary product requirement. Keep the UI elegant, spacious, dark, and restrained. Never fabricate production activity or hardware utilization.

## Architecture
- `src/main.rs`: eframe/egui desktop interface; portable Rust UI, no web runtime.
- `src/store.rs`: SQLite persistence and shared application services.
- `src/mcp.rs`: newline-delimited JSON-RPC stdio MCP entry point. No stdout diagnostics.
- Database uses the platform application data directory via `directories`.
- Inference must run in isolated worker processes, never the UI thread.

## Development
`cargo run` launches the desktop. `cargo run -- --mcp` starts MCP. Run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` before finishing changes. `scripts/bundle-macos.sh` builds a local macOS app bundle.

## Product rules
- Public versions are whole-number identifiers: v001, v002, v003. Cargo's internal semver is packaging metadata only. macOS build numbers are integers.
- Repository: bazstudios/DramaFactory. Public source, MIT license.
- macOS initially; keep services platform independent for future Windows/Linux.
- Silent updates must use signed release artifacts, verify before installation, and defer replacement while rendering. Never trust an unsigned download or execute a command obtained from a feed.
- No Apple developer account currently. Do not claim notarization. Developer ID signing will be added when available.
- Ask before large model downloads, paid inference, significant data deletion, or irreversible system changes.
- No verified local video generation yet. Do not imply otherwise. Block render requests explicitly until a real adapter is operational.
- Preserve original assets. Do not commit private keys, credentials, model files, or project databases.
