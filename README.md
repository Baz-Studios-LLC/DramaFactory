# DramaFactory · v008

A Rust-native, Mac-first studio for developing a coherent short drama series, from synopsis and cast to generated episodes. MIT licensed. **v008 is a work in progress, not a finished production release.**

## New in v008

- Portable per-series project folders for iCloud Drive, including media, chat and edits.
- Signed Mac downloads and quiet update checks at launch and every six hours; installation waits for app exit.
- Release bundles use an explicit file allowlist and are scanned for credentials and personal home-folder paths.

Pushes to `main` run the Mac release workflow. Successful builds receive the next whole-number public version and publish a download plus an Ed25519-signed update. CI injects the public build label; Cargo's version is the source-development baseline. The private update key is a GitHub Actions secret, never a source file or app resource. Failed builds do not publish an update.

The app keeps API credentials in macOS Keychain and project data outside its bundle. Only the public update-verification key is embedded. Pattern-based release scans are an additional safeguard, not a proof that arbitrary content contains no private information. Historical git commits may retain old benchmark paths; current reports use a generic username.

Mac downloads are currently Apple Silicon, ad-hoc signed and not notarized. See [installation notes](docs/mac-release-notes.md).

## Previous v007 changes

- Video model selection, full-request prompt counts and Wan prompt-length validation.
- Timeline clip sliding with gaps, reliable timeline transport, and clip context menus for splitting, duplication, removal, volume and audio fades.
- Consistent production activity cards with timestamps and clearer controls.
- Sidekick guidance for motivated camera coverage and dialogue/ambience without background music in new generation requests.

## Workflow

1. **Synopsis** — brainstorm with a shared creative sidekick and refine the series premise.
2. **Characters & world** — develop character briefs, locations and reusable portrait references.
3. **Overall plot** — plan character arcs, continuity and the series ending.
4. **Episode plot** — develop each episode and its screenplay.
5. **Episode generation** — prepare segments and generate multiple takes.
6. **Editing room** — arrange clips, trim and split with linked audio, scrub source frames, preview the timeline and export an MP4.

The current series applies across every workspace. Use **+ New series** under the series selector to begin a different story. Editing a synopsis changes the existing project; it does not replace its cast or episodes.

## AI services and current limitations

The current rewrite uses **OpenRouter** for AI services: Gemini 4 Argon for the shared sidekick, Grok Imagine Image 2.0 for portraits, and Wan 3.0 by default for video with native audio, with Seedance and Google Veo alternatives. Sidekick can alternatively use **OpenAI direct**: in Settings → Sidekick, select OpenAI direct, save your OpenAI API key, use GPT-6 Astra and choose its reasoning effort (Medium by default), then Save Sidekick settings. OpenAI is preferred when a key exists and no connection preference has been saved. Saving an OpenAI key also activates it. Direct OpenAI uses the Responses API. The next message uses that connection, including existing history and on-demand portraits. WAN and image generation continue through OpenRouter. Both keys are stored separately in macOS Keychain; connection preferences stay local to each Mac. Test connection only checks authentication, not generation or model access. Sending messages or generating media uses your paid provider account.

Sidekick conversation persists per series. Synopsis, character briefs and portrait prompts support guarded unsaved draft proposals and Undo; editing every document through the sidekick remains unfinished. Local portrait attachments are implemented using embedded image data, but acceptance through the live OpenRouter video route remains unverified. Portions of the production interface remain incomplete. Legacy local-inference services are being removed; local inference execution is no longer supported. Historical benchmark reports describe earlier experiments, not current supported features.

Hosted image/video transport has fixture-based integration coverage. Local episode assembly has been exercised through the real MCP server and FFmpeg, preserving source picture order and audio. This does not establish live provider acting, identity or dialogue quality. No completed generated series is claimed.

Character cards include a **Character profile** form with optional physical traits, personality, background, relationships, and episode-specific continuity. **Full character brief** previews the combined text; **Save profile** stays visible when the card sections are collapsed and saves the form and production brief together. Existing biographies stay in Story notes, and approved portraits stay selected. Sidekick profile edits remain drafts with Undo. Image generation uses the entire saved profile alongside additional portrait direction and the selected visual style. Save form changes first; **Full prompt for next image** shows the combined request. Choose **Photorealistic**, **Cinematic**, **3D / CGI**, **Animated film**, **Cartoon**, **Cel-shaded**, or **Illustrated** on **Synopsis**. The series style is automatically used by every character for the next image; existing portraits remain unchanged.

Use **Delete character…** on a card to remove it from the active cast. Confirm deletion, then restore it anytime from **Deleted characters** at the bottom of the cast page. Original images and saved details are retained. Saved story/shot/generation-plan links and active generation jobs must be resolved before deletion.

## Projects in iCloud Drive

Use **Project… → Save project as…** below Save project to choose a new `.dramafactory` folder in iCloud Drive (or any normal folder). Each folder contains one series, its media and immutable saved revisions. Subsequent **Save project** actions update that folder. Finish generation jobs before saving a portable snapshot, and wait for Finder to finish syncing before switching Macs.

On another Mac, use **Project… → Open project…** and select the project folder. The app verifies and copies its media into local application storage; it never edits a live database inside iCloud. The local working copy is remembered on restart. Open the folder again to load a newer save made on another Mac. Conflicting saves are preserved and reported rather than silently overwritten. Do not edit the same series on both Macs simultaneously. iCloud synchronization itself is handled by macOS and has not been tested across two physical Macs.

OpenRouter credentials and machine configuration are not transferred. Set up the key and local FFmpeg video tools on each Mac. **Local projects** returns to the original local library. Your original projects and media are preserved.

## Run on macOS

Install Rust and Xcode Command Line Tools, then double-click **Launch DramaFactory.app** in the repository folder. Keep this launcher in the repository; you can make a Finder alias to it elsewhere. It builds changed sources in the background and opens the app without a Terminal window. Build output goes to `~/Library/Logs/DramaFactory/launcher.log`; failures show a dialog. Quit and reopen an already-running app to load rebuilt code.

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

Sidekick latency diagnostics are saved under `diagnostics/sidekick/` beside the application database. Each normal request records elapsed credential, HTTP-header and full-response times, request size, provider generation ID and reported token counts (including reasoning/cache counts when supplied). These files contain no prompts, replies or credentials. Header time is not time to first generated token; the non-streaming request waits for the complete JSON reply. Character conflict snapshots remain local instead of being repeated in the model context.

Overall plot separates Story & ending, Acts & storylines, searchable Episode outline, and Continuity & canon. Acts use editable episode ranges; ongoing storylines are separate from episode-linked setups/payoffs. Sidekick plot patches update the current series draft with before/after section review, guarded Undo and explicit Save plot. Draft or saved-revision changes during a request reject the proposal instead of overwriting newer work. Episode outline edits require existing episode IDs; act ranges do not create episodes or screenplays. Develop plot with sidekick prepares a message for you to send, without starting generation automatically.

Episode plot now contains Story beats, Screenplay and Continuity workspaces, with searchable episode browsing, previous/next navigation and the planned handoff from adjacent episodes. Save episode atomically saves its outline and screenplay, preserving unrelated unsaved Overall plot edits. Reload preserves the current episode and shared story draft in a recovery file. Sidekick edits target one exact episode snapshot, have before/after review and guarded Undo, and never save or generate media automatically. Prepare generation saves the episode and opens the hosted segment planner. Word counts and planned handoffs do not certify runtime, performed dialogue or reviewed canon.

Save project replaces Studio overview in the sidebar; the overview is available in Settings. It saves pending current-series synopsis, profiles, location briefs, portrait prompts, shared story outlines, screenplay, generation plans, chat drafts and supported editing controls through their existing conflict checks. It preserves a recovery snapshot before saving, reports failures without discarding drafts, and never queues generation. Unfinished creation forms and legacy shot forms without safe save baselines are retained as recovery drafts and explicitly reported rather than silently committed.

Sidekick can create and fill one episode by default, or a requested batch of up to five, from Episode plot or Overall plot. The app assigns IDs, validates complete beats and screenplay, rejects duplicate titles/stale plot revisions, and atomically saves new episodes and outline links. Request receipts prevent duplicate replay. Existing episode edits still remain unsaved drafts with Undo. Creating episodes never generates video.

Episode generation is organized into Prepare, Takes, and Finished episode. Prepare checks the live OpenRouter catalog for Seedance 2.5 durations and resolutions (currently up to 1080p), supports ordered segments and separate reference/frame modes, and requires saved edits before generation. Takes provides original playback, approval/rejection and selection; Finished episode assembles selected, approved takes with their original sound. Automatic ending-frame handoff, audio/video reference inputs, and segment trims remain incomplete. No paid generation was used to verify this UI update.

The Sidekick can prepare the selected episode’s complete Seedance segment plan from screenplay and series context in either Visual plan or Episode generation. These destinations share one draft, with guarded application and Undo. Generate episode saves and atomically queues missing segment requests; identical existing requests are never automatically retried. Preparation alone does not generate video. Automatic frame handoff still requires integration. Selected character portraits are embedded directly at submission and paired with numbered names and saved profile briefs in the prompt. Exact image hashes are checked before submission. This path is locally tested; no paid request has yet verified OpenRouter route acceptance.

Sidekick accepts up to four pasted images per message: use Command-V while the composer is focused or click Paste image. Thumbnails can be removed before Send. Images are stored with the per-series draft and conversation, and sent as image content only when you submit the message. Clipboard images are resized to fit 2048 pixels and capped at 4 MiB each. Assistant replies render Markdown.
