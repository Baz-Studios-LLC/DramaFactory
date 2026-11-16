# Roadmap

## v001 — Initial foundation
Rust desktop, SQLite projects and screenplays, initial stdio MCP, stored render requests, release discovery. Published source exists; no production release was made.

## v002 — Local development, not pushed
- [x] Original icon and wordmark integrated; native Mac app icon
- [x] Maintained Rust MCP SDK and shared GUI/MCP services
- [x] v001 database migration and stale-editor conflict protection
- [x] Editable/reorderable episode shot lists and take selection
- [x] Non-destructive picture in-points and duration edits with expected-range/take conflict checks; later source frames verified in actual MP4 output
- [x] Render completion preserves intervening editorial choices and late cancellation; outputs remain available for review
- [x] Series casting/style profile, slice-of-life direction, editable 1–4s cut guide and episode target
- [x] Stable shot cast/location links, reusable camera geometry, wardrobe/acting/story-beat notes and shared reference-guided render preparation
- [ ] Visual verification of the new directing controls (Mac locked during the latest attempt)
- [x] Cast and location libraries with managed reference images and conflict-protected name/brief editing through UI/MCP
- [x] Character voice assignments and individual dialogue regeneration
- [x] Model-independent dialogue drafts, optimistic edits and stale speech-output protection; pending speech survives timing-only edits
- [x] Audio-only episode dialogue rehearsal export through GUI/MCP and worker, before video production
- [x] Shared UI/MCP read-only episode production checklist, missing coverage/speech, reference planning warnings and authoritative export-request checks; no automatic approvals
- [x] Resumable episode speech production: atomic queueing, concurrent-producer convergence, pending job reuse, cancellation retry and selective take repair
- [x] Queue-time reference hashes and verified attempt-local conditioning copies for Wan/images; stale image-brief/reference approvals reject without changing selections
- [x] Isolated local Wan worker; real progress, cancellation, retry, durable attempts
- [x] OS queue ownership lock and atomic claims; worker survives GUI/client exit
- [x] Shared cancellation-aware completion, retained late outputs without speech selection, clean retry active-output snapshot and cancellation-aware interrupted-row recovery; actual FFmpeg/Kokoro integration verified
- [x] Real Wan 2.2 TI2V-5B text-to-video benchmark on the M5 Max
- [x] Local Kokoro speech generation and WAV storage
- [x] Asset hashes, provenance, posters, review decisions, versioned editorial notes/history, in-app video preview
- [x] Shared paginated asset search and media/review filters; queue/shot links to retained takes and saved rejection notes (new UI visual QA pending)
- [x] Ordered timeline assembly, exact shot trims, portrait H.264/AAC MP4 export
- [x] Dialogue mixing and duration/resolution/full-decode validation
- [x] Actual five-second generated human test production with dialogue and experimental lip sync, through MCP
- [x] Signed update staging and deferred installation, tested in disposable bundle
- [x] Core, MCP, media-production, and updater integration checks
- [x] Native Icon Composer document, Apple-compiled Assets.car + ICNS; raw macOS icon override removed
- [x] Desktop visual QA, thumbnail loader fix, preview audio and card layout refinement
- [x] Bounded latest-frame preview, paced real decode, immediate child cleanup, timeout/end/error states and system-player action (recent UI visual QA pending; separate A/V clocks remain)
- [x] Human reference-guided video benchmark and sampled identity/face/hand review
- [ ] Multi-episode identity consistency evaluation
- [x] Local FLUX.2 images and reference-guided edits, through GUI/MCP durable queue
- [x] Local MuseTalk MLX technical lip-sync path, through GUI/MCP
- [x] Experimental native LTX image/audio acting through UI/MCP/durable worker; real generation, retry, reconnect, input provenance and denoising cancellation verified
- [x] Native bounded speech-window and same-primary continuation requests through UI/MCP, exact sample preparation, once-only final audio routing and stale-window checks; real generated windows remain visually rejected
- [x] Native-performance speech/timing selection and export guards; actual final-export fixture verifies one speech mix and captions
- [x] Calibrated upstream SyncNet diagnostic and official face crops on four retained Ben takes; weak native results, no acting approval
- [ ] Lip-sync visual quality refinement and reliable synchronized human acting
- [x] Imported WAV music/SFX cues, source trims, timing, gain and fades through GUI/MCP; actual mix measured in integration tests
- [x] Explicit local sound cue drafts, expected-state save/removal conflicts, idempotent retries, deleted-update protection and exit snapshots; new controls need visual QA
- [x] Shared measured dialogue timing diagnostics: unknown timings, overlaps, cut/episode boundaries and dialogue-free gaps, with actual speech integration checks
- [x] Move dialogue between coverage shots through UI/MCP while preserving selected or pending speech, direction and absolute timing; conflict/cross-episode/unknown timing gates
- [x] On/off-screen dialogue direction through UI/MCP; phone/listener coverage protected from inappropriate native acting and shot-linked lip sync
- [x] Dialogue bridges across camera cuts, SRT/ASS sidecars and optional burned captions; real audio and caption-frame integration checks
- [x] Shot picture fades from/to black through UI/MCP, with unchanged episode/audio timing, actual rendered-frame tests and dependent canon checks
- [ ] Overlapping dissolves and richer timeline editing; caption chunk timing is estimated within measured speech, not word-aligned
- [x] Shared model required-file diagnostics, missing paths and explicit runtime/quality limits through UI/MCP
- [x] Explicit background UI/MCP import and executable diagnostics with bounded subprocesses and per-dependency errors; all five installed environments checked (new panel visual QA pending)
- [ ] Dependency/model installation and runtime checks suitable for a fresh consumer installation
- [ ] Complete signed app-bundle update packaging for future notarized distribution

## Core product completion — required by the clarified goal

- [x] Revisioned series bible editor, planned ending, cast-linked character arcs and setup/payoff map through GUI/MCP
- [ ] End-to-end assistant-led idea-to-series creative development through MCP, as requested; plots/scripts are authored by the assistant
- [ ] Reliable MCP production orchestration from assistant-authored plans through completed episodes
- [x] Ordered episode outline editor with goal/conflict/turn/payoff/hook and declared fact transitions
- [ ] Editorial quality evaluation of generated outlines and finale resolution
- [x] The Other Chair: 67 assistant-authored outlines saved through real MCP, original milestone IDs retained, six screenplay drafts, a measured 78.5s pilot audio rehearsal and revised 65s episode-2 rehearsal (neither is a finished episode)
- [x] Persistent declared-fact ledger supporting knowledge, relationships, secrets, props, wardrobe, location and story time; planned projections separate from committed canon
- [ ] Automatic extraction and semantic/visual continuity review against scripts and footage
- [x] Revision history, stale-draft rejection, reviewed final-cut commits and dependent canon invalidation on plan/script/timeline/cast/source changes
- [ ] Regeneration orchestration for affected downstream production work
- [ ] Resumable idea-to-finished-episode orchestration, followed by a coherent next episode with the same identities
- [ ] Full multi-episode story and media acceptance review, including setup/payoff and finale resolution

These remain part of the active product goal. A version label or completed media pipeline does not substitute for them. Research and evidence limits: `benchmarks/short-drama-research-2026-11-15.json`.

## v003 — Earlier sequencing proposal (subject to core goal)
Dependency graph, continuity validation, richer creative documents, full script-to-shot orchestration, model scheduling, complete 75-second drama validation.

## v004 — Advanced production
Advanced timeline, richer asset browser, batch episodes, additional inference backends and export profiles.

Do not push or publish without the user's explicit instruction. Whole-number release labels only. v002 is a functional local production path, not a claim that every planned feature is finished.

Latest visual review: Mac unlocked, current bundle reopened, screening-room prompt overflow fixed and compact cards visually verified. Runtime panel observed. Other recent controls and search/runtime interactions still need review; automation yielded to concurrent user changes. Evidence: `benchmarks/visual-review-resumed-v002.json`.

## Requested usability overhaul — in progress
- [x] Five-stage guide and grouped navigation, actual Mac inspection
- [x] One remembered series context, per-series episode memory and safe screenplay switches
- [x] Searchable story-ordered episode browser and screenplay pane; actual pilot opened
- [x] Large reference sizing and full-size inspector; actual cast image review
- [ ] Complete episode search/navigation/zoom and smaller-window interaction review
- [x] Shot planner first layout pass: searchable list, focused inspector, large linked references and current-shot takes; actual Mac review
- [ ] Complete shot edit/search/take interaction review, expected-state shot edit safety and draft recovery UI
- [x] Dialogue first layout pass: episode-wide searchable lines, focused words/voice editor, prominent Generate/Listen; actual Mac search/selection/new-form review
- [ ] Story bible/arc redesign and remaining dialogue save/voice/listening/small-window interaction review
- [ ] Generation/review and timeline/sound page redesign
- [ ] Preferences/setup and end-to-end first-use review
First-pass evidence: `benchmarks/usability-overhaul-pass-01-v002.json`. This does not mark the complete app overhaul finished.

Dialogue first-pass evidence: `benchmarks/usability-overhaul-dialogue-v002.json`. No production speech or take selections were changed during visual review.

Dialogue follow-up: unfinished new-line forms are remembered separately per episode during the session. Actual Mac Next/Previous isolation and restoration verified using an unsaved marker, then cleared. Exit/switch snapshots remain available on disk; restart recovery UI is still incomplete.

Joint audio/video candidate: retained Ben distilled LTX 2.3 sample shows clear body/hand motion in sampled frames; facial softness remains. Calibrated SyncNet on first 3s gives zero offset / raw confidence 4.052, actor crop verified. No transcript/voice or full audiovisual approval; no production integration or selection. Evidence: `benchmarks/ltx-ben-joint-v002.json`.

## Latest direction — hosted production pivot
The user requested a much simpler app powered by OpenRouter, with Seedance generating picture and sound together and in-app image creation likely using Grok Imagine. This supersedes further local-only inference development/evaluation. Existing assets and adapters remain preserved during migration.

Target flow: Series → Cast & world → Episode → Generate → Review & export. Keep capable writing/continuity and references; remove separate speech/lip-sync/model-install setup from the primary path. Episode generation must respect actual provider duration limits, plan multiple connected segments where needed, and retain soundtrack/voice continuity for review.

Verified 2026-11-16: public OpenRouter videos/models lists bytedance/seedance-2.5, native audio, durations 4–30s, 480p/720p, 9:16 among supported ratios. Official OpenRouter model page lists x-ai/grok-imagine-image-2.0 for image generation/editing; general chat model discovery alone did not return it in our query, so implement the documented image-specific discovery/route rather than infer absence. No paid generations performed.

Remaining: provider settings and secure key storage, live capability discovery, durable submit/poll/download with ambiguous-submit protection, cost visibility, remote generation UI/MCP, image reference editing, episode segment assembly preserving generated audio, simplify navigation and migrate existing projects without data loss. Hosted production is not implemented yet.

OpenRouter setup first pass: masked key input and native Keychain save implemented at top of Settings; local worker path controls removed from this page. Actual packaged Mac layout verified; empty Save disabled. No real key entered, no Keychain write exercised during visual QA, no connection/generation request made. 30 unit tests, Clippy, real MCP and strict bundle signature passed. Credentials read/status/remove and authenticated remote jobs remain pending.

Rewrite design is now specified in `DESIGN.md`: Synopsis → Characters & world → Overall plot → Episode plot → Episode generation. Includes explicit Seedance reference-vs-frame modes, native audio, segment limits, cost/recovery rules and page acceptance. Existing five-stage sidebar is interim; page implementation remains pending. Local inference executors/adapters and setup script removed; 28 retained unit tests/Clippy/MCP passed after deletion, packaged navigation observed. Remaining old local authoring/service controls and diagnostic/setup dependencies must still be removed.

OpenRouter discovery implemented: bounded read-only public video/image catalogs shared by background Settings refresh and MCP. Real isolated stdio call found Seedance 2.5 and Grok Imagine Image 2.0, including current capabilities; per-endpoint limits remain to validate. 29 tests/Clippy/MCP/bundle signature passed. Packaged catalog-panel interaction deferred because app was in active media-review workspace. Evidence: `benchmarks/openrouter-discovery-v002.json`.

Dock icon regression diagnosed from eframe 0.33.3 source: an absent viewport icon installs the eframe default. macOS now passes explicit empty IconData, which AppTitleIconSetter treats as no runtime override. Bundle still uses actool-compiled Icon Composer assets. 29 tests/Clippy/rebuild/strict signature passed; corrected bundle reopened. Dock-specific CUA inspection timed out, so visual confirmation of Dock rendering remains unproven.

Five-step primary navigation implemented with related tools inside workspaces. Dedicated Synopsis editor supports per-series drafts, exit snapshots, atomic expected-text conflict rejection and idempotent saves through UI/MCP. Actual Mac sidebar and saved Other Chair synopsis layout observed; removed unsupported arrow glyph found during screenshot review. 30 unit tests/Clippy/build/strict signature passed; existing MCP integration passed before glyph-only change. AI authoring and cloud generation remain pending; synopsis currently contains title/premise, with richer format/tone fields still to integrate.

OpenRouter credential lifecycle: native Keychain save/read/remove isolated in `src/credentials.rs`; Settings background status and read-only authentication check added. Fixed HTTPS endpoint, 15s bound, redirects disabled, 64KiB response cap and sanitized messages. Actual packaged Mac Check saved key and Test connection correctly report missing key without network generation. No real key was supplied; authenticated success/save/remove remain unverified. 30 tests/Clippy/MCP/rebuild/strict signature passed.

Hosted video request planner implemented in UI/MCP. Removed local render form from generation page. Typed exclusive prompt/reference/frame modes, HTTPS media URL validation, live duration/resolution/ratio/audio/frame-role checks and inspectable request snapshot. Fixed preview profile currently 10s/720p/9:16/audio. Real isolated MCP verified live Seedance first+last payload and 90s rejection; no submission. 31 tests/Clippy/MCP/rebuilt strict signature passed. New planner UI visual interaction remains pending; local uploads, persisted per-episode plans, endpoint media limits, editable profile and durable paid jobs incomplete. Reference count cap12 is an explicit planner bound, not a provider guarantee.

Generation drafts now persist per episode with ordered segments, revision-guarded/idempotent UI/MCP saves and exit snapshots. 32 tests/Clippy/build passed; real stdio save/read/stale-conflict passed. UI interaction/restart review pending. User priority added: Synopsis discussion pane with model editing the main draft; design recorded, implementation pending. Research shortlist/default Gemini 4 Argon, alternative Opus 5.5, based on current creative-writing Arena and confirmed live OpenRouter availability, no paid tests.

Shared sidekick first implementation: append-only per-series messages, idempotent IDs, restart persistence, common right panel on every page, current workspace and project/cast/plot/open-draft context, Gemini4Argon through fixed OpenRouter chat route (explicit user Send only). 33 tests/Clippy/MCP/build passed; actual Mac Synopsis→Characters preserves sidekick and updates context. No paid chat sent. Direct document edits/undo, portrait generation, durable in-flight reply recovery, unsent composer restart recovery, conversation pagination/summarization and model choice remain unfinished. Current UI sends latest200 messages, with 1MiB aggregate bound; do not claim unlimited conversation memory.

Sidekick Synopsis proposal application implemented with strict structured reply parsing, exact draft snapshot guards, unsaved-only updates and guarded Undo. Other document/portrait tools still pending. Real MCP returned-draft/conflict checks and 34 tests/Clippy/build/strict signature passed. No paid model call, live JSON adherence or generated-edit UI interaction claimed. Evidence: `benchmarks/sidekick-synopsis-edits-v002.json`.

Grok portrait first path implemented: character-card prompt and explicit paid generation, OpenRouter image job kind/worker, bounded native HTTPS request, raw response retained, base64/format/dimension/full-decode validation, original-byte asset registration and manual selection using existing stale-reference guards. No attempted hosted job replay through Retry; provider billing may be uncertain. 35 tests/Clippy/MCP/build/strict signature passed. Actual Mac prompt/paid control inspected without generation; found shared collapsing ID and fixed per character (final fix not rechecked visually). No paid API result, endpoint capability preflight, reference-guided editing, cost estimate, sidekick prompt edits, prompt restart persistence or remote error/cancel integration acceptance yet.

Grok image loopback integration passed through actual native HTTP and SQLite: successful original-byte retention/manual selection, late-canceled output registration, malformed image/redirect/auth failures and attempted-job retry rejection. Fixture key only, no Keychain access or paid requests. 36 tests/Clippy/MCP/rebuilt strict signature passed. Live endpoint inspected read-only: xai route advertises medium/1K/3:4, up to3 references; medium_1k output pricing observed $0.06 at inspection, not an implemented quote/preflight. Evidence: `benchmarks/grok-image-http-v002.json`.

Sidekick character/portrait-prompt draft edits implemented, with exact request snapshots, per-series target guards, unsaved baseline preservation and guarded Undo. Character card displays unsaved draft text. Portrait prompts now save/reload persistently through UI/MCP with stale-text conflicts and exit snapshots. 38 tests/Clippy/build/signature and real MCP prompt roundtrip/conflict passed. Live model-generated apply/Undo and new GUI controls unverified; no paid requests. Evidence: `benchmarks/sidekick-character-edits-v002.json`.

Episode segment cast binding implemented: choose same-series characters, retain exact selected portrait hash/path/name, persist via saved-plan services, reject duplicate/cross-series/stale references. Frame mode preserves but deactivates cast guidance. Local-reference preview blocks until upload transport is implemented; never silently drops portraits. 39 tests/Clippy/build/signature and real MCP reference-bind/save passed using explicit disposable fixture. GUI picker interaction and actual provider transport remain unverified. Evidence: `benchmarks/episode-cast-references-v002.json`.

Seedance remote job foundation implemented: atomic submit gate, durable remote ID, accepted-job monitoring/download resumption, ambiguous submission replay rejection, fixed-origin bounded native HTTP, retained downloaded media and cancellation-aware unreviewed asset registration. 40 tests/Clippy/MCP/build/signature passed; only state/retry persistence tested, not full video API execution. Paid MCP tool exists but was not invoked. UI submission, exact episode/segment result binding, mock video API acceptance and local-reference transport pending. Evidence: `benchmarks/seedance-remote-jobs-v002.json`.

Queue timing implemented: recorded worker-attempt start/end, real elapsed time while running, queued waiting time when creation metadata exists, frozen completed timing and unknown interrupted/legacy timing. Hosted queue cards show spinner/stage and explicit unknown ETA instead of unsupported percentage. Seedance stages include submitting/provider queued/generating/downloading/validating; Grok includes generating/validating. Shared get_job_timing MCP. 41 tests/Clippy/MCP/build/signature passed. Running-job GUI timing and provider ETA/history estimation remain unverified/unimplemented; no paid generation was used.

Seedance loopback HTTP/real FFmpeg integration passed: once-only POST, persisted ID, in_progress/completed polls, fixed-origin content download ignoring untrusted response URLs, validated original-byte registration, and accepted-ID resume with GET only/no POST. Added requested duration/framing/minimum-resolution/audio validation after finding insufficient checks. 42 tests/Clippy/MCP/build/signature passed; actual optional FFmpeg test explicitly enabled and passed. Tone/color fixture is plumbing evidence, not an acting test. Paid live generation, upload transport and full GUI episode path still incomplete. Evidence: `benchmarks/seedance-http-v002.json`.

Saved segment job binding implemented: stable segment IDs (deterministic legacy fallback), exact saved snapshot/revision checks, transactional episode/segment/job mapping, unresolved-attempt reuse and explicit paid GUI/MCP generation. Per-segment job/result summaries shown. 43 tests/Clippy/MCP/build/signature passed. Actual Mac opened generation page, inspected disabled empty submit, selected Ben and observed portrait/link/blocking explanation, then reloaded without saving or generating. Global old job list still clutters this workspace and must be removed from primary view. No paid generation or actual segment result selection/assembly yet.

Sidekick composer updated: Enter sends, Shift+Enter inserts newline, focused-composer-only event handling, held-key repeat ignored, button/keyboard share the same nonempty/no-pending guard. Build/Clippy/43 existing tests/MCP/signature passed; no paid send test or forced restart of user's pending conversation. Segment take-selection/hosted assembly services and UI are also drafted in this worktree but still need actual FFmpeg integration tests before acceptance.

Hosted episode assembly integration now passes through actual MCP/FFmpeg: two source clips preserve red/blue order and measured440/876.7Hz soundtracks in a1s export, original hashes unchanged, unreviewed selection and stale-segment export rejected. Generation workspace now filters current series/episode jobs, retaining history. Sidekick bubbles added: user right/warm tint, sidekick left/dark tint, wrapped/selectable text and spacing. 43 tests/Clippy/build/signature passed; no forced GUI restart while user has an active reply, bubble layout still needs visual acceptance. Evidence: `benchmarks/hosted-episode-assembly-v002.json`.


Series creation clarity: added a full-width + New series button under the global selector, an additional Synopsis entry point, and a clearly named creation dialog. New series opens Synopsis. Copper & Crown was separated into its own record with an independent conversation copy and empty cast/episodes; the original Other Chair cast, episodes and conversation remain. A SQLite backup was retained outside the repository before the repair. 43 tests, Clippy and packaged build passed. Actual Mac accessibility inspection confirms the sidebar button and separate projects; interaction was interrupted by user activity, so full visual acceptance remains pending.

GitHub source update authorized by the user on 2026-11-16 to the previously confirmed Baz-Studios-LLC organization. Public MIT repository already existed. README corrected for the current hosted rewrite and its limitations; local secrets, databases, environments and generated working data excluded. This authorization is for the requested source upload, not automatic future pushes, releases, tags or deployment.


Sidekick reliability fixes after actual user testing: successful Keychain reads now reuse a process-local credential cache; save replaces the cache and remove clears it. Keychain ACLs and signing are unchanged; a new process/rebuild can still require macOS authorization. No typing handler reads Keychain; explicit Send/check/generate were the only read sites. Cache tested with fake loader, not real credential reads. Sidekick now requests strict json_schema + require_parameters per OpenRouter docs, confirmed read-only live Gemini endpoints advertise structured_outputs/response_format. Invalid/truncated text is retained as a non-editing chat reply instead of discarded; no automatic retry. Unsent composer drafts autosave per series with restart restore and expected-text guards. 46 tests, Clippy, packaged build, strict signature and real MCP smoke passed. No paid calls. Actual Mac screenshot confirmed Copper & Crown empty cast, New series button and left/right bubbles; active pending reply prevented restarting to inspect the newest fixes. Character creation through sidekick is still unsupported and must be connected next; existing-character edits only. Hosted reference transport and all-document sidekick editing remain major gaps. Changes after the one authorized GitHub upload remain local.


Latest chat/cast usability work: user-message hover Edit/Delete implemented with series/role/expected-text guards; edits do not resend or regenerate assistant replies. Full-frame hover replaces inner text-region detection. Actual rebuilt Mac app verified Edit/Delete become enabled when pointer is in the empty bottom-right padding of a user bubble. Existing user messages/replies preserved during inspection. Added real elapsed time and separate Keychain/provider/response-reading stages; no inferred ETA. Per-series draft restart persistence remains tested.

Sidekick new_character structured action now creates a saved brief and portrait prompt atomically; generated IDs are application-owned, exact creation replay is idempotent and duplicate-name conflicts rejected. Added optional generate_portrait flag, instructed to require explicit latest-user image request; after creation it queues Grok and starts the worker, with partial failure honestly reported. Form has Create character & generate image · paid, and saved character cards have a visible Generate image · paid button. No agent provider calls. 48 tests including creation/restart/isolation/duplicate replay/queue construction passed; Clippy and packaged build passed. Actual live model creation/image action, new image buttons and newest voice-selector removal still need GUI acceptance; user was actively testing. Message mutation/new-character services still need dedicated MCP tool parity.

Removed Assigned voice Kokoro dropdown from character page: it did not affect Seedance's native audio. Historical database voice values remain for old projects. Voice consistency across separately generated Seedance segments remains unverified; no stable voice-ID control is claimed. Next work: live paid acceptance only under user spending authorization, hosted local-reference transport, all-document sidekick actions and remaining retired local services cleanup. No pushes after the separately authorized source upload.


MCP parity added for create_character_with_prompt (atomic saved brief+prompt, no provider call) and change_user_message (expected text, user role and series guards, no resend). Real stdio integration now tests exact creation replay, cross-series conflict, prompt retrieval, persisted message edit, stale-delete rejection and deletion. 48 tests/Clippy/build/MCP passed.

Portrait card now shows its latest matching job status, spinner while queued/running, measured elapsed time, worker phase and errors. Generate buttons disable while a portrait is unresolved to reduce accidental duplicate charges; completed cards offer review guidance and default-open portrait section. No fake ETA. User-initiated real Grok job f167a6c0-1948-43e8-9f6d-111dddf6264a was observed running and subsequently completed with phase Grok portrait ready for review; the agent did not submit or retry it. Visual image quality and new status panel still require review. No additional push.


v003 source update authorized by user after image-style feedback. Added per-character/location Photo, Cinematic, Animation and Illustration choices, Photo default. New jobs capture chosen style, effective provider prompt and original prompt; queued requests and original assets are untouched. Photo discourages glow, oversmoothing and painted/CGI rendering; Animation permits non-human expressive characters. These are prompt controls, not a quality guarantee. Actual v003 Mac screenshot inspected: all four buttons, Photo selected, completed portrait timing and visible generated-image gallery. No paid style comparison was run. 49 tests, Clippy, real MCP style/create/message integration, strict bundle signature and disposable updater integration passed. Version labels, Cargo0.3.0, MCP version, update CURRENT3 and bundle build3 aligned. Updater test fixture advances to v004; no actual v004 release.

Latest user-requested GitHub source upload includes all chat/cast improvements since the earlier v002 push. This does not declare the full studio finished or publish a binary release. Primary remaining gaps: hosted portrait transport into Seedance, all-document sidekick edits, live audiovisual quality/identity continuity, model choice, remaining local service deletion, robust in-flight chat recovery and production distribution signing. User-generated Tamsin portrait was visually inspected as a stylized still; user rejected its painterly finish, leading to style controls. Preserve it as an original attempt.


Post-v003-upload local development: segment profiles now expose 4–30 seconds and 480p/720p, with planned episode duration total. Seedance public catalog was queried read-only and confirmed these choices. Defaults migrate old plans to their original 10s/720p/9:16/native-audio behavior. Profiles persist with revision conflict guards, are captured in segment snapshots and feed actual VideoPlan requests; saving rejects out-of-range profiles. 50 tests/Clippy/build/real MCP profile save-read and hosted FFmpeg assembly integration passed. Actual new planner controls still need packaged GUI acceptance; no paid Seedance render. v003 retained following user's explicit version/update request; no further push.


Local-reference transport investigation: official OpenRouter Files API docs rule out workspace uploads as a downloadable video-reference source; uploaded files are sandbox-only. Official video cookbook still specifies direct public HTTPS URLs, and no video data-URL support was established. Recorded exact sources and remaining requirements in SEEDANCE_INPUTS.md; planner now explains this constraint instead of implying an upload path is nearly connected. No uploads/provider calls. This changes the next action: establish a supported transport before implementing a Files upload. Other all-document authoring and continuity work can continue independently. Goal remains incomplete; no new push.


v004 source upload requested by user after a separate polish pass. Included the shared native typography/theme, calmer surfaces and chat colors, aligned form controls, refined workflow navigation/cards, constrained Synopsis layout, sidekick visibility toggle, improved empty states and image-inspector layout. Also includes the previously local segment profile controls and documented Seedance reference-transport limits. Version labels, Cargo0.4.0, MCP, bundle4 and updater CURRENT4 aligned. Disposable updater fixture advances to v005; this does not publish v005. 50 tests, fmt, Clippy, real MCP, Mac bundle and disposable updater integration passed. Source upload only; no downloadable release/update artifact or notarization claim. Autonomous goal remains paused after this separately requested upload.
