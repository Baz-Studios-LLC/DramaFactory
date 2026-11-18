import Foundation
import AVFoundation
import CoreImage
import CoreVideo

struct Clip: Decodable {
    let path: String
    let start: Double
    let duration: Double
    let gap_before: Double
    let audio_gain_db: Double
    let audio_muted: Bool
    let audio_fade_in: Double
    let audio_fade_out: Double
}
struct Cue: Decodable {
    let path: String
    let start: Double
    let source_start: Double
    let duration: Double
    let gain_db: Double
    let muted: Bool?
    let fade_in: Double
    let fade_out: Double
}
struct Manifest: Decodable {
    let clips: [Clip]
    let cues: [Cue]
    let width: Int
    let height: Int
    let origin: Double
}
struct Command: Decodable {
    let command: String
    let manifest: Manifest?
}
func time(_ seconds: Double) -> CMTime { CMTime(seconds: seconds, preferredTimescale: 60000) }
let stdout = FileHandle.standardOutput
func packet(_ tag: UInt32, _ width: UInt32, _ height: UInt32, _ seconds: Double, _ payload: Data = Data()) {
    var data = Data()
    for var value in [tag.littleEndian, UInt32(payload.count).littleEndian, width.littleEndian, height.littleEndian] {
        withUnsafeBytes(of: &value) { data.append(contentsOf: $0) }
    }
    var timestamp = seconds.bitPattern.littleEndian
    withUnsafeBytes(of: &timestamp) { data.append(contentsOf: $0) }
    data.append(payload)
    stdout.write(data)
}
func fail(_ message: String) { packet(3, 0, 0, 0, Data(message.utf8)) }

final class TimelinePlayer {
    let player = AVPlayer()
    let context = CIContext(options: [.cacheIntermediates: false])
    var output: AVPlayerItemVideoOutput?
    var manifest: Manifest?
    var wantedPlaying = true
    var seeking = false
    var ended = false
    var width = 854
    var height = 480
    var audioTracks: [(CMPersistentTrackID, Double, Double, Double, Bool, Double, Double)] = []
    var composition: AVMutableComposition?
    var audioOrigins: [(Bool, Int)] = []
    var timer: Timer?
    var lastVideo = Date()

    var lastNativeFrame = Date()
    var fallbackPath: String?
    var fallbackGenerator: AVAssetImageGenerator?
    var lastFallbackTime = -1.0

    // Some compositions advance their audio clock after a seek without vending video buffers.
    // Decode from the active source at the same clock position until native output resumes.
    func sourceFrame(at seconds: Double) -> CIImage? {
        guard let manifest = manifest else { return nil }
        var cursor = 0.0
        for clip in manifest.clips {
            cursor += clip.gap_before
            if seconds < cursor { return CIImage(color: .black) }
            if seconds < cursor + clip.duration {
                if fallbackPath != clip.path {
                    let generator = AVAssetImageGenerator(asset: AVURLAsset(url: URL(fileURLWithPath: clip.path)))
                    generator.appliesPreferredTrackTransform = true
                    generator.maximumSize = CGSize(width: width, height: height)
                    generator.requestedTimeToleranceBefore = time(1.0 / 24)
                    generator.requestedTimeToleranceAfter = time(1.0 / 24)
                    fallbackGenerator = generator
                    fallbackPath = clip.path
                }
                guard let frame = try? fallbackGenerator?.copyCGImage(at: time(clip.start + seconds - cursor), actualTime: nil) else { return nil }
                let image = CIImage(cgImage: frame)
                let scale = min(CGFloat(width) / image.extent.width, CGFloat(height) / image.extent.height)
                let scaled = image.transformed(by: CGAffineTransform(scaleX: scale, y: scale))
                return scaled.transformed(by: CGAffineTransform(translationX: (CGFloat(width) - scaled.extent.width) / 2, y: (CGFloat(height) - scaled.extent.height) / 2)).composited(over: CIImage(color: .black))
            }
            cursor += clip.duration
        }
        return nil
    }

    func mix(_ tracks: [(CMPersistentTrackID, Double, Double, Double, Bool, Double, Double)]) -> AVAudioMix {
        let mix = AVMutableAudioMix()
        mix.inputParameters = tracks.map { id, start, duration, gain, muted, fadeIn, fadeOut in
            let p = AVMutableAudioMixInputParameters()
            p.trackID = id
            let volume: Float = muted ? 0 : Float(pow(10, gain / 20))
            let fadeIn = min(fadeIn, duration)
            let fadeOut = min(fadeOut, max(0, duration - fadeIn))
            p.setVolume(volume, at: time(start))
            if fadeIn > 0 { p.setVolumeRamp(fromStartVolume: 0, toEndVolume: volume, timeRange: CMTimeRange(start: time(start), duration: time(fadeIn))) }
            if fadeOut > 0 { p.setVolumeRamp(fromStartVolume: volume, toEndVolume: 0, timeRange: CMTimeRange(start: time(start + duration - fadeOut), duration: time(fadeOut))) }
            return p
        }
        return mix
    }

    func shape(_ m: Manifest) -> [String] {
        return ["\(m.width)x\(m.height)"] + m.clips.map { "\($0.path)|\($0.start)|\($0.duration)|\($0.gap_before)" } + ["music"] + m.cues.map { "\($0.path)|\($0.start)|\($0.source_start)|\($0.duration)" }
    }
    func load(_ m: Manifest, preservePosition: Bool) throws {
        if preservePosition, let old = manifest, shape(old) == shape(m) {
            audioTracks = zip(audioTracks, audioOrigins).map { track, source in
                let (id, start, duration, _, _, _, _) = track
                if source.0 {
                    let cue = m.cues[source.1]
                    return (id, start, duration, cue.gain_db, cue.muted ?? false, cue.fade_in, cue.fade_out)
                }
                let clip = m.clips[source.1]
                return (id, start, duration, clip.audio_gain_db, clip.audio_muted, clip.audio_fade_in, clip.audio_fade_out)
            }
            player.currentItem?.audioMix = mix(audioTracks)
            manifest = m
            return
        }
        let position = preservePosition ? CMTimeGetSeconds(player.currentTime()) : m.origin
        width = m.width; height = m.height
        let composition = AVMutableComposition()
        var instructions: [AVVideoCompositionInstructionProtocol] = []
        var tracks: [(CMPersistentTrackID, Double, Double, Double, Bool, Double, Double)] = []
        var origins: [(Bool, Int)] = []
        var cursor = 0.0
        for (index, clip) in m.clips.enumerated() {
            if clip.gap_before > 0 {
                let gap = AVMutableVideoCompositionInstruction()
                gap.timeRange = CMTimeRange(start: time(cursor), duration: time(clip.gap_before))
                gap.backgroundColor = CGColor(gray: 0, alpha: 1)
                gap.layerInstructions = []
                instructions.append(gap)
                cursor = CMTimeGetSeconds(CMTimeAdd(time(cursor), time(clip.gap_before)))
            }
            let asset = AVURLAsset(url: URL(fileURLWithPath: clip.path))
            guard let source = asset.tracks(withMediaType: .video).first,
                  let video = composition.addMutableTrack(withMediaType: .video, preferredTrackID: kCMPersistentTrackID_Invalid) else {
                throw NSError(domain: "Timeline", code: 1, userInfo: [NSLocalizedDescriptionKey: "Video track unavailable: \(URL(fileURLWithPath: clip.path).lastPathComponent)"])
            }
            try video.insertTimeRange(CMTimeRange(start: time(clip.start), duration: time(clip.duration)), of: source, at: time(cursor))
            let layer = AVMutableVideoCompositionLayerInstruction(assetTrack: video)
            let bounds = CGRect(origin: .zero, size: source.naturalSize).applying(source.preferredTransform)
            let scale = min(CGFloat(width) / abs(bounds.width), CGFloat(height) / abs(bounds.height))
            let transform = source.preferredTransform
                .concatenating(CGAffineTransform(translationX: -bounds.minX, y: -bounds.minY))
                .concatenating(CGAffineTransform(scaleX: scale, y: scale))
                .concatenating(CGAffineTransform(translationX: (CGFloat(width) - abs(bounds.width) * scale) / 2, y: (CGFloat(height) - abs(bounds.height) * scale) / 2))
            layer.setTransform(transform, at: time(cursor))
            let instruction = AVMutableVideoCompositionInstruction()
            instruction.timeRange = CMTimeRange(start: time(cursor), duration: time(clip.duration))
            instruction.backgroundColor = CGColor(gray: 0, alpha: 1)
            instruction.layerInstructions = [layer]
            instructions.append(instruction)
            if let sourceAudio = asset.tracks(withMediaType: .audio).first,
               let audio = composition.addMutableTrack(withMediaType: .audio, preferredTrackID: kCMPersistentTrackID_Invalid) {
                let audioLength = min(clip.duration, max(0, CMTimeGetSeconds(sourceAudio.timeRange.end) - clip.start))
                if audioLength > 0 { try audio.insertTimeRange(CMTimeRange(start: time(clip.start), duration: time(audioLength)), of: sourceAudio, at: time(cursor)) }
                origins.append((false, index))
                tracks.append((audio.trackID, cursor, clip.duration, clip.audio_gain_db, clip.audio_muted, clip.audio_fade_in, clip.audio_fade_out))
            }
            cursor = CMTimeGetSeconds(CMTimeAdd(time(cursor), time(clip.duration)))
        }
        for (index, cue) in m.cues.enumerated() where cue.start < cursor {
            let asset = AVURLAsset(url: URL(fileURLWithPath: cue.path))
            guard let source = asset.tracks(withMediaType: .audio).first,
                  let audio = composition.addMutableTrack(withMediaType: .audio, preferredTrackID: kCMPersistentTrackID_Invalid) else {
                throw NSError(domain: "Timeline", code: 2, userInfo: [NSLocalizedDescriptionKey: "Music track unavailable"])
            }
            let length = min(cue.duration, cursor - cue.start)
            try audio.insertTimeRange(CMTimeRange(start: time(cue.source_start), duration: time(length)), of: source, at: time(cue.start))
            origins.append((true, index))
            tracks.append((audio.trackID, cue.start, cue.duration, cue.gain_db, cue.muted ?? false, cue.fade_in, cue.fade_out))
        }
        let videoComposition = AVMutableVideoComposition()
        videoComposition.renderSize = CGSize(width: width, height: height)
        videoComposition.frameDuration = CMTime(value: 1, timescale: 24)
        videoComposition.instructions = instructions
        let item = AVPlayerItem(asset: composition)
        item.videoComposition = videoComposition
        item.audioMix = mix(tracks)
        let output = AVPlayerItemVideoOutput(pixelBufferAttributes: [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA])
        item.add(output)
        self.output = output
        self.composition = composition
        self.audioTracks = tracks
        self.audioOrigins = origins
        self.manifest = m
        ended = false
        lastVideo = Date()
        lastNativeFrame = Date()
        lastFallbackTime = -1
        fallbackPath = nil
        seeking = true
        player.replaceCurrentItem(with: item)
        player.seek(to: time(max(0, min(position.isFinite ? position : 0, cursor))), toleranceBefore: .zero, toleranceAfter: .zero) { [weak self] _ in
            guard let self = self else { return }
            self.seeking = false
            if self.wantedPlaying { self.player.play() }
        }
        if timer == nil {
            timer = Timer.scheduledTimer(withTimeInterval: 1.0 / 24, repeats: true) { [weak self] _ in self?.tick() }
        }
    }
    func tick() {
        guard let item = player.currentItem, let output = output else { return }
        if item.status == .failed { fail(item.error?.localizedDescription ?? "Native playback failed"); player.pause(); timer?.invalidate(); return }
        let current = player.currentTime()
        let seconds = CMTimeGetSeconds(current)
        if seconds.isFinite { packet(4, 0, 0, seconds) }
        if wantedPlaying && !seeking && !ended && Date().timeIntervalSince(lastVideo) > 8 {
            player.pause(); wantedPlaying = false
            fail("Video frames stopped arriving. Playback paused; reopen the timeline to retry.")
            return
        }
        if !seeking && seconds.isFinite && CMTimeGetSeconds(item.duration).isFinite && seconds >= CMTimeGetSeconds(item.duration) - 0.02 && !ended {
            ended = true; wantedPlaying = false; player.pause(); packet(2, 0, 0, seconds)
        }
        let frame: CIImage?
        if output.hasNewPixelBuffer(forItemTime: current), let buffer = output.copyPixelBuffer(forItemTime: current, itemTimeForDisplay: nil) {
            lastNativeFrame = Date()
            frame = CIImage(cvPixelBuffer: buffer)
        } else if wantedPlaying && !seeking && seconds.isFinite && Date().timeIntervalSince(lastNativeFrame) > 0.2 && abs(seconds - lastFallbackTime) >= 1.0 / 30 {
            lastFallbackTime = seconds
            frame = sourceFrame(at: seconds)
        } else {
            frame = nil
        }
        if let image = frame {
            lastVideo = Date()
            var pixels = Data(count: width * height * 4)
            pixels.withUnsafeMutableBytes { bytes in
                context.render(image, toBitmap: bytes.baseAddress!, rowBytes: width * 4, bounds: CGRect(x: 0, y: 0, width: width, height: height), format: .RGBA8, colorSpace: CGColorSpaceCreateDeviceRGB())
            }
            packet(1, UInt32(width), UInt32(height), seconds, pixels)
        }
    }
    func command(_ command: Command) {
        do {
            switch command.command {
            case "load": if let m = command.manifest { try load(m, preservePosition: false) }
            case "update": if let m = command.manifest { try load(m, preservePosition: true) }
            case "pause": wantedPlaying = false; player.pause()
            case "play": wantedPlaying = true; lastVideo = Date(); player.play()
            default: break
            }
        } catch { fail(error.localizedDescription) }
    }
}
let engine = TimelinePlayer()
DispatchQueue.global().async {
    while let line = readLine() {
        do {
            let command = try JSONDecoder().decode(Command.self, from: Data(line.utf8))
            DispatchQueue.main.async { engine.command(command) }
        } catch { DispatchQueue.main.async { fail(error.localizedDescription) } }
    }
    DispatchQueue.main.async { exit(0) }
}
RunLoop.main.run()
