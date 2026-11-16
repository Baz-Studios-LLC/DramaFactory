# DramaFactory v002 — OpenRouter rewrite design

Status: target design, not a statement of implemented functionality. Supersedes the earlier local production workflow. Rust-native Mac first, existing branding, one OpenRouter connection for every AI request. No local inference or separate lip-sync workflow. Preserve original projects and media; delete unused implementation rather than retain legacy modes.

## Navigation and shared context

Five numbered destinations, always in this order:
1. Synopsis
2. Characters & world
3. Overall plot
4. Episode plot
5. Episode generation

Settings sits separately at the bottom. Current series is remembered globally. An episode browser appears on steps 4 and 5, with numbered searchable cards and previous/next, not a long dropdown. Series-level pages do not ask for an episode. Each page explains what is needed, presents one primary action, and links to the next step. Users can revisit any step. Readiness is based on saved material and explicit review, never inferred creative approval.

## 1. Synopsis

Series title, premise, genre/tone, audience, central relationship/conflict, visual style and format. Defaults: 67 episodes, about 90 seconds each, vertical format; editable. Develop/refine with AI or edit directly. AI proposals are drafts with differences shown before applying. Primary action: Develop synopsis; then Continue to characters. Do not generate an entire unreviewed season in the background.

## 2. Characters & world

Large character cards, full-size reference inspection, names, role, age, personality, relationships and wardrobe. Generate or edit portraits through Grok Imagine via OpenRouter; upload existing references too. User-approved identities remain selected until explicitly replaced. Retain every attempt. Locations live in a secondary World tab on this page, with the same large-reference treatment. Avoid separate image-studio navigation: image creation belongs to the person/place being developed. Voice descriptions belong to character identity; no separate TTS assignment requirement.

## 3. Overall plot

Series arc, ending, character arcs, turning points, setups/payoffs and ordered episode outline. Clearly distinguish outline from written screenplay. Show season as searchable ordered episode cards, not 67 expanded editors. AI develops proposals from the saved synopsis and cast. Changes show affected episodes and preserve prior revisions. A semantic review is distinct from simple structural validation. Primary action: Develop series plot; then Open episode plot.

## 4. Episode plot

Selected episode shows previous-episode recap, purpose, conflict, emotional turn, hook, screenplay and dialogue. Develop the story using series canon and selected identities. Plan connected generation segments under the chosen model's current duration limits. A segment can contain motivated camera cuts; do not force every 1–4-second camera angle into a separate provider request. Show timing, characters/location, action, words, atmosphere/music and continuity anchors in a readable segment list. Prompt details are inspectable/editable, not the default giant form. Primary action: Prepare generation. Changing saved story/reference inputs marks affected prepared requests stale instead of silently reusing them.

## 5. Episode generation

Three tabs inside one destination: Prepare, Takes, Finished episode.

Prepare shows segment cards with duration, model, reference thumbnails, expected inputs and sound instructions. Preview exactly what will be sent. Use the same key and provider for writing/images/video. Display provider-supported settings only. Offer generate one segment or the planned batch with explicit cost visibility; do not fabricate a fixed estimate when token-based billing cannot be predicted accurately. No automatic paid retries for ambiguous submissions.

Takes shows real remote states, reconnectable jobs, full-resolution synchronized playback, clear error/retry actions, comparisons and explicit selection. Provider completion is not creative approval. Keep original sound with its video. Check exact words, voices, faces, hands, costumes and continuity. Changing a take never deletes the old one.

Finished episode assembles chosen segments in story order, lets the user trim/reorder where appropriate, reviews soundtrack boundaries and exports one MP4 with generated sound. Optional captions and simple final adjustments stay here. Record a reviewed continuity recap before continuing to the next episode. Do not promise music or voice continuity between independent requests; review it and replace/adjust failed results.

## Seedance input design

Verified against OpenRouter's public video catalog and documentation on 2026-11-16; re-fetch capabilities before enabling controls. Model default: bytedance/seedance-2.5. New segments default to portrait 9:16 at 480p; existing saved settings stay intact. Current catalog: 4–30 seconds, 480p/720p/1080p (live catalog rechecked), portrait 9:16 supported, native audio, first/last frame roles. Never equate limits of ByteDance's direct product with those exposed by OpenRouter.

Generation mode is explicit:
- Prompt: no supplied frames/references.
- Reference guidance: character, location and style images as input_references, with a visible ordered mapping to the prompt.
- Frame control: first frame and optional last frame as frame_images with first_frame/last_frame roles. First frame can come from an approved generated scene image or a previous selected segment's extracted ending frame.

OpenRouter documents that frame_images takes precedence over input_references. Therefore do not expose a UI that promises independent character references PLUS exact start/end anchors in one request unless the selected route explicitly supports that combination. Preserve reference selections while switching modes, but clearly show which inputs are active/sent. A character portrait is guidance, not an exact first-frame composition. Create a scene frame from character/location references when frame-control mode requires it.

Reference-video/audio, edit/extension support, number/size/format limits, upload/data-URL support and valid combinations require route-specific verification. Catalog descriptions alone are insufficient. Until verified, do not enable these controls or invent numerical limits. Last-frame-only support likewise needs explicit verification rather than inferring it from the last_frame enum. Audio descriptions do not guarantee a reusable voice identity.

Documentation: https://openrouter.ai/docs/guides/overview/multimodal/video-generation
Catalog: https://openrouter.ai/api/v1/videos/models
Images: https://openrouter.ai/docs/guides/overview/multimodal/image-generation

## Settings and platform behavior

One masked OpenRouter API key stored in macOS Keychain. Show saved/not saved and connection status without displaying credentials. Add replace/remove/test actions; connection tests must not start paid generation. Select capable writing model, image model and video model with sensible defaults and capability discovery. No Python paths, model downloads or lip-sync settings. Keep updates and local project/export storage settings. No claim of notarization until actual Developer ID packaging exists.

## Services, durability and MCP

Shared Rust services for saved series context, revisioned creative drafts, reference assets, generation plans, remote job records, downloads, take selection and export. MCP calls the same services and observes saved state. Credentials are never returned by MCP, logged or included in prompts/exports. Persist request identity and remote ID before polling; recover accepted remote jobs after restart without resubmitting. Ambiguous submit failures require reconciliation rather than automatic duplicate spend. Poll/download failures resume the same remote job. Validate downloaded media and hashes, keep local originals, and restrict credential-bearing requests to trusted OpenRouter origins. A local cancel must not claim the provider stopped billing unless cancellation is confirmed.

## Acceptance and implementation order

1. Complete provider credential lifecycle, capability discovery and shared API client; test with local mock server before paid calls.
2. Replace navigation and page content with these five destinations, including first-use empty states and global context.
3. Implement creative proposals and in-context image generation, reference selection and revisions.
4. Implement segment plans and Seedance mode-specific request validation.
5. Implement durable remote submit/poll/download, take review and soundtrack-preserving assembly/export; verify real server/MCP behavior.
6. Remove all obsolete local services, adapters, setup scripts, dependencies and UI. Keep historical benchmark reports/media as evidence, not a runnable legacy product mode.
7. Packaged Mac visual/interaction review, fresh-install checks, conflict/restart/cancel tests and paid end-to-end sample only after spending authorization.

Unresolved details should be verified or decided during implementation without blocking this design: route-specific media limits, writing model default, upload transport and remote cancellation behavior. The five-step product flow is the intended design; current screens still require substantial replacement.

## Synopsis discussion and direct draft editing — accepted direction

Keep the five-step navigation on the far left. Within Synopsis, the main document editor occupies the larger left area and a resizable discussion pane occupies the right. On narrow windows, Discussion becomes a toggleable drawer, preserving the document width.

Discussion remembers its conversation per series across restarts. Display the selected writing model and concise usage/cost information. Initial recommendation as researched 2026-11-16: google/gemini-4-argon; alternative anthropic/claude-opus-5.5. Arena creative-writing ranking (2026-11-11) places Argon High first (1523±10) and Opus5.5 High second (1517±11); intervals overlap. This is a shortlist/default decision, not proof of superiority for serialized drama. OpenRouter usage rankings measure popularity, not literary merit. Verify exact model availability/pricing dynamically. No paid comparison has been performed.

User can brainstorm freely, request alternatives, criticize a direction, or ask the assistant to rewrite the synopsis. Chat replies stay in Discussion; actual document revisions update the main editable draft with an explicit changed indicator and Undo. Do not require copying prose out of chat. Do not save model revisions to canon automatically. Keep a before/after change view available, preserve manual edits, and apply only to the exact draft revision the request read. If the user types while a response is pending, retain the proposed edit separately and show a conflict rather than overwriting newer text. A structured document-edit payload must be validated independently from the conversational answer. Conversation/prompt content cannot invoke arbitrary application tools or secrets.

Primary actions: Send message, Update synopsis from discussion, Undo draft edit, Save synopsis, Continue to characters. User instructions such as “rewrite that opening” can request an editor update in the same response; informational questions need not change the document. Generation failures preserve the message/draft and permit explicit retry, with no automatic duplicate paid submissions. Per-series conversation and revisions must persist through shared UI/MCP services. Cancellation must not claim that provider billing stopped unless confirmed. Brainstorming needs the user-supplied key and explicit user send; autonomous development does not run paid comparisons.

Research sources: https://arena.ai/leaderboard/chat/text/creative-writing ; https://openrouter.ai/google/gemini-4-argon ; https://openrouter.ai/anthropic/claude-opus-5.5 . EQ-Bench creative-writing v3 is LLM-judged; its ranking table was not exposed by the fetched page, so do not invent scores from it.

Latest clarification: discussion belongs on every page, not only Synopsis. One conversation per series spans all tabs; workspace/episode context changes without creating a new thread. Sidekick must edit the current content, including character briefs and Grok portrait prompts; image results require selection before replacing approved identity.

## Connected episode preparation

The episode screenplay is the source of truth. Sidekick generation proposals use current series, cast, locations, image style, plot and screenplay context to author a complete ordered segment plan. Visual plan and Episode generation share this plan. Each prompt carries relevant context and explicit dialogue; several shots may belong to one generated segment. Proposals remain drafts with source/context conflict checks and Undo. Generate episode is the explicit paid action: it saves the draft and atomically queues missing requests, preserving existing identical requests without retrying them. Selected local portraits are resolved into embedded image data at submission, with numbered character-name/profile mapping and hash verification. BytePlus documents data URIs and the OpenRouter request schema accepts an image URL string, but the cookbook demonstrates HTTPS URLs only: end-to-end embedded-image route acceptance remains unverified without a paid test. No voice identity guarantee or automatic ending-frame handoff is implied.


## Minimal interface direction (2026-11-16)

Keep the studio minimal, friendly and intuitive. Use compact portrait cards for character selection, with names and clear selected states. Each workspace should emphasize its immediate creative task and primary action. Collapse batch actions and technical details; omit repeated payment reminders and implementation explanations. Preserve actionable errors, explicit generation controls, save feedback and destructive-action confirmation.

Generation is repeatable: an explicit Generate action after a completed result makes a new take, even with identical settings. Keep every generated take visible in segment review, including rejected takes, for comparison and future non-destructive editing. Hiding older production attempts must not hide playable clips. Repeated clicks while the same request remains unresolved reuse that request.


## Editing room first implementation (2026-11-16)

Sixth primary workspace: per-episode non-destructive cuts using any generated takes, including multiple ranges from the same source. Clip library with drag-to-insert, horizontal sequence timeline, drag reorder, drag-edge and numeric in/out trims, split, ripple delete, Undo/Redo, immediate optimistic saves, immutable local MP4 export. Picture and original sound share source offsets. Source preview, local rendered timeline playback and direct source-frame scrubbing are available. Frame stepping, transitions and Sidekick edit actions remain future work. Existing generation plans and original media are preserved.
