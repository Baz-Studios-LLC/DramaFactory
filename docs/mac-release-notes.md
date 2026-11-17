Download the **macOS-arm64.zip**, unzip it, and drag DramaFactory.app to Applications. Requires an Apple Silicon Mac.

**One-time installation:** this release introduces a new update-verification key. Install it manually on each Mac once to enable future automatic updates.

Includes portable series projects for iCloud Drive and signed background updates. New versions download quietly and install after the app exits. Keep the app in a writable Applications folder; do not run it inside the download archive.

This is an ad-hoc signed development build, **not Apple-notarized**. macOS may require approval in System Settings → Privacy & Security on first launch.

Your projects and OpenRouter API key are not included. Set your own key in Settings on each Mac. Existing local data is retained. FFmpeg is required for video playback and export; on a new Mac, install it with `brew install ffmpeg` if it is not already available. The app detects the standard Homebrew location.
