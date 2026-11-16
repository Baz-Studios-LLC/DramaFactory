# DramaFactory · v003

A Rust-native, Mac-first studio for developing a coherent short drama series, from synopsis and cast to generated episodes. MIT licensed. **v003 is a work in progress, not a finished production release.**

## Workflow

1. **Synopsis** — brainstorm with a shared creative sidekick and refine the series premise.
2. **Characters & world** — develop character briefs, locations and reusable portrait references.
3. **Overall plot** — plan character arcs, continuity and the series ending.
4. **Episode plot** — develop each episode and its screenplay.
5. **Episode generation** — prepare segments, review takes and assemble an MP4 with its soundtrack.

The current series applies across every workspace. Use **+ New series** under the series selector to begin a different story. Editing a synopsis changes the existing project; it does not replace its cast or episodes.

## AI services and current limitations

The current rewrite uses **OpenRouter** for AI services: Gemini 4 Argon for the shared sidekick, Grok Imagine Image 2.0 for portraits, and Seedance 2.5 for video with native audio. Settings stores the OpenRouter key in macOS Keychain. Sending messages or generating media uses your paid provider account.

Sidekick conversation persists per series. Synopsis, character briefs and portrait prompts support guarded unsaved draft proposals and Undo; editing every document through the sidekick remains unfinished. Local portrait transport to hosted video, provider capability checks and portions of the production interface remain incomplete. Legacy local-inference services are being removed; local inference execution is no longer supported. Historical benchmark reports describe earlier experiments, not current supported features.

Hosted image/video transport has fixture-based integration coverage. Local episode assembly has been exercised through the real MCP server and FFmpeg, preserving source picture order and audio. This does not establish live provider acting, identity or dialogue quality. No completed generated series is claimed.

## Run on macOS

Install Rust and Xcode Command Line Tools, then double-click **Launch DramaFactory.command**. It builds changed sources and opens the app. Quit and reopen an already-running app to load rebuilt code.

```sh
cargo run
./scripts/bundle-macos.sh
open target/DramaFactory.app
```

The bundle uses the original Icon Composer branding. It is locally ad-hoc signed, **not Developer ID signed or notarized**. Public versions increment in whole numbers: v001, v002, v003. Cargo semver and macOS bundle numbers are internal packaging metadata.

Project databases, models and generated media live outside the repository in the platform application data directory. `cargo run -- --data-path` prints the database path; `DRAMAFACTORY_DATABASE` selects an isolated database. Preserve existing media and projects during migration.

## Development and MCP

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
python3 scripts/test-mcp.py
cargo run -- --mcp
cargo run -- --worker
```

The MCP server uses stdio and shares application services with the desktop. The worker drains queued jobs then exits. FFmpeg is required for local assembly; historical integration scripts may also require dependencies from the retired local workflow. See `DESIGN.md`, `ROADMAP.md` and `benchmarks/` for design, evidence and open work.

Signed update verification and staging exist, with an external private signing key. The current updater targets the developer bundle; complete signed/notarized bundle updates are required before production distribution. A GitHub source update does not publish an app release or an update artifact.

Portraits support per-character Photo, Cinematic, Animation and Illustration styles. The chosen style is saved and captured in each generation request; existing images remain unchanged. Style prompts guide the provider but do not guarantee photographic realism.
