## New in this release

- Avoid provider schema-state-limit errors for character replies by using JSON mode with strict local validation.

- Added right-click Copy/Paste menus for portraits, shared text forms, character profiles and Sidekick.
- Sidekick can create up to twelve complete cast members in one reply, with duplicate-name and complete-profile validation.

- Preserve portrait aspect ratios in character cards and generation cast selection; show the full image without stretching.

- Added Use my image and portrait drag-and-drop to existing character cards, so you can import your own portrait without generating one.

- Improved Cmd+V image paste in Sidekick and removed the extra Paste image button.
- Added a Stop button for pending Sidekick replies.
- Added reference-image drag and drop, previews and validation to character/location creation.
- Updated Sidekick instructions to apply requested form edits immediately without an extra conversational confirmation.

Download the **macOS-arm64.zip**, unzip it, and drag DramaFactory.app to Applications. Requires an Apple Silicon Mac.

**Updating:** v008 and newer can update automatically. To check immediately, open Settings → Check for updates, wait for the update to be verified and staged, then quit and reopen after installation. Builds v007 and earlier need a one-time manual installation because v008 introduced a new update-verification key.

Includes portable series projects for iCloud Drive and signed background updates. New versions download quietly and install after the app exits. Keep the app in a writable Applications folder; do not run it inside the download archive.

This is an ad-hoc signed development build, **not Apple-notarized**. macOS may require approval in System Settings → Privacy & Security on first launch.

Your projects and OpenRouter API key are not included. Set your own key in Settings on each Mac. Existing local data is retained. FFmpeg is required for video playback and export; on a new Mac, install it with `brew install ffmpeg` if it is not already available. The app detects the standard Homebrew location.
