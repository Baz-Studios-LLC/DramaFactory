use crate::{production::RenderRequest, store::Store};
use anyhow::{Context, Result, bail};
use rmcp::{
    ServerHandler, ServiceExt,
    model::*,
    service::{RequestContext, RoleServer},
};
use serde_json::{Value, json};
use std::path::PathBuf;
fn definitions() -> Vec<Tool> {
    let mut list = Vec::new();
    let mut add = |name: &str, description: &str, properties: Value, required: Vec<&str>| {
        let schema = json!({"type":"object","properties":properties,"required":required,"additionalProperties":false});
        list.push(Tool::new(
            format!("dramafactory_{name}"),
            description.to_string(),
            schema.as_object().unwrap().clone(),
        ));
    };
    let string = || json!({"type":"string"});
    add("list_series", "List local series", json!({}), vec![]);
    add(
        "create_series",
        "Create a series",
        json!({"name":string(),"premise":string()}),
        vec!["name"],
    );
    add(
        "create_episode",
        "Create an episode",
        json!({"series_id":string(),"title":string()}),
        vec!["series_id", "title"],
    );
    add(
        "list_episodes",
        "List episodes",
        json!({"series_id":string()}),
        vec!["series_id"],
    );
    add(
        "save_screenplay",
        "Save episode screenplay. Include expected_text from get_episode to reject concurrent edits without overwriting them.",
        json!({"episode_id":string(),"text":string(),"expected_text":string()}),
        vec!["episode_id", "text"],
    );
    let render = json!({"prompt":string(),"negative_prompt":string(),"width":{"type":"integer","minimum":256,"maximum":1280,"multipleOf":32},"height":{"type":"integer","minimum":256,"maximum":1280,"multipleOf":32},"frames":{"type":"integer","minimum":5,"maximum":121},"steps":{"type":"integer","minimum":1,"maximum":60},"seed":{"type":"integer","minimum":0,"maximum":4294967295u64},"image":string(),"shot_id":string()});
    add(
        "queue_render",
        "Queue real local Wan generation. Returns a durable job ID immediately; blocked if model or worker is not configured.",
        render.clone(),
        vec!["prompt"],
    );
    add(
        "generate_video",
        "Generate video using the same durable local queue",
        render,
        vec!["prompt"],
    );
    add(
        "list_jobs",
        "List durable production jobs and actual progress",
        json!({}),
        vec![],
    );
    for (name, desc) in [
        (
            "cancel_job",
            "Request cancellation of a running or waiting job",
        ),
        ("retry_job", "Retry a failed, canceled or blocked job"),
        ("get_job_status", "Get one job's status"),
    ] {
        add(name, desc, json!({"job_id":string()}), vec!["job_id"]);
    }
    add(
        "create_shot",
        "Append a shot to the editable episode sequence",
        json!({"episode_id":string(),"title":string(),"prompt":string(),"camera":string(),"duration":{"type":"number","exclusiveMinimum":0,"maximum":30}}),
        vec!["episode_id", "title", "prompt", "duration"],
    );
    add(
        "list_shots",
        "Get the ordered episode shot list",
        json!({"episode_id":string()}),
        vec!["episode_id"],
    );
    add(
        "trim_shot",
        "Save a non-destructive picture in-point and duration against the expected selected take/range. Originals and dialogue timing are preserved. Recheck lip sync after picture edits.",
        json!({"trim":{"type":"object","additionalProperties":false,"properties":{"shot_id":string(),"source_start":{"type":"number","minimum":0},"duration":{"type":"number","minimum":0.04,"maximum":30},"expected_source_start":{"type":"number"},"expected_duration":{"type":"number"},"expected_asset_id":string()},"required":["shot_id","source_start","duration","expected_source_start","expected_duration","expected_asset_id"]}}),
        vec!["trim"],
    );
    add(
        "get_shot",
        "Get a shot",
        json!({"shot_id":string()}),
        vec!["shot_id"],
    );
    add(
        "list_assets",
        "List generated assets, provenance and review state",
        json!({}),
        vec![],
    );
    add(
        "review_asset",
        "Approve, reject, or clear a generated asset review",
        json!({"asset_id":string(),"review":{"type":"string","enum":["approved","rejected","unreviewed"]}}),
        vec!["asset_id", "review"],
    );
    for name in ["get_asset_review", "get_asset_review_history"] {
        add(
            name,
            "Read editorial decision, notes or immutable review history",
            json!({"asset_id":string()}),
            vec!["asset_id"],
        );
    }
    add(
        "save_asset_review",
        "Save a decision and editorial note using the revision returned by get_asset_review. Stale edits reject without losing saved notes. Media/provenance is preserved.",
        json!({"review":{"type":"object","additionalProperties":false,"properties":{"asset_id":string(),"revision":{"type":"integer","minimum":0},"decision":{"type":"string","enum":["approved","rejected","unreviewed"]},"note":string()},"required":["asset_id","revision","decision","note"]}}),
        vec!["review"],
    );
    for (kind, plural) in [("character", "characters"), ("location", "locations")] {
        add(
            &format!("create_{kind}"),
            "Create a reusable series library entry; an optional reference is copied into managed storage",
            json!({"series_id":string(),"name":string(),"description":string(),"reference":string()}),
            vec!["series_id", "name"],
        );
        add(
            &format!("list_{plural}"),
            "List series library entries",
            json!({"series_id":string()}),
            vec!["series_id"],
        );
    }
    add(
        "export_episode",
        "Queue deterministic assembly of the approved episode shot list",
        json!({"episode_id":string()}),
        vec!["episode_id"],
    );
    add(
        "update_shot",
        "Revise an existing shot without deleting generated assets",
        json!({"shot_id":string(),"title":string(),"prompt":string(),"camera":string(),"duration":{"type":"number","exclusiveMinimum":0,"maximum":30}}),
        vec!["shot_id", "title", "prompt", "duration"],
    );
    add(
        "move_shot",
        "Move a shot one position earlier or later",
        json!({"shot_id":string(),"offset":{"type":"integer","enum":[-1,1]}}),
        vec!["shot_id", "offset"],
    );
    add(
        "generate_dialogue",
        "Queue local Kokoro speech for a shot; optionally regenerate an existing line",
        json!({"shot_id":string(),"text":string(),"voice":string(),"speed":{"type":"number","minimum":0.5,"maximum":2},"line_id":string()}),
        vec!["shot_id", "text", "voice"],
    );
    add(
        "set_dialogue_timing",
        "Place a generated or pending line at an absolute episode time so it can bridge camera cuts. Null restores sequential timing inside its shot; audio is not regenerated.",
        json!({"line_id":string(),"timeline_start":{"type":["number","null"],"minimum":0,"maximum":1800}}),
        vec!["line_id", "timeline_start"],
    );
    add(
        "set_caption_mode",
        "Choose a separate SRT file or captions burned into the final MP4. Both modes write SRT when dialogue exists. Long-line chunks use estimated timing; line boundaries use actual generated audio duration.",
        json!({"episode_id":string(),"burn_in":{"type":"boolean"}}),
        vec!["episode_id", "burn_in"],
    );
    add(
        "list_dialogue",
        "List a shot's dialogue lines and generated assets",
        json!({"shot_id":string()}),
        vec!["shot_id"],
    );
    add(
        "export_dialogue_rehearsal",
        "Queue an audio-only WAV of all generated dialogue at episode timeline positions. No video is required. Missing speech and timing errors are reported; this is a rehearsal, never a final episode cut.",
        json!({"episode_id":string()}),
        vec!["episode_id"],
    );
    add(
        "produce_episode_dialogue",
        "Resume speech production for all saved episode lines. Keeps usable selected speech, reuses pending jobs and retries current failed/canceled attempts. Repeated/concurrent calls are idempotent. Does not write scripts, approve takes, or render video.",
        json!({"episode_id":string()}),
        vec!["episode_id"],
    );
    add(
        "save_dialogue_draft",
        "Save authored dialogue without inference or model setup. Supply a stable UUID and revision 0 for a new line, then the returned revision for edits. Content edits clear voice selection, preserving original assets. Timing-only edits retain voice. Repeated identical saves are idempotent.",
        json!({"line":{"type":"object","properties":{"id":string(),"shot_id":string(),"text":string(),"voice":string(),"speed":{"type":"number"},"timeline_start":{"type":["number","null"]},"revision":{"type":"integer","minimum":0}},"required":["id","shot_id","text","voice","speed","timeline_start","revision"],"additionalProperties":false}}),
        vec!["line"],
    );
    add(
        "delete_character",
        "Recoverably remove a character from active cast using its complete expected entry from list_characters. Retains portraits, profiles and history. Rejects stale entries, saved story/episode links and active jobs.",
        json!({"expected":{"type":"object"}}),
        vec!["expected"],
    );
    add(
        "list_deleted_characters",
        "List recoverably deleted characters in a series",
        json!({"series_id":string()}),
        vec!["series_id"],
    );
    add(
        "restore_character",
        "Restore the same deleted character and its retained profile and portrait to the active cast",
        json!({"series_id":string(),"character_id":string()}),
        vec!["series_id", "character_id"],
    );
    add(
        "get_character_profile",
        "Read structured character details and expected saved baselines. Blank fields are unspecified; existing prose is preserved as summary.",
        json!({"character_id":string()}),
        vec!["character_id"],
    );
    add(
        "save_character_profile",
        "Save a complete character profile draft and compose its production brief atomically. Supply the baselines from get_character_profile. Preserves approved images and portrait prompts; no generation.",
        json!({"draft":{"type":"object"}}),
        vec!["draft"],
    );
    add(
        "update_library_entry",
        "Update a stable cast/location entry name and brief, preserving its reference and voice. Supply expected saved name/description for conflict protection; identical retries are idempotent. Existing coverage and canon require review after brief changes.",
        json!({"edit":{"type":"object","additionalProperties":false,"properties":{"id":string(),"name":string(),"description":string(),"expected_name":string(),"expected_description":string()},"required":["id","name","description","expected_name","expected_description"]}}),
        vec!["edit"],
    );
    add(
        "assign_character_voice",
        "Persist a Kokoro voice identity for a character",
        json!({"character_id":string(),"voice":string()}),
        vec!["character_id", "voice"],
    );
    add(
        "get_episode",
        "Get one episode and its screenplay",
        json!({"episode_id":string()}),
        vec!["episode_id"],
    );
    add(
        "get_series",
        "Get one series",
        json!({"series_id":string()}),
        vec!["series_id"],
    );
    add(
        "update_series",
        "Update a series title and premise",
        json!({"series_id":string(),"name":string(),"premise":string()}),
        vec!["series_id", "name", "premise"],
    );
    add(
        "update_episode",
        "Rename an episode",
        json!({"episode_id":string(),"title":string()}),
        vec!["episode_id", "title"],
    );
    add(
        "assign_shot_asset",
        "Select a generated take for a shot without removing previous assets",
        json!({"shot_id":string(),"asset_id":string()}),
        vec!["shot_id", "asset_id"],
    );
    add(
        "generate_image",
        "Queue local FLUX.2 Klein generation or reference-guided editing. An optional library_id selects and updates a reusable cast/location reference.",
        json!({"prompt":string(),"width":{"type":"integer","minimum":256,"maximum":1536,"multipleOf":16},"height":{"type":"integer","minimum":256,"maximum":1536,"multipleOf":16},"steps":{"type":"integer","minimum":1,"maximum":20},"seed":{"type":"integer","minimum":0},"reference":string(),"library_id":string()}),
        vec!["prompt"],
    );
    add(
        "synchronize_lips",
        "Experimental local MuseTalk synchronization. Requires one visible face, a completed video asset and a shorter dialogue asset. Originals are preserved. Include the returned revision when updating.",
        json!({"video_asset_id":string(),"audio_asset_id":string(),"shot_id":string()}),
        vec!["video_asset_id", "audio_asset_id"],
    );
    add(
        "generate_performance",
        "Experimental offline LTX image+audio-conditioned acting. Uses one visible generated source line and a linked cast reference or explicit reference image. Optional source_line_id supports bounded continuations from another shot in the same episode with the same primary character; full episode audio is preserved. Preserves originals and returns an unreviewed, unselected take. Synchronization is not guaranteed.",
        json!({"shot_id":string(),"audio_asset_id":string(),"reference":string(),"source_line_id":string()}),
        vec!["shot_id", "audio_asset_id"],
    );
    add(
        "get_picture_fades",
        "Get saved shot picture fades in seconds. Zero means a hard cut.",
        json!({"shot_id":string()}),
        vec!["shot_id"],
    );
    let fades = json!({"type":"object","additionalProperties":false,"properties":{"fade_in":{"type":"number","minimum":0},"fade_out":{"type":"number","minimum":0}},"required":["fade_in","fade_out"]});
    add(
        "set_picture_fades",
        "Non-destructive picture fades from/to black within a shot. Preserves total duration and all audio timing. Supply expected saved values for conflict protection; identical retries are idempotent. Not an overlapping dissolve.",
        json!({"edit":{"type":"object","additionalProperties":false,"properties":{"shot_id":string(),"fades":fades.clone(),"expected":fades},"required":["shot_id","fades","expected"]}}),
        vec!["edit"],
    );
    add(
        "get_dialogue_direction",
        "Get whether a saved dialogue speaker is visible in its source shot. Off-screen audio remains exportable; native speaking animation must not animate a listener.",
        json!({"line_id":string()}),
        vec!["line_id"],
    );
    add(
        "set_dialogue_visibility",
        "Save on/off-screen speaker direction with expected-state protection. Preserves speech takes and timing, advances the authoring revision, and blocks off-screen lines from native speaking animation. Does not detect visual speakers automatically.",
        json!({"line_id":string(),"speaker_visible":{"type":"boolean"},"expected_visible":{"type":"boolean"}}),
        vec!["line_id", "speaker_visible", "expected_visible"],
    );
    add(
        "get_dialogue_timing",
        "Inspect saved episode speech placement using registered durations, including unknown timings, overlaps and camera-cut/episode overrun flags. Read-only; overlaps may be intentional. Does not verify current file hashes, listening quality or word alignment.",
        json!({"episode_id":string()}),
        vec!["episode_id"],
    );
    add(
        "move_dialogue",
        "Move a line to another shot in the same episode, preserving its stable ID, generated speech, speaker visibility and absolute episode start. Converts known implicit placement to explicit timing. Rejects stale edits and unknown earlier timing; does not regenerate speech or approve coverage.",
        json!({"edit":{"type":"object","additionalProperties":false,"properties":{"line_id":string(),"target_shot_id":string(),"expected_shot_id":string(),"expected_revision":{"type":"integer","minimum":0}},"required":["line_id","target_shot_id","expected_shot_id","expected_revision"]}}),
        vec!["edit"],
    );
    add(
        "find_assets",
        "Search local media with paged results, media kind and review filters. Searches IDs, prompts, speech, current shot/library/episode/series labels and saved editorial notes. Literal text search; does not infer depicted identities, modify reviews or verify media files. Newest first. Existing list_assets remains available.",
        json!({"text":string(),"media":{"type":"string","enum":["all","video","image","audio"]},"review":{"type":"string","enum":["all","approved","unreviewed","rejected"]},"limit":{"type":"integer","minimum":1,"maximum":100},"offset":{"type":"integer","minimum":0}}),
        vec![],
    );
    add(
        "get_episode_readiness",
        "Read-only saved-state production checklist: all missing pictures/speech, review requirements, reference planning warnings and the actual export-request gate. Does not enqueue, approve, verify media bytes or claim story/acting quality. Refresh after edits.",
        json!({"episode_id":string()}),
        vec!["episode_id"],
    );
    add(
        "get_production_status",
        "Get real production counts and active jobs",
        json!({}),
        vec![],
    );
    add(
        "save_synopsis",
        "Save series title/premise with expected saved text; conflicts preserve newer edits",
        json!({"draft":{"type":"object"}}),
        vec!["draft"],
    );
    add(
        "apply_synopsis_proposal",
        "Validate a sidekick proposal against an exact draft snapshot and return an unsaved draft. Never writes series state or calls a model.",
        json!({"current":{"type":"object"},"expected":{"type":"object"},"edit":{"type":"object"}}),
        vec!["current", "expected", "edit"],
    );
    add(
        "get_portrait_style",
        "Read saved image style; does not generate an image",
        json!({"library_id":string()}),
        vec!["library_id"],
    );
    add(
        "save_portrait_style",
        "Set Photo, Cinematic, Animation or Illustration for future images; existing images and queued requests are unchanged",
        json!({"library_id":string(),"style":{"type":"string","enum":crate::portrait_style::PortraitStyle::ALL}}),
        vec!["library_id", "style"],
    );
    add(
        "get_portrait_prompt",
        "Read a character/location portrait prompt and saved baseline",
        json!({"library_id":string()}),
        vec!["library_id"],
    );
    add(
        "save_portrait_prompt",
        "Save portrait prompt with expected-text conflict protection; no generation",
        json!({"draft":{"type":"object"}}),
        vec!["draft"],
    );
    add(
        "generate_grok_portrait",
        "PAID: queue one Grok portrait via OpenRouter using the complete saved character profile plus the supplied portrait direction and selected style. Save form changes first; profile traits take priority over conflicting prompt traits. No automatic reference replacement. Requires user spending authorization.",
        json!({"library_id":string(),"prompt":string()}),
        vec!["library_id", "prompt"],
    );
    add(
        "create_character_with_prompt",
        "Create a saved character brief and portrait prompt atomically. No image generation or provider call. Caller-generated character_id makes exact replay idempotent.",
        json!({"series_id":string(),"character_id":string(),"name":string(),"description":string(),"portrait_prompt":string(),"profile":{"type":"object"},"style":{"type":"string","enum":crate::portrait_style::PortraitStyle::ALL}}),
        vec![
            "series_id",
            "character_id",
            "name",
            "description",
            "portrait_prompt",
        ],
    );
    add(
        "change_user_message",
        "Edit or delete one user chat message using expected text and series guards. replacement null deletes. Does not call a model, change replies or undo document edits.",
        json!({"series_id":string(),"message":{"type":"object"},"replacement":{"type":["string","null"]}}),
        vec!["series_id", "message", "replacement"],
    );
    add(
        "get_sidekick_messages",
        "Read the series conversation shared by every workspace; latest 200 messages",
        json!({"series_id":string()}),
        vec!["series_id"],
    );
    add(
        "generate_episode_segment",
        "PAID: queue a saved episode segment with exact revision/snapshot and reuse an existing unresolved attempt. Requires explicit user spending authorization.",
        json!({"episode_id":string(),"revision":{"type":"integer","minimum":1},"segment":{"type":"object"}}),
        vec!["episode_id", "revision", "segment"],
    );
    add(
        "select_segment_take",
        "Select a reviewed hosted segment take with expected-selection conflict protection",
        json!({"episode_id":string(),"segment_id":string(),"asset_id":string(),"expected_asset_id":{"type":["string","null"]}}),
        vec!["episode_id", "segment_id", "asset_id", "expected_asset_id"],
    );
    add(
        "export_hosted_episode",
        "Assemble selected approved hosted segment takes in saved order, preserving generated soundtracks; local FFmpeg only",
        json!({"episode_id":string()}),
        vec!["episode_id"],
    );
    add(
        "get_segment_jobs",
        "Read generation jobs/results tied to one episode segment",
        json!({"episode_id":string(),"segment_id":string()}),
        vec!["episode_id", "segment_id"],
    );
    add(
        "generate_seedance_video",
        "PAID: queue Seedance video+audio through OpenRouter. Explicit user spending authorization required. Durable remote identity prevents automatic duplicate submission.",
        json!({"plan":{"type":"object"}}),
        vec!["plan"],
    );
    add(
        "prepare_cast_reference",
        "Bind an episode segment to the exact selected portrait of a same-series character; read-only, no upload or generation",
        json!({"episode_id":string(),"character_id":string()}),
        vec!["episode_id", "character_id"],
    );
    add(
        "get_job_timing",
        "Read recorded worker attempt elapsed time and stage; unknown ETA stays null",
        json!({"job_id":string()}),
        vec!["job_id"],
    );
    add(
        "get_generation_plan",
        "Read saved episode generation segments and revision",
        json!({"episode_id":string()}),
        vec!["episode_id"],
    );
    add(
        "save_generation_plan",
        "Save episode segment drafts with revision conflicts; never generates or bills",
        json!({"plan":{"type":"object"}}),
        vec!["plan"],
    );
    add(
        "preview_video_plan",
        "Validate a hosted video plan against live public capabilities and return an inspectable payload. Never submits or incurs generation charges; endpoint validation remains required.",
        json!({"plan":{"type":"object"}}),
        vec!["plan"],
    );
    add(
        "get_openrouter_catalog",
        "Fetch public image/video capabilities from OpenRouter; no key or paid generation",
        json!({}),
        vec![],
    );
    add(
        "list_available_models",
        "List configured local adapters and installed file readiness",
        json!({}),
        vec![],
    );
    add(
        "check_model_runtime",
        "Run bounded offline Python imports and FFmpeg identity checks for one local adapter. No model loading, downloads or generation. Reports versions/errors separately from file readiness and quality.",
        json!({"model_id":string()}),
        vec!["model_id"],
    );
    add(
        "get_model_status",
        "Inspect the configured local model and executable availability",
        json!({}),
        vec![],
    );
    add(
        "import_audio",
        "Copy a mono/stereo PCM or float WAV (up to 200 MB) into managed assets; original preserved",
        json!({"path":string()}),
        vec!["path"],
    );
    add(
        "list_audio_cues",
        "List timed music and sound effects for an episode",
        json!({"episode_id":string()}),
        vec!["episode_id"],
    );
    add(
        "save_audio_cue",
        "Create/update a music or sfx cue. Seconds, gain in dB; fades must fit duration. Omit id for a new cue. Updating an existing cue requires expected: the complete cue from list_audio_cues. Identical retries are idempotent. Audio must be approved before export.",
        json!({"expected":{"type":"object","description":"Complete last-saved cue; required for changes to an existing ID"},"id":string(),"episode_id":string(),"asset_id":string(),"name":string(),"kind":{"type":"string","enum":["music","sfx"]},"start":{"type":"number","minimum":0},"source_start":{"type":"number","minimum":0},"duration":{"type":"number","exclusiveMinimum":0},"gain_db":{"type":"number","minimum":-60,"maximum":12},"fade_in":{"type":"number","minimum":0},"fade_out":{"type":"number","minimum":0}}),
        vec![
            "episode_id",
            "asset_id",
            "name",
            "kind",
            "start",
            "source_start",
            "duration",
            "gain_db",
            "fade_in",
            "fade_out",
        ],
    );
    add(
        "remove_audio_cue",
        "Remove a timeline cue only if it matches the complete expected saved cue. Preserve the original audio asset; repeated removal is idempotent.",
        json!({"id":string(),"expected":{"type":"object"}}),
        vec!["id", "expected"],
    );
    let direction = json!({"type":"object","additionalProperties":false,"properties":{"revision":{"type":"integer","minimum":0},"character_ids":{"type":"array","items":string()},"location_id":{"type":["string","null"]},"camera_setup_id":{"type":["string","null"]},"dramatic_beat":string(),"performance":string(),"wardrobe":string()},"required":["character_ids","location_id","camera_setup_id","dramatic_beat","performance","wardrobe"]});
    add(
        "get_shot_direction",
        "Get stable cast, location and camera links plus performance, wardrobe and story beat",
        json!({"shot_id":string()}),
        vec!["shot_id"],
    );
    add(
        "save_shot_direction",
        "Link same-series cast (first character is primary reference), location and a camera setup at that location. Originals are preserved.",
        json!({"shot_id":string(),"direction":direction}),
        vec!["shot_id", "direction"],
    );
    add(
        "list_camera_setups",
        "Get reusable camera geometry for a location",
        json!({"location_id":string()}),
        vec!["location_id"],
    );
    add(
        "save_camera_setup",
        "Create/update a reusable camera setup. Omit id for a new camera. Setups keep a fixed location. Include the returned revision when updating.",
        json!({"id":string(),"revision":{"type":"integer","minimum":0},"location_id":string(),"name":string(),"framing":string(),"placement":string(),"eyeline":string(),"screen_axis":string()}),
        vec![
            "location_id",
            "name",
            "framing",
            "placement",
            "eyeline",
            "screen_axis",
        ],
    );
    add(
        "prepare_shot_render",
        "Prepare a render from shot direction, series style, saved camera and primary cast reference. Does not enqueue. Notes identify pacing outliers and the single-reference limitation.",
        json!({"shot_id":string()}),
        vec!["shot_id"],
    );
    add(
        "render_shot",
        "Prepare and queue local video from saved shot direction. Requires a primary character reference when cast is linked. Returns job and preparation notes.",
        json!({"shot_id":string()}),
        vec!["shot_id"],
    );
    let fact = json!({"type":"object","additionalProperties":false,"properties":{"key":string(),"value":string()},"required":["key","value"]});
    let change = json!({"type":"object","additionalProperties":false,"properties":{"key":string(),"from":{"type":["string","null"]},"to":string()},"required":["key","from","to"]});
    let outline = json!({"type":"object","additionalProperties":false,"properties":{"episode_id":string(),"goal":string(),"conflict":string(),"turn":string(),"payoff":string(),"hook":string(),"ending_resolution":string(),"requires":{"type":"array","items":fact},"changes":{"type":"array","items":change}},"required":["episode_id","goal","conflict","turn","payoff","hook","ending_resolution","requires","changes"]});
    let arc = json!({"type":"object","additionalProperties":false,"properties":{"character_id":string(),"desire":string(),"flaw":string(),"transformation":string()},"required":["character_id","desire","flaw","transformation"]});
    let thread = json!({"type":"object","additionalProperties":false,"properties":{"setup_episode_id":string(),"payoff_episode_id":string(),"setup":string(),"payoff":string()},"required":["setup_episode_id","payoff_episode_id","setup","payoff"]});
    let plan = json!({"type":"object","additionalProperties":false,"properties":{"series_id":string(),"revision":{"type":"integer","minimum":0},"audience":string(),"genre":string(),"theme":string(),"central_question":string(),"ending":string(),"creative":{"type":"object","additionalProperties":false,"properties":{"style":{"type":"string","enum":["character_drama","slice_of_life","romance","thriller","melodrama","custom"]},"casting_brief":string(),"min_shot_seconds":{"type":"number","minimum":0.5},"max_shot_seconds":{"type":"number","maximum":30},"episode_seconds":{"type":"number","minimum":15,"maximum":600},"target_episodes":{"type":"integer","minimum":1,"maximum":150}},"required":["style","casting_brief","min_shot_seconds","max_shot_seconds","episode_seconds"]},"characters":{"type":"array","items":arc},"initial_facts":{"type":"array","items":fact},"episodes":{"type":"array","items":outline},"threads":{"type":"array","items":thread}},"required":["series_id","revision","audience","genre","theme","central_question","ending","characters","initial_facts","episodes","threads"]});
    add(
        "get_series_plan",
        "Read the series bible, cast arcs, ordered episode outlines, declared continuity and setup/payoff map. Missing plan returns revision 0.",
        json!({"series_id":string()}),
        vec!["series_id"],
    );
    add(
        "save_series_plan",
        "Save a development draft with optimistic revision checking; immutable history preserved. Any revision invalidates prior canon approvals. Empty drafts are allowed; get_continuity reports gaps. Create cast/episodes first and link their stable IDs.",
        json!({"plan":plan}),
        vec!["plan"],
    );
    add(
        "get_continuity",
        "Validate declared story facts and show projected entry/exit states separately from committed canon. Stale media or predecessor changes invalidate downstream commits. This cannot prove literary or visual quality.",
        json!({"series_id":string()}),
        vec!["series_id"],
    );
    add(
        "commit_episode",
        "After editorial review, commit a delivered episode's declared changes to canon. Requires complete series plan, valid fact transitions, preceding committed episodes, and an approved final export matching current screenplay, timeline and cast/location context.",
        json!({"series_id":string(),"episode_id":string(),"export_id":string()}),
        vec!["series_id", "episode_id", "export_id"],
    );
    list
}
fn required<'a>(a: &'a Value, key: &str) -> Result<&'a str> {
    a[key]
        .as_str()
        .with_context(|| format!("'{key}' must be a string"))
}
fn optional<'a>(a: &'a Value, key: &str) -> Result<&'a str> {
    match a.get(key) {
        None => Ok(""),
        Some(v) => v
            .as_str()
            .with_context(|| format!("'{key}' must be a string")),
    }
}
fn dispatch(path: PathBuf, name: String, a: Value) -> Result<Value> {
    let s = Store::open(path)?;
    let req = |key| need(&a, key);
    fn need<'a>(a: &'a Value, k: &str) -> Result<&'a str> {
        required(a, k)
    }
    Ok(match name.as_str() {
        "dramafactory_get_shot_direction" => {
            s.shot(req("shot_id")?)?;
            json!(s.shot_direction(req("shot_id")?)?)
        }
        "dramafactory_save_shot_direction" => json!(s.save_shot_direction(
            req("shot_id")?,
            &serde_json::from_value::<crate::directing::ShotDirection>(a["direction"].clone())?
        )?),
        "dramafactory_list_camera_setups" => json!(s.camera_setups(req("location_id")?)?),
        "dramafactory_save_camera_setup" => {
            let mut value = a.clone();
            if value.get("id").is_none() {
                value["id"] = json!(uuid::Uuid::new_v4().to_string());
            }
            let camera: crate::directing::CameraSetup = serde_json::from_value(value)?;
            json!(s.save_camera_setup(&camera)?)
        }
        "dramafactory_prepare_shot_render" => json!(s.prepare_shot_render(req("shot_id")?)?),
        "dramafactory_render_shot" => {
            let prepared = s.prepare_shot_render(req("shot_id")?)?;
            let job = s.enqueue(&prepared.request)?;
            if job.status == "queued" {
                crate::worker::spawn_detached(&s.path)?;
            }
            json!({"job":job,"notes":prepared.notes})
        }
        "dramafactory_get_series_plan" => json!(s.series_plan(req("series_id")?)?),
        "dramafactory_save_series_plan" => json!(s.save_series_plan(&serde_json::from_value::<
            crate::story::SeriesPlan,
        >(
            a["plan"].clone()
        )?)?),
        "dramafactory_get_continuity" => json!(s.story_report(req("series_id")?)?),
        "dramafactory_commit_episode" => {
            json!(s.commit_episode(req("series_id")?, req("episode_id")?, req("export_id")?)?)
        }
        "dramafactory_import_audio" => json!(s.import_audio(std::path::Path::new(req("path")?))?),
        "dramafactory_list_audio_cues" => json!(s.audio_cues(req("episode_id")?)?),
        "dramafactory_remove_audio_cue" => {
            let expected = serde_json::from_value(
                a.get("expected")
                    .cloned()
                    .context("Expected saved cue required")?,
            )?;
            s.remove_audio_cue(req("id")?, &expected)?;
            json!({"removed":true})
        }
        "dramafactory_save_audio_cue" => {
            let mut value = a.clone();
            if value.get("id").is_none() {
                value["id"] = json!(uuid::Uuid::new_v4().to_string());
            }
            let expected = value
                .as_object_mut()
                .and_then(|value| value.remove("expected"))
                .map(serde_json::from_value::<crate::sound::AudioCue>)
                .transpose()?;
            let cue: crate::sound::AudioCue = serde_json::from_value(value)?;
            s.save_audio_cue(&cue, expected.as_ref())?;
            json!(cue)
        }
        "dramafactory_list_series" => json!(s.series()?),
        "dramafactory_create_series" => {
            json!(s.create_series(req("name")?, optional(&a, "premise")?)?)
        }
        "dramafactory_create_episode" => json!(s.create_episode(req("series_id")?, req("title")?)?),
        "dramafactory_list_episodes" => json!(s.episodes(req("series_id")?)?),
        "dramafactory_save_screenplay" => {
            if a.get("expected_text").is_some() {
                s.save_screenplay_if_unchanged(
                    req("episode_id")?,
                    req("expected_text")?,
                    req("text")?,
                )?;
            } else {
                s.save_screenplay(req("episode_id")?, req("text")?)?;
            }
            json!({"saved":true})
        }
        "dramafactory_queue_render" | "dramafactory_generate_video" => {
            let r: RenderRequest = serde_json::from_value(a)?;
            let job = s.enqueue(&r)?;
            if job.status == "queued" {
                crate::worker::spawn_detached(&s.path)?;
            }
            json!(job)
        }
        "dramafactory_list_jobs" => json!(s.jobs()?),
        "dramafactory_get_job_status" => json!(s.job(req("job_id")?)?),
        "dramafactory_cancel_job" => {
            s.cancel(req("job_id")?)?;
            json!({"cancellation_requested":true})
        }
        "dramafactory_retry_job" => {
            let id = req("job_id")?;
            s.retry(id)?;
            crate::worker::spawn_detached(&s.path)?;
            json!(s.job(id)?)
        }
        "dramafactory_create_shot" => json!(s.add_shot(
            req("episode_id")?,
            req("title")?,
            req("prompt")?,
            optional(&a, "camera")?,
            a["duration"].as_f64().context("duration must be numeric")?
        )?),
        "dramafactory_list_shots" => json!(s.shots(req("episode_id")?)?),
        "dramafactory_get_shot" => json!(s.shot(req("shot_id")?)?),
        "dramafactory_list_assets" => json!(s.assets()?),
        "dramafactory_get_asset_review" => json!(s.asset_review(req("asset_id")?)?),
        "dramafactory_get_asset_review_history" => json!(s.review_history(req("asset_id")?)?),
        "dramafactory_save_asset_review" => {
            json!(s.save_asset_review(&serde_json::from_value(a["review"].clone())?)?)
        }
        "dramafactory_review_asset" => {
            s.review(req("asset_id")?, req("review")?)?;
            json!({"saved":true})
        }
        "dramafactory_create_character" | "dramafactory_create_location" => {
            let kind = if name.ends_with("character") {
                "character"
            } else {
                "location"
            };
            json!(
                s.add_library(
                    req("series_id")?,
                    kind,
                    req("name")?,
                    optional(&a, "description")?,
                    a.get("reference")
                        .map(|_| req("reference").map(std::path::Path::new))
                        .transpose()?
                )?
            )
        }
        "dramafactory_list_characters" | "dramafactory_list_locations" => json!(s.library(
            req("series_id")?,
            if name.ends_with("characters") {
                "character"
            } else {
                "location"
            }
        )?),
        "dramafactory_export_episode" => {
            let job = s.enqueue_export(req("episode_id")?)?;
            crate::worker::spawn_detached(&s.path)?;
            json!(job)
        }
        "dramafactory_update_shot" => {
            s.update_shot(
                req("shot_id")?,
                req("title")?,
                req("prompt")?,
                optional(&a, "camera")?,
                a["duration"].as_f64().context("duration must be numeric")?,
            )?;
            json!({"saved":true})
        }
        "dramafactory_move_shot" => {
            s.move_shot(
                req("shot_id")?,
                a["offset"]
                    .as_i64()
                    .and_then(|v| i32::try_from(v).ok())
                    .context("offset must be -1 or 1")?,
            )?;
            json!({"saved":true})
        }
        "dramafactory_produce_episode_dialogue" => {
            let batch = s.produce_episode_dialogue(req("episode_id")?)?;
            if !batch.jobs.is_empty() {
                crate::worker::spawn_detached(&s.path)?;
            }
            json!(batch)
        }
        "dramafactory_export_dialogue_rehearsal" => {
            let job = s.enqueue_rehearsal(req("episode_id")?)?;
            crate::worker::spawn_detached(&s.path)?;
            json!(job)
        }
        "dramafactory_save_dialogue_draft" => {
            let mut line = a.get("line").context("line is required")?.clone();
            line.as_object_mut()
                .context("line must be an object")?
                .insert("asset_id".into(), Value::Null);
            json!(s.save_dialogue_draft(&serde_json::from_value(line)?)?)
        }
        "dramafactory_trim_shot" => {
            json!(s.trim_shot(&serde_json::from_value(a["trim"].clone())?)?)
        }
        "dramafactory_generate_performance" => {
            let job = s.enqueue_performance_window(
                req("shot_id")?,
                req("audio_asset_id")?,
                a.get("reference")
                    .map(|_| req("reference").map(std::path::Path::new))
                    .transpose()?,
                a.get("source_line_id")
                    .map(|_| req("source_line_id"))
                    .transpose()?,
            )?;
            crate::worker::spawn_detached(&s.path)?;
            json!(job)
        }
        "dramafactory_generate_dialogue" => {
            let speed = match a.get("speed") {
                None => 1.,
                Some(v) => v.as_f64().context("speed must be numeric")?,
            };
            let job = s.enqueue_dialogue(
                req("shot_id")?,
                req("text")?,
                req("voice")?,
                speed,
                a.get("line_id").map(|_| req("line_id")).transpose()?,
            )?;
            crate::worker::spawn_detached(&s.path)?;
            json!(job)
        }
        "dramafactory_set_dialogue_timing" => {
            let start = match a
                .get("timeline_start")
                .context("timeline_start is required")?
            {
                Value::Null => None,
                v => Some(
                    v.as_f64()
                        .context("timeline_start must be a number or null")?,
                ),
            };
            s.set_dialogue_timing(req("line_id")?, start)?;
            json!({"saved":true})
        }
        "dramafactory_set_caption_mode" => {
            s.set_caption_mode(
                req("episode_id")?,
                a["burn_in"].as_bool().context("burn_in must be boolean")?,
            )?;
            json!({"saved":true})
        }
        "dramafactory_list_dialogue" => json!(s.dialogue(req("shot_id")?)?),
        "dramafactory_assign_character_voice" => {
            s.set_voice(req("character_id")?, req("voice")?)?;
            json!({"saved":true})
        }
        "dramafactory_get_episode" => json!(s.get_episode(req("episode_id")?)?),
        "dramafactory_get_series" => json!(
            s.series()?
                .into_iter()
                .find(|x| x.id == req("series_id").unwrap_or(""))
                .context("Series not found")?
        ),
        "dramafactory_update_series" => {
            s.update_series(req("series_id")?, req("name")?, req("premise")?)?;
            json!({"saved":true})
        }
        "dramafactory_update_episode" => {
            s.rename_episode(req("episode_id")?, req("title")?)?;
            json!({"saved":true})
        }
        "dramafactory_assign_shot_asset" => {
            s.assign_take(req("shot_id")?, req("asset_id")?)?;
            json!({"saved":true})
        }
        "dramafactory_generate_image" => {
            let r: crate::production::ImageRequest = serde_json::from_value(a)?;
            let job = s.enqueue_image(&r)?;
            if job.status == "queued" {
                crate::worker::spawn_detached(&s.path)?;
            }
            json!(job)
        }
        "dramafactory_synchronize_lips" => {
            let r: crate::production::LipSyncRequest = serde_json::from_value(a)?;
            let job = s.enqueue_lipsync(&r)?;
            crate::worker::spawn_detached(&s.path)?;
            json!(job)
        }
        "dramafactory_update_library_entry" => json!(s.update_library_entry(
            &serde_json::from_value(a.get("edit").cloned().context("edit is required")?)?
        )?),
        "dramafactory_get_picture_fades" => json!(s.picture_fades(req("shot_id")?)?),
        "dramafactory_set_picture_fades" => json!(s.set_picture_fades(&serde_json::from_value(
            a.get("edit").cloned().context("edit is required")?
        )?)?),
        "dramafactory_get_dialogue_direction" => json!(s.dialogue_direction(req("line_id")?)?),
        "dramafactory_set_dialogue_visibility" => json!(
            s.set_dialogue_visibility(
                req("line_id")?,
                a.get("speaker_visible")
                    .and_then(Value::as_bool)
                    .context("speaker_visible is required")?,
                a.get("expected_visible")
                    .and_then(Value::as_bool)
                    .context("expected_visible is required")?
            )?
        ),
        "dramafactory_get_dialogue_timing" => json!(s.dialogue_timing(req("episode_id")?)?),
        "dramafactory_move_dialogue" => json!(s.move_dialogue(&serde_json::from_value(
            a.get("edit").cloned().context("edit is required")?
        )?)?),
        "dramafactory_find_assets" => json!(s.find_assets(&serde_json::from_value(a)?)?),
        "dramafactory_get_episode_readiness" => json!(s.episode_readiness(req("episode_id")?)?),
        "dramafactory_get_production_status" => {
            let jobs = s.jobs()?;
            json!({"series":s.series()?.len(),"assets":s.assets()?.len(),"jobs":jobs,"active":jobs.iter().filter(|j|["queued","running"].contains(&j.status.as_str())).count()})
        }
        "dramafactory_check_model_runtime" => {
            json!(crate::runtime::check(&s.config()?, req("model_id")?)?)
        }
        "dramafactory_delete_character" => {
            s.delete_character(&serde_json::from_value(
                a.get("expected").cloned().context("expected is required")?,
            )?)?;
            json!({"deleted":true,"recoverable":true,"media_preserved":true})
        }
        "dramafactory_list_deleted_characters" => json!(s.deleted_characters(req("series_id")?)?),
        "dramafactory_restore_character" => {
            s.restore_character(req("series_id")?, req("character_id")?)?;
            json!({"restored":true})
        }
        "dramafactory_get_character_profile" => json!(s.character_profile(req("character_id")?)?),
        "dramafactory_save_character_profile" => json!(s.save_character_profile(
            &serde_json::from_value(a.get("draft").cloned().context("draft is required")?)?
        )?),
        "dramafactory_save_synopsis" => {
            s.save_synopsis(&serde_json::from_value(
                a.get("draft").cloned().context("draft is required")?,
            )?)?;
            json!({"saved":true})
        }
        "dramafactory_apply_synopsis_proposal" => {
            let mut current: crate::synopsis::SynopsisDraft =
                serde_json::from_value(a.get("current").cloned().context("current is required")?)?;
            let expected = serde_json::from_value(
                a.get("expected").cloned().context("expected is required")?,
            )?;
            let edit = serde_json::from_value(a.get("edit").cloned().context("edit is required")?)?;
            let before = crate::sidekick_edit::apply(&mut current, &expected, &edit)?;
            json!({"draft":current,"undo_before":before,"saved":false})
        }
        "dramafactory_get_portrait_style" => json!({"style":s.portrait_style(req("library_id")?)?}),
        "dramafactory_save_portrait_style" => {
            let style =
                serde_json::from_value(a.get("style").cloned().context("style is required")?)?;
            s.save_portrait_style(req("library_id")?, style)?;
            json!({"saved":true,"image_generated":false})
        }
        "dramafactory_get_portrait_prompt" => {
            let id = req("library_id")?;
            let (series, kind): (String, String) = s.conn.query_row(
                "SELECT series_id,kind FROM library WHERE id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            let entry = s
                .library(&series, &kind)?
                .into_iter()
                .find(|entry| entry.id == id)
                .context("Library entry not found")?;
            json!(s.portrait_prompt(&entry)?)
        }
        "dramafactory_save_portrait_prompt" => json!(s.save_portrait_prompt(
            &serde_json::from_value(a.get("draft").cloned().context("draft is required")?)?
        )?),
        "dramafactory_generate_grok_portrait" => {
            let job = s.enqueue_grok_portrait(req("library_id")?, req("prompt")?)?;
            crate::worker::spawn_detached(&s.path)?;
            json!(job)
        }
        "dramafactory_create_character_with_prompt" => {
            let proposal = crate::character_edit::NewCharacter {
                style: a
                    .get("style")
                    .cloned()
                    .map(serde_json::from_value)
                    .transpose()?,
                profile: a
                    .get("profile")
                    .cloned()
                    .map(serde_json::from_value)
                    .transpose()?,
                name: req("name")?.into(),
                description: req("description")?.into(),
                portrait_prompt: req("portrait_prompt")?.into(),
                generate_portrait: false,
            };
            s.create_sidekick_character(req("series_id")?, req("character_id")?, &proposal)?;
            json!({"created":true,"character_id":req("character_id")?,"image_generated":false})
        }
        "dramafactory_change_user_message" => {
            let message =
                serde_json::from_value(a.get("message").cloned().context("message is required")?)?;
            let replacement = a.get("replacement").context("replacement is required")?;
            if !replacement.is_null() && !replacement.is_string() {
                bail!("replacement must be text or null");
            }
            s.change_user_message(req("series_id")?, &message, replacement.as_str())?;
            json!({"changed":true,"model_called":false})
        }
        "dramafactory_get_sidekick_messages" => json!(s.sidekick_messages(req("series_id")?)?),
        "dramafactory_select_segment_take" => {
            s.select_segment_take(
                req("episode_id")?,
                req("segment_id")?,
                req("asset_id")?,
                a["expected_asset_id"].as_str(),
            )?;
            json!({"selected":true})
        }
        "dramafactory_export_hosted_episode" => {
            let job = s.enqueue_hosted_episode_export(req("episode_id")?)?;
            crate::worker::spawn_detached(&s.path)?;
            json!(job)
        }
        "dramafactory_get_segment_jobs" => {
            json!(s.segment_generation_jobs(req("episode_id")?, req("segment_id")?)?)
        }
        "dramafactory_generate_episode_segment" => {
            let segment =
                serde_json::from_value(a.get("segment").cloned().context("segment is required")?)?;
            let job = s.enqueue_episode_segment(
                req("episode_id")?,
                a["revision"]
                    .as_u64()
                    .context("revision must be an integer")?,
                &segment,
            )?;
            if job.status == "queued" {
                crate::worker::spawn_detached(&s.path)?;
            }
            json!(job)
        }
        "dramafactory_generate_seedance_video" => {
            let job = s.enqueue_seedance(&serde_json::from_value(
                a.get("plan").cloned().context("plan is required")?,
            )?)?;
            crate::worker::spawn_detached(&s.path)?;
            json!(job)
        }
        "dramafactory_prepare_cast_reference" => {
            json!(s.prepare_cast_reference(req("episode_id")?, req("character_id")?)?)
        }
        "dramafactory_get_job_timing" => json!(s.job_timing(req("job_id")?)?),
        "dramafactory_get_generation_plan" => json!(s.generation_plan(req("episode_id")?)?),
        "dramafactory_save_generation_plan" => json!(s.save_generation_plan(
            &serde_json::from_value(a.get("plan").cloned().context("plan is required")?)?
        )?),
        "dramafactory_preview_video_plan" => {
            let plan: crate::video_plan::VideoPlan =
                serde_json::from_value(a.get("plan").cloned().context("plan is required")?)?;
            json!(plan.preview(&crate::openrouter::discover()?)?)
        }
        "dramafactory_get_openrouter_catalog" => json!(crate::openrouter::discover()?),
        "dramafactory_list_available_models" => json!(s.model_files()?),
        "dramafactory_get_model_status" => {
            let c = s.config()?;
            json!({"model":"Wan 2.2 TI2V-5B MLX","configured":c.validate().is_ok(),"readiness_error":c.validate().err().map(|e|e.to_string()),"path":c.model})
        }
        _ => bail!("Unknown tool: {name}"),
    })
}
struct Server {
    path: PathBuf,
}
impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("DramaFactory", "v007"))
    }
    fn get_tool(&self, name: &str) -> Option<Tool> {
        definitions().into_iter().find(|t| t.name == name)
    }
    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> std::result::Result<ListToolsResult, rmcp::ErrorData> {
        Ok(ListToolsResult::with_all_items(definitions()))
    }
    async fn call_tool(
        &self,
        r: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> std::result::Result<CallToolResponse, rmcp::ErrorData> {
        let path = self.path.clone();
        let result = tokio::task::spawn_blocking(move || {
            dispatch(
                path,
                r.name.to_string(),
                Value::Object(r.arguments.unwrap_or_default()),
            )
        })
        .await;
        let content = match result {
            Ok(Ok(v)) => CallToolResult::success(vec![ContentBlock::text(v.to_string())]),
            Ok(Err(e)) => CallToolResult::error(vec![ContentBlock::text(format!("{e:#}"))]),
            Err(_) => CallToolResult::error(vec![ContentBlock::text("Application service failed")]),
        };
        Ok(content.into())
    }
}
pub fn run(store: Store) -> Result<()> {
    let path = store.path.clone();
    drop(store);
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let running = Server { path }.serve(rmcp::transport::stdio()).await?;
        running.waiting().await?;
        Ok(())
    })
}
