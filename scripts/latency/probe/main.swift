// Iron latency probe (plans/rendering/latency-view.md §4): drives the real OS
// pointer over the Latency view and measures, from screen captures, how far
// behind the pointer each tracker is drawn.
//
//   scripts/latency/build.sh && scripts/latency/latency-probe --label web-worker-solo
//
// 1. Finds the view's yellow square on the main display (a still capture).
// 2. Moves the pointer to its centre and presses (CGEvent, needs Accessibility).
// 3. Sweeps the pointer left and right at a constant speed (a triangle wave),
//    posting drag events at a fixed rate, and logs each event's host time.
// 4. Records the band around the sweep with ScreenCaptureKit (needs Screen
//    Recording); each frame carries the host time it was displayed.
// 5. In each frame finds each tracker's centre by colour, and its lag: the
//    frame's display time minus the time the pointer was where the tracker is
//    drawn.
//
// Capture timestamps are the compositor's: the panel's scanout (a few ms) is
// not included, equally for every tracker. Output: a JSON summary on stdout
// (and --out), optionally per-frame CSV (--csv).
import AppKit
import CoreGraphics
import CoreMedia
import Foundation
import ScreenCaptureKit

// ---- arguments --------------------------------------------------------------

var args: [String: String] = [:]
do {
    var it = CommandLine.arguments.dropFirst().makeIterator()
    while let a = it.next() {
        guard a.hasPrefix("--") else { continue }
        let key = String(a.dropFirst(2))
        if let eq = key.firstIndex(of: "=") { args[String(key[..<eq])] = String(key[key.index(after: eq)...]) } else { args[key] = it.next() ?? "" }
    }
}
func num(_ k: String, _ d: Double) -> Double { args[k].flatMap(Double.init) ?? d }
let label = args["label"] ?? "run"
let speed = num("speed", 600)          // pt/s
let amplitude = num("amplitude", 180)  // pt either side of the start
let duration = num("duration", 6)      // s of sweeping
let rate = num("rate", 250)            // drag events per second
let settle = num("settle", 0.6)        // s held still before and after

// The trackers (src-ui/views/html/LatencyView.svelte TRACKERS): hue (°) and
// radius in CSS px; the square is filled.
struct Tracker { let key: String; let hue: Double; let radius: Double }
let trackers = [
    Tracker(key: "square", hue: 50.4, radius: 20),
    Tracker(key: "engine", hue: 320, radius: 34),
    Tracker(key: "canvas", hue: 120, radius: 44),
    Tracker(key: "raf", hue: 28.8, radius: 54),
    Tracker(key: "svg", hue: 265, radius: 64),
]

// ---- time -------------------------------------------------------------------

var timebase = mach_timebase_info_data_t()
mach_timebase_info(&timebase)
func seconds(_ mach: UInt64) -> Double { Double(mach) * Double(timebase.numer) / Double(timebase.denom) / 1e9 }
func machNow() -> UInt64 { mach_absolute_time() }
func machTicks(_ s: Double) -> UInt64 { UInt64(s * 1e9 * Double(timebase.denom) / Double(timebase.numer)) }

func fail(_ m: String) -> Never {
    FileHandle.standardError.write((m + "\n").data(using: .utf8)!)
    exit(1)
}

guard CGPreflightPostEventAccess() else { fail("needs Accessibility (post events) for the host app") }
guard CGPreflightScreenCaptureAccess() else { fail("needs Screen Recording for the host app") }

// ---- pixels -----------------------------------------------------------------

@inline(__always) func hueOf(_ r: Double, _ g: Double, _ b: Double) -> Double? {
    let mx = max(r, g, b), mn = min(r, g, b), d = mx - mn
    if mx < 0.55 || d / mx < 0.65 { return nil }
    var h: Double
    if mx == r { h = (g - b) / d } else if mx == g { h = (b - r) / d + 2 } else { h = (r - g) / d + 4 }
    h *= 60
    if h < 0 { h += 360 }
    return h
}
@inline(__always) func near(_ h: Double, _ t: Double) -> Bool { min(abs(h - t), 360 - abs(h - t)) <= 9 }

/// A BGRA bitmap and its origin and scale in global points.
struct Bitmap {
    let base: UnsafePointer<UInt8>
    let width: Int, height: Int, rowBytes: Int
    let origin: CGPoint, scale: Double
    @inline(__always) func hue(_ x: Int, _ y: Int) -> Double? {
        let p = base + y * rowBytes + x * 4
        return hueOf(Double(p[2]) / 255, Double(p[1]) / 255, Double(p[0]) / 255)
    }
}

/// The yellow square: the largest yellow blob about 40 pt across.
func findSquare(_ bm: Bitmap) -> CGPoint? {
    let cell = 4
    let gw = bm.width / cell, gh = bm.height / cell
    var mask = [Bool](repeating: false, count: gw * gh)
    for gy in 0..<gh { for gx in 0..<gw {
        if let h = bm.hue(gx * cell + 2, gy * cell + 2), near(h, trackers[0].hue) { mask[gy * gw + gx] = true }
    } }
    var seen = [Bool](repeating: false, count: gw * gh)
    var best: (n: Int, minX: Int, maxX: Int, minY: Int, maxY: Int)? = nil
    for first in 0..<(gw * gh) where mask[first] && !seen[first] {
        var stack = [first]; seen[first] = true
        var n = 0, minX = Int.max, maxX = 0, minY = Int.max, maxY = 0
        while let i = stack.popLast() {
            n += 1
            let x = i % gw, y = i / gw
            minX = min(minX, x); maxX = max(maxX, x); minY = min(minY, y); maxY = max(maxY, y)
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let nx = x + dx, ny = y + dy
                guard nx >= 0, ny >= 0, nx < gw, ny < gh else { continue }
                let j = ny * gw + nx
                if mask[j] && !seen[j] { seen[j] = true; stack.append(j) }
            }
        }
        let side = Double((maxX - minX + 1) * cell) / bm.scale
        if (28.0...60.0).contains(side) && n > (best?.n ?? 0) { best = (n, minX, maxX, minY, maxY) }
    }
    guard let b = best else { return nil }
    // Refine on full pixels around the blob.
    var minX = Int.max, maxX = 0, minY = Int.max, maxY = 0
    for y in max(0, b.minY * cell - cell)..<min(bm.height, (b.maxY + 2) * cell) {
        for x in max(0, b.minX * cell - cell)..<min(bm.width, (b.maxX + 2) * cell) {
            if let h = bm.hue(x, y), near(h, trackers[0].hue) { minX = min(minX, x); maxX = max(maxX, x); minY = min(minY, y); maxY = max(maxY, y) }
        }
    }
    return CGPoint(x: bm.origin.x + (Double(minX + maxX) / 2 + 0.5) / bm.scale, y: bm.origin.y + (Double(minY + maxY) / 2 + 0.5) / bm.scale)
}

/// Each tracker's horizontal centre (global pt) in the rows near `cy`.
func centres(_ bm: Bitmap, cy: Double) -> [Double] {
    let row = Int(((cy - bm.origin.y) * bm.scale).rounded())
    let band = Int((6 * bm.scale).rounded())
    var lo = [Int](repeating: Int.max, count: trackers.count), hi = [Int](repeating: -1, count: trackers.count)
    for y in max(0, row - band)...min(bm.height - 1, row + band) {
        for x in 0..<bm.width {
            guard let h = bm.hue(x, y) else { continue }
            for (i, t) in trackers.enumerated() where near(h, t.hue) {
                if x < lo[i] { lo[i] = x }
                if x > hi[i] { hi[i] = x }
            }
        }
    }
    return trackers.indices.map { i in
        hi[i] < 0 ? .nan : bm.origin.x + (Double(lo[i] + hi[i]) / 2 + 0.5) / bm.scale
    }
}

// ---- capture ----------------------------------------------------------------

let content = try await SCShareableContent.excludingDesktopWindows(false, onScreenWindowsOnly: true)
guard let display = content.displays.first(where: { $0.displayID == CGMainDisplayID() }) ?? content.displays.first else { fail("no display") }
let scale = Double(NSScreen.screens.first(where: { ($0.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] as? NSNumber)?.uint32Value == display.displayID })?.backingScaleFactor ?? 2)
let filter = SCContentFilter(display: display, excludingWindows: [])

func still() async throws -> CGImage {
    let c = SCStreamConfiguration()
    c.width = Int(Double(display.width) * scale)
    c.height = Int(Double(display.height) * scale)
    c.showsCursor = false
    c.colorSpaceName = CGColorSpace.sRGB
    return try await SCScreenshotManager.captureImage(contentFilter: filter, configuration: c)
}
func bitmap(of image: CGImage, _ body: (Bitmap) -> Void) {
    let w = image.width, h = image.height
    var data = [UInt8](repeating: 0, count: w * h * 4)
    let ctx = CGContext(data: &data, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue)!
    ctx.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
    data.withUnsafeBufferPointer { p in
        body(Bitmap(base: p.baseAddress!, width: w, height: h, rowBytes: w * 4, origin: CGPoint(x: display.frame.minX, y: display.frame.minY), scale: scale))
    }
}

// `--wait s`: keep looking that long (a page still loading).
var found: CGPoint? = nil
let deadline = Date().addingTimeInterval(num("wait", 0))
var lastStill: CGImage? = nil
repeat {
    let image = try await still()
    lastStill = image
    bitmap(of: image) { found = findSquare($0) }
    if found == nil { try await Task.sleep(nanoseconds: 500_000_000) }
} while found == nil && Date() < deadline
if found == nil, let image = lastStill, let path = args["debug"] {
    let dest = CGImageDestinationCreateWithURL(URL(fileURLWithPath: path) as CFURL, "public.png" as CFString, 1, nil)!
    CGImageDestinationAddImage(dest, image, nil)
    CGImageDestinationFinalize(dest)
}
guard let start = found else { fail("no yellow square on the main display: open the Latency view") }
FileHandle.standardError.write("square at \(start)\n".data(using: .utf8)!)

struct Frame { let t: Double; let x: [Double] }
final class Recorder: NSObject, SCStreamOutput, @unchecked Sendable {
    let lock = NSLock()
    var frames: [Frame] = []
    var recording = false
    let origin: CGPoint
    let cy: Double
    func set(_ on: Bool) -> Frame? { lock.lock(); defer { lock.unlock() }; recording = on; return frames.last }
    func all() -> [Frame] { lock.lock(); defer { lock.unlock() }; return frames }
    init(origin: CGPoint, cy: Double) { self.origin = origin; self.cy = cy }
    func stream(_ stream: SCStream, didOutputSampleBuffer sb: CMSampleBuffer, of type: SCStreamOutputType) {
        guard type == .screen, let pb = CMSampleBufferGetImageBuffer(sb) else { return }
        guard let atts = CMSampleBufferGetSampleAttachmentsArray(sb, createIfNecessary: false) as? [[SCStreamFrameInfo: Any]],
              let info = atts.first,
              let raw = info[.status] as? Int, SCFrameStatus(rawValue: raw) == .complete,
              let shown = info[.displayTime] as? UInt64 else { return }
        lock.lock(); let on = recording; lock.unlock()
        guard on else { return }
        CVPixelBufferLockBaseAddress(pb, .readOnly)
        defer { CVPixelBufferUnlockBaseAddress(pb, .readOnly) }
        let bm = Bitmap(base: UnsafePointer(CVPixelBufferGetBaseAddress(pb)!.assumingMemoryBound(to: UInt8.self)), width: CVPixelBufferGetWidth(pb), height: CVPixelBufferGetHeight(pb), rowBytes: CVPixelBufferGetBytesPerRow(pb), origin: origin, scale: scale)
        let xs = centres(bm, cy: cy)
        lock.lock(); frames.append(Frame(t: seconds(shown), x: xs)); lock.unlock()
    }
}

let margin = trackers.map(\.radius).max()! + 40
let region = CGRect(x: start.x - amplitude - margin, y: start.y - 40, width: 2 * (amplitude + margin), height: 80)
let config = SCStreamConfiguration()
config.sourceRect = CGRect(x: region.minX - display.frame.minX, y: region.minY - display.frame.minY, width: region.width, height: region.height)
config.width = Int(region.width * scale)
config.height = Int(region.height * scale)
config.minimumFrameInterval = CMTime(value: 1, timescale: 240)
config.queueDepth = 8
config.showsCursor = false
config.pixelFormat = kCVPixelFormatType_32BGRA
config.colorSpaceName = CGColorSpace.sRGB
let recorder = Recorder(origin: region.origin, cy: start.y)
let stream = SCStream(filter: filter, configuration: config, delegate: nil)
try stream.addStreamOutput(recorder, type: .screen, sampleHandlerQueue: DispatchQueue(label: "capture", qos: .userInteractive))
try await stream.startCapture()

// ---- input ------------------------------------------------------------------

let source = CGEventSource(stateID: .hidSystemState)
func post(_ type: CGEventType, _ p: CGPoint) {
    CGEvent(mouseEventSource: source, mouseType: type, mouseCursorPosition: p, mouseButton: .left)!.post(tap: .cghidEventTap)
}
struct Sample { let t: Double; let x: Double }
var samples: [Sample] = []
func wait(_ s: Double) { mach_wait_until(machNow() + machTicks(s)) }

post(.mouseMoved, CGPoint(x: start.x + 1, y: start.y)); wait(0.1)
post(.mouseMoved, start); wait(settle)
post(.leftMouseDown, start); wait(0.3)
_ = recorder.set(true)
wait(settle)

// Triangle wave about the start: right, back through it, left, back.
let period = 4 * amplitude / speed
func offset(_ t: Double) -> Double {
    let u = (t / period).truncatingRemainder(dividingBy: 1)
    if u < 0.25 { return amplitude * u * 4 }
    if u < 0.75 { return amplitude * (2 - u * 4) }
    return amplitude * (u * 4 - 4)
}
let steps = Int(duration * rate)
let t0 = machNow()
let dt = 1 / rate
for i in 0...steps {
    mach_wait_until(t0 + machTicks(Double(i) * dt))
    let x = start.x + offset(Double(i) * dt)
    let now = machNow()
    post(.leftMouseDragged, CGPoint(x: x, y: start.y))
    samples.append(Sample(t: seconds(now), x: x))
}
// Back to the start and held: every tracker should come to rest there.
wait(settle)
let rest = recorder.set(false)
post(.leftMouseUp, CGPoint(x: samples.last!.x, y: start.y))
try await stream.stopCapture()

// ---- analysis ---------------------------------------------------------------

let sweepStart = samples.first!.t, sweepEnd = samples.last!.t
/// The last event posted by host time `t`.
func lastEvent(at t: Double) -> Int? {
    guard t >= samples[0].t else { return nil }
    var lo = 0, hi = samples.count - 1
    while lo < hi { let mid = (lo + hi + 1) / 2; if samples[mid].t <= t { lo = mid } else { hi = mid - 1 } }
    return lo
}
/// The direction of travel at event `i`, and when that run began.
func run(at i: Int) -> (dir: Double, began: Double) {
    let j = max(1, i)
    let dir: Double = samples[j].x >= samples[j - 1].x ? 1 : -1
    var k = j
    while k > 1 && (samples[k - 1].x - samples[k - 2].x) * dir > 0 { k -= 1 }
    return (dir, samples[k - 1].t)
}

var lags = [[Double]](repeating: [], count: trackers.count)
var csv = "t_ms,pointer_x," + trackers.map(\.key).joined(separator: ",") + "\n"
let frames = recorder.all().sorted { $0.t < $1.t }
let maxLag = 0.25
for f in frames {
    guard let i = lastEvent(at: f.t) else { continue }
    csv += String(format: "%.3f,%.2f", (f.t - sweepStart) * 1000, samples[i].x)
    let (dir, began) = run(at: i)
    for (k, x) in f.x.enumerated() {
        csv += x.isNaN ? "," : String(format: ",%.2f", x)
        // Only frames inside the sweep, in one direction of travel for longer
        // than any lag, so the tracker's position names one moment.
        guard !x.isNaN, f.t > sweepStart + 0.3, f.t < sweepEnd, f.t - began > maxLag else { continue }
        // Walk back along this run to the last event that had not yet passed
        // where the tracker is drawn.
        var j = i
        while j > 0 && (samples[j].x - x) * dir > 0 && samples[j].t > f.t - maxLag { j -= 1 }
        guard samples[j].t > f.t - maxLag, j + 1 < samples.count else { continue }
        // Within one event step, interpolate for sub-event precision.
        let a = samples[j], b = samples[j + 1]
        let frac = b.x == a.x ? 0 : min(1, max(0, (x - a.x) / (b.x - a.x)))
        let tau = a.t + frac * (b.t - a.t)
        lags[k].append((f.t - tau) * 1000)
    }
    csv += "\n"
}

func stats(_ v: [Double]) -> [String: Double] {
    guard !v.isEmpty else { return ["n": 0] }
    let s = v.sorted()
    func q(_ p: Double) -> Double { s[min(s.count - 1, Int((Double(s.count - 1) * p).rounded()))] }
    return ["n": Double(s.count), "p10": q(0.1), "p50": q(0.5), "p90": q(0.9), "mean": s.reduce(0, +) / Double(s.count), "min": s.first!, "max": s.last!]
}
let sweepFrames = frames.filter { $0.t > sweepStart + 0.3 && $0.t < sweepEnd }
let intervals = zip(sweepFrames.dropFirst(), sweepFrames).map { ($0.t - $1.t) * 1000 }
let restOffsets: [String: Double] = rest.map { r in
    Dictionary(uniqueKeysWithValues: trackers.enumerated().map { ($1.key, r.x[$0].isNaN ? -999 : r.x[$0] - samples.last!.x) })
} ?? [:]
var summary: [String: Any] = [
    "label": label,
    "date": ISO8601DateFormatter().string(from: Date()),
    "display": ["id": display.displayID, "points": [display.width, display.height], "scale": scale, "hz": CGDisplayCopyDisplayMode(display.displayID)?.refreshRate ?? 0],
    "sweep": ["speed_pt_s": speed, "amplitude_pt": amplitude, "duration_s": duration, "rate_hz": rate, "events": samples.count],
    "frames": sweepFrames.count,
    "frame_interval_ms": stats(intervals),
    "lag_ms": Dictionary(uniqueKeysWithValues: trackers.enumerated().map { ($1.key, stats(lags[$0])) }),
    "rest_offset_pt": restOffsets,
    "note": "frame display time (ScreenCaptureKit) minus the time the OS pointer was where the tracker is drawn; excludes panel scanout",
]
if let extra = args["meta"], let d = extra.data(using: .utf8), let m = try? JSONSerialization.jsonObject(with: d) { summary["meta"] = m }
let json = try JSONSerialization.data(withJSONObject: summary, options: [.prettyPrinted, .sortedKeys])
FileHandle.standardOutput.write(json)
FileHandle.standardOutput.write("\n".data(using: .utf8)!)
if let out = args["out"] { try json.write(to: URL(fileURLWithPath: out)) }
if let path = args["csv"] { try csv.write(toFile: path, atomically: true, encoding: .utf8) }
