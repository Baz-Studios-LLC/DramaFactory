## New in v019

- Add Buffer queue publishing for TikTok, Instagram Reels and YouTube Shorts, with private local API-key storage and Cloudinary video hosting configuration. Requires connected Buffer channels and an upload preset. This integration has not yet been verified with a live publishing request.
- Give episode exports readable series and episode filenames, and retain verified, versioned copies in Movies/DramaFactory/Finished Episodes.
- Import existing music files into the editing room. Keep music controls visible, label segment/take thumbnails, and improve panel layout.
- Generate custom music tracks up to five minutes even when the video timeline is shorter or empty. Show music positions and lengths in minutes:seconds, and use the same five-minute limit for Sidekick music drafts.
- Recover video frames when native timeline playback advances its audio clock without delivering video after a seek.
- Copy highlighted Sidekick conversation text through a right-click menu.
- Offer Save As when a portable project's remembered folder is missing, and clarify revision-conflict messages.

Download the **macOS-arm64.zip**, unzip it, and drag DramaFactory.app to Applications. Requires an Apple Silicon Mac.

**Updating:** v008 and newer can update automatically. To check immediately, open Settings → Check for updates, wait for the update to be verified and staged, then quit and reopen after installation. Builds v007 and earlier need a one-time manual installation because v008 introduced a new update-verification key.

Includes portable series projects for iCloud Drive and signed background updates. New versions download quietly and install after the app exits. Keep the app in a writable Applications folder; do not run it inside the download archive.

This is an ad-hoc signed development build, **not Apple-notarized**. macOS may require approval in System Settings → Privacy & Security on first launch.

Your projects and provider API keys are not included. Set your own keys in Settings on each Mac. Existing local data is retained. FFmpeg is required for export, thumbnails and local effects; on a new Mac, install it with `brew install ffmpeg` if it is not already available. The app detects the standard Homebrew location.
