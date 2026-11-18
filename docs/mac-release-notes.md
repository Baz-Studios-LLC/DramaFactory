## New in v018

- Play the editing timeline directly from source clips with native macOS playback, without rendering a full preview first. Fix fractional-trim video playback and keep the playhead on the playback clock.
- Dock music controls beside the viewer. Move, trim, split, duplicate, mute and delete music cues with undo/redo. Snap music trims to the playhead and video boundaries. Update volume and fades during playback.
- Create a cliffhanger ending from the viewer: choose a freeze frame and zoom target, adjust duration and movement, and optionally add a local sound hit or imported audio. Original footage is retained and the cut can be undone.
- Choose fal.ai or OpenRouter for WAN 3.0 per project, with durable provider-specific jobs. Separate fal and ElevenLabs key inputs and visible saved-key indicators.
- Organize Settings and store credentials in owner-only files outside projects, iCloud and the repository, avoiding repeated Keychain prompts across builds.
- Improve iCloud project discovery and the shared project picker.
- Fix fractional timeline save conflicts and ordinary Sidekick chat parsing. Sidekick can prepare ElevenLabs music directions and receives explicit self-contained video-continuity and prompt-budget instructions.

Download the **macOS-arm64.zip**, unzip it, and drag DramaFactory.app to Applications. Requires an Apple Silicon Mac.

**Updating:** v008 and newer can update automatically. To check immediately, open Settings → Check for updates, wait for the update to be verified and staged, then quit and reopen after installation. Builds v007 and earlier need a one-time manual installation because v008 introduced a new update-verification key.

Includes portable series projects for iCloud Drive and signed background updates. New versions download quietly and install after the app exits. Keep the app in a writable Applications folder; do not run it inside the download archive.

This is an ad-hoc signed development build, **not Apple-notarized**. macOS may require approval in System Settings → Privacy & Security on first launch.

Your projects and provider API keys are not included. Set your own keys in Settings on each Mac. Existing local data is retained. FFmpeg is required for export, thumbnails and local effects; on a new Mac, install it with `brew install ffmpeg` if it is not already available. The app detects the standard Homebrew location.
