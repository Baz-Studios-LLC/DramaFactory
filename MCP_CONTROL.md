# Live MCP Sidekick

The stdio server is the DramaFactory executable with `--mcp`. Its database must match the open app (`--data-path` prints the default; `DRAMAFACTORY_DATABASE` selects a specific local working copy). No remote listener or credentials endpoint is added.

## Session flow

1. `dramafactory_get_live_workspace` returns the GUI instance, current series/episode/page, unsaved drafts, shared chat, action contracts, and structured reply schema. An offline GUI produces an error rather than stale state.
2. `dramafactory_control_app` with a unique `request_id`, that `instance`, `action: "take_over"`, and `payload: {}` enables MCP Sidekick mode. A currently running built-in response must finish or be stopped first.
3. Poll `dramafactory_get_control_result` using the command ID. Queued is not completed. Reuse the same command ID and identical content after an uncertain response.
4. To act on a request from the external chat, issue `capture`. Its result returns a separate reply `request_id` and the captured authoring context. If Baz sends inside the app while in MCP mode, the pending request already exists in live state; do not capture a second one.
5. Read that context and shared chat, author the response, then issue `reply` with `payload: {"request_id": "captured reply ID", "reply": {"message": "...", "synopsis": null, ...}}`. Only include desired edits. The app validates the response and uses the same draft edit/Undo/conflict pipeline as built-in Sidekick. Delivery acceptance does not imply every proposed edit applied. Read the resulting chat for individual results.
6. `undo` supports synopsis, plot, episode, generation, publishing and character (also requires character_id). `save_drafts` requires the exact latest `workspace` object in `expected_workspace`; it preserves recovery snapshots and reports individual save failures. It saves the local working copy. Use portable save separately.
7. `release` returns to the built-in Sidekick. Baz can switch the **Sidekick chat / MCP chat** toggle at the top of the panel. Both modes share the same conversation. Switching modes stops any pending response so it cannot apply late. Stopped or released replies cannot arrive later. An app restart invalidates the old GUI instance and pending requests.

## Automatic replies

The MCP chat panel enables **Automatic Codex answers** by default. Send starts a local `codex exec` responder immediately, using the existing ChatGPT login, shared per-series history, latest attached images and captured workspace drafts. It is a separate ephemeral Codex session, not a wake-up of an existing desktop conversation. The reply enters the same validated draft/Undo/conflict pipeline. No API-key billing, automatic media generation, publishing or automatic retry is enabled. Codex user configuration is ignored; DramaFactory explicitly supplies its own stdio MCP server and exact working database. The responder can discover and use the application authoring/read tools across tabs and episodes. Shell execution is disabled and the process uses a read-only sandbox. Paid generation and external publishing tools are excluded from the automatic responder; they remain available to explicitly authorized external clients. Temporary request/schema/result files are removed after the request. Stop, mode changes and app exit cancel the responder process group. Errors retain the user message.

Turn off Automatic Codex answers for a manually connected client. `take_over` chooses that manual mode so two responders cannot answer the same request. `capture` remains a manual-client action. Restarting does not replay old unanswered messages. Live state reports `automatic_answers` and `automatic_running`.

The installed Codex CLI must support `exec --ignore-user-config --ephemeral --output-schema`; sign in using ChatGPT. No live model request is run during automated tests.

## Other controls

- `navigate`: page (exact enum in live state), optional series_id and episode_id.
- `edit_timeline`: exact `workspace.timeline` as `expected`, and complete replacement `cut`. Changes the live timeline draft and adds normal UI Undo.
- `get_episode_cut`, `save_episode_cut`, `export_episode_cut`: saved timeline service with optimistic guards and validated local export.
- `get_release_draft`, `save_release_draft`: saved publishing fields and available exports.
- `publish_episode`: explicit publishing action with `confirmation: true` and exact saved release `expected`. Uses Cloudinary and Buffer's existing immediate publish and duplicate-submission reservations. Returns Buffer acceptance, not proof of live platform delivery. Do not call without user authorization.
- `get_project_link`, `save_portable_project`: immutable portable snapshots with expected revision guards; preserve originals.
- Existing MCP tools cover series, characters/profiles/portrait prompts, outlines/screenplays, generation plans/jobs/takes, sound cues, continuity, reviews, and exports.

External replies cannot launch a paid cast queue or auto-generate portraits. Use explicit generation tools only after spending authorization. No provider requests are made merely by taking over, capturing, or replying.

## Persistence and limitations

The live mailbox lives in a sibling `*.control.sqlite`, outside portable snapshots. Commands expire after 30 seconds if not picked up. The GUI marks each command before executing, so interrupted work is never automatically replayed. Read completion receipts and resulting project state if an operation was interrupted.

This is structured workflow control, not a universal pixel/click API. Credential entry, native file dialogs, and some specialized controls (including music generation and cliffhanger framing) still use the UI. Saved-service edits do not overwrite dirty GUI drafts; use live replies or live timeline edits when working with unsaved content. Current snapshots/receipts are local files, not a remote access mechanism.

## Cross-tab episode batches

Automatic Codex can fulfill “fill out episodes 5 through 15” while Baz remains on Synopsis or any other page. It reads the overall plan and surrounding episodes using MCP, then:

1. `control_app` action `read_episode_batch` with `series_id`; poll its command receipt. The snapshot contains numbered episodes, their saved and live screenplay text, saved/live overall plans, and the proposal field schema.
2. Author the requested consecutive range, preserving source continuity. Send `apply_episode_batch` with `series_id`, the exact snapshot as `expected`, one-based `start_episode`, and 1-25 episode objects. Each object has title, screenplay and story beats. Existing slots keep their IDs; missing slots are created. No skipped episode numbers.
3. Poll the receipt and reread to verify. Large ranges can use successive batches, reading fresh state each time. A queued command is not completion.

A batch saves its targeted screenplays, titles and outlines atomically, and never queues media. The current page and selected episode stay unchanged. Unrelated unsaved plot and screenplay edits remain unsaved. A conflicting saved or GUI change rejects the batch. A failure partway through rolls back all database changes. Original snapshots are retained in the local `drafts/episode-batch-before-*.json` recovery files. These batch saves use recovery snapshots rather than the single-episode Undo button. Existing saved-service tools continue to cover other workspaces; a dirty GUI document should be edited through live structured replies rather than overwritten through saved-service tools.
