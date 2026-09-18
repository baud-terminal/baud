// Post plain letters to `baud` as CGEvents stamped with the mach host-time
// clock, for scripts/key-latency.py. This is the *only* thing in the row-0
// instrument that touches the desktop, so it is guarded hard.
//
// WHAT IT DOES
//   For each key: it checks that baud owns the frontmost ordinary window AND
//   is the frontmost application and, only then, builds a key-down CGEvent
//   for one letter, stamps it with clock_gettime_nsec_np(CLOCK_UPTIME_RAW)
//   -- the same clock baud reads for NSEvent.timestamp -- prints
//   `post <seq> <ns> <char>`, and posts it; 1 ms later, after the guard has
//   run AGAIN, the matching key-up; then it sleeps the next inter-key gap
//   (jittered, below). The driver pairs `post` lines with the app's
//   `keylat:` lines by order to recover the per-key legs.
//
// SAFETY, and why each line is here
//   - GUARD, before every post -- the key-down AND the key-up: the frontmost
//     layer-0 window's owner PID must equal <pid>, and so must
//     NSWorkspace.shared.frontmostApplication's (posted HID events go to the
//     active application's key window, which can differ from the front
//     window's owner). If either differs it prints `REFUSED front window
//     owner <pid'> <name> front app <pid''> <name> posted <k>` and exits 3,
//     having posted nothing further: <k> keys were posted before the
//     refusal (0 when the first guard refuses). A key can only ever reach
//     a window baud owns.
//   - Plain letters only. Every character in <chars> must be an ASCII letter;
//     anything else exits 2 before posting. It NEVER posts Return, a modifier,
//     or any control key -- there is no virtualKey path here, only
//     keyboardSetUnicodeString with a validated letter, and every event's
//     `flags` is set to [] explicitly so a posted letter carries no modifier
//     of its own. A modifier PHYSICALLY held on the keyboard during a sweep
//     is outside this guard.
//   - Activation is best-effort and is not the gate: `activate()` from an
//     unbundled helper is refused by focus-stealing prevention on this box,
//     so the driver raises baud through System Events before running this
//     and polls `--front`; the guard above is what enforces safety.
//
// RATE, and why the gap is jittered
//   <gap_ms> is the MEAN inter-key gap. Each gap after a key is drawn
//   uniformly from [gap - gap/4, gap + gap/4] (`gapSchedule`) by a
//   SplitMix64 generator seeded with <pid>, so the same pid replays the
//   same schedule and the driver, which prints the pid, can reproduce a
//   run. A FIXED gap can phase-lock to the display refresh: Hume's
//   light-sensor tester (`[[source: hume-latency-tester]]`) drove keys at
//   a fixed delay and read a false SD of 1 ms, because every key then met
//   the same point of the display period and the vsync term stopped
//   varying. The jitter is what keeps the driver's `vsync`/`total` noise
//   floor honest; the `post` stamps are real either way, so nothing in
//   the driver's analysis depends on the schedule. One line,
//   `jitter seed <pid> mean <gap_ms> ms range [<lo>-<hi>] ms`, precedes
//   the first post. `--gap 0` stays exactly 0 and is allowed, so typeahead
//   (several keys in one frame) is reachable; the floor on the inter-key
//   interval is then the 1 ms key-up plus the guard's own
//   CGWindowListCopyWindowInfo call, twice a key, not a sleep.
//
// MODES that post nothing
//   --info   prints `refresh <hz>`: the main display's refresh rate from
//            CGDisplayCopyDisplayMode(CGMainDisplayID()).refreshRate (0 when
//            the mode reports none, which some built-in panels do).
//   --front  prints `front <pid> <name> app <pid> <name>`: the frontmost
//            layer-0 window's owner and the frontmost application, the two
//            things the guard requires to be baud's.
//   --gaps <pid> <count> <gap_ms>
//            prints the `jitter` line and the gap schedule a posting run
//            with those arguments would sleep, `gap <k> <ms>` per key after
//            the first. This is the jitter's self-test: the same arguments
//            twice print the same lines, every gap lies inside the range,
//            and a mean of 0 prints only zeros.
//
// Run it the way scripts/winid.swift documents (this box's PATH `swift` is a
// nix xcode-select shim with no compiler):
//   DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer \
//     swift -sdk <MacOSX.sdk> scripts/key-post.swift <pid> <count> <gap_ms> <chars>
//
// EXIT CODES
//   0  posted <count> keys to baud's window (or a --info/--front/--gaps query)
//   2  bad arguments, or <chars> contains a non-letter
//   3  REFUSED: baud did not own the front window or was not the front
//      application; nothing further was posted (the line says how many were)
import AppKit
import CoreGraphics
import Foundation

// stdout unbuffered: the driver reads `post` lines as they happen.
setbuf(stdout, nil)

func die(_ msg: String, _ code: Int32) -> Never {
    FileHandle.standardError.write((msg + "\n").data(using: .utf8)!)
    exit(code)
}

func hostNow() -> UInt64 { clock_gettime_nsec_np(CLOCK_UPTIME_RAW) }

// Sleep to a deadline on the host clock, spinning the last stretch. A plain
// `Thread.sleep` wakes when the kernel's timer does, and the kernel coalesces
// a sleep's wakeup with other timers inside its leeway -- under a display-paced
// load that pinned every post to one phase of the refresh: with a child
// printing a line every 2 ms the posts' phase spread 1.6 ms of an 8.3 ms
// period where the idle `cat` child gave the whole period, so the jittered
// gap was a fixed one in the workload where the phase decides the number.
// Sleeping to 3 ms short and spinning the rest lands the post at the schedule
// rather than at the nearest timer.
func sleepUntil(_ deadline: UInt64) {
    let slack: UInt64 = 3_000_000
    let now = hostNow()
    if deadline > now + slack {
        Thread.sleep(forTimeInterval: Double(deadline - now - slack) / 1e9)
    }
    while hostNow() < deadline {}
}

// A seedable generator (SplitMix64) so the gap schedule is a pure function
// of the seed: the same pid gives the same schedule. Swift's
// SystemRandomNumberGenerator cannot be seeded, and a run could not be
// replayed from it.
struct SplitMix64: RandomNumberGenerator {
    var state: UInt64
    init(seed: UInt64) { state = seed }
    mutating func next() -> UInt64 {
        state &+= 0x9E37_79B9_7F4A_7C15
        var z = state
        z = (z ^ (z >> 30)) &* 0xBF58_476D_1CE4_E5B9
        z = (z ^ (z >> 27)) &* 0x94D0_49BB_1331_11EB
        return z ^ (z >> 31)
    }
}

// The fraction of the mean each gap may move either way. The driver's
// GAP_JITTER must agree (it sizes the pairing window from the minimum).
let jitterFraction = 0.25

// The inter-key gaps in ms, one per key after the first: uniform in
// [mean - mean/4, mean + mean/4]; a mean of 0 stays exactly 0 (typeahead).
func gapSchedule(mean: Int, count: Int, seed: UInt64) -> [Double] {
    var rng = SplitMix64(seed: seed)
    let m = Double(mean)
    let q = m * jitterFraction
    return (1..<max(count, 1)).map { _ in
        mean == 0 ? 0.0 : Double.random(in: (m - q)...(m + q), using: &rng)
    }
}

func seedOf(_ pid: Int32) -> UInt64 { UInt64(bitPattern: Int64(pid)) }

func jitterLine(pid: Int32, mean: Int) -> String {
    let m = Double(mean)
    return "jitter seed \(pid) mean \(mean) ms range "
        + "[\(m - m * jitterFraction)-\(m + m * jitterFraction)] ms"
}

// The frontmost ordinary (layer-0) window's owner pid and name, or nil.
func frontOwner() -> (pid: Int32, name: String)? {
    let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID)
        as? [[String: Any]] ?? []
    for w in list where (w["kCGWindowLayer"] as? Int) == 0 {
        return (w["kCGWindowOwnerPID"] as? Int32 ?? -1,
                w["kCGWindowOwnerName"] as? String ?? "?")
    }
    return nil
}

// The frontmost application's pid and name (-1, "?" when none).
func frontApp() -> (pid: Int32, name: String) {
    let app = NSWorkspace.shared.frontmostApplication
    return (app?.processIdentifier ?? -1, app?.localizedName ?? "?")
}

let a = CommandLine.arguments

// --info: the main display's refresh rate; posts nothing.
if a.count == 2 && a[1] == "--info" {
    let hz = CGDisplayCopyDisplayMode(CGMainDisplayID())?.refreshRate ?? 0
    print("refresh \(hz)")
    exit(0)
}

// --front: who owns the front window and who is the front app; posts nothing.
if a.count == 2 && a[1] == "--front" {
    let f = frontOwner()
    let app = frontApp()
    print("front \(f?.pid ?? -1) \(f?.name ?? "?") app \(app.pid) \(app.name)")
    exit(0)
}

// --gaps: the schedule a posting run would sleep; posts nothing.
if a.count == 5 && a[1] == "--gaps" {
    guard let pid = Int32(a[2]), let count = Int(a[3]), let gapMs = Int(a[4]),
          count >= 0, gapMs >= 0 else {
        die("usage: key-post.swift --gaps <pid> <count> <gap_ms>", 2)
    }
    print(jitterLine(pid: pid, mean: gapMs))
    for (k, g) in gapSchedule(mean: gapMs, count: count, seed: seedOf(pid)).enumerated() {
        print("gap \(k + 1) \(g)")
    }
    exit(0)
}

guard a.count == 5, let pid = Int32(a[1]), let count = Int(a[2]),
      let gapMs = Int(a[3]), count >= 0, gapMs >= 0 else {
    die("usage: key-post.swift <pid> <count> <gap_ms> <chars> | --info | --front"
        + " | --gaps <pid> <count> <gap_ms>", 2)
}
let chars = Array(a[4])
guard !chars.isEmpty else { die("no chars", 2) }
// Plain letters only -- never a control key, never Return.
for c in chars {
    guard c.isASCII, c.isLetter else {
        die("refusing non-letter char '\(c)': plain letters only", 2)
    }
}

// How many key-downs have been posted, so a refusal can say what preceded it.
var posted = 0

// The inter-key gaps, drawn once from the pid so the run is reproducible;
// said before the first post so the driver's capture carries it.
let gaps = gapSchedule(mean: gapMs, count: count, seed: seedOf(pid))
print(jitterLine(pid: pid, mean: gapMs))

// The guard. Called before every single post -- the key-down and the key-up;
// refusing exits 3 with nothing further posted. This is the whole safety
// contract: keys reach baud's window or no window at all.
func guardFront() {
    let f = frontOwner()
    let app = frontApp()
    if let f = f, f.pid == pid, app.pid == pid { return }
    print("REFUSED front window owner \(f?.pid ?? -1) \(f?.name ?? "?") "
          + "front app \(app.pid) \(app.name) posted \(posted)")
    exit(3)
}

// Bring baud forward, then let the activation settle before the first guard.
// (The spec writes `.activate(options: .activateIgnoringOtherApps)`; that
// option is deprecated with no effect since macOS 14, so plain `activate()`
// is identical here and warning-free. Focus-stealing prevention refuses it
// for an unbundled helper anyway -- the driver activates through System
// Events first -- and the guard, not activation, is what enforces safety.)
if let app = NSRunningApplication(processIdentifier: pid) {
    _ = app.activate()
}
Thread.sleep(forTimeInterval: 0.3)

var deadline: UInt64? = nil

for i in 0..<count {
    guardFront()  // refuse before building or posting anything
    let ch = chars[i % chars.count]
    var u: [UniChar] = Array(String(ch).utf16)

    let down = CGEvent(keyboardEventSource: nil, virtualKey: 0, keyDown: true)!
    down.keyboardSetUnicodeString(stringLength: u.count, unicodeString: &u)
    down.flags = []  // a plain letter: no modifier of the event's own
    // The wait to the schedule is the LAST thing before the stamp: the guard
    // and the event's creation are round trips to the window server, and
    // under a compositing load they return in phase with its passes -- a
    // wait placed before them landed every post at one phase of the
    // refresh, whatever the schedule said.
    if let d = deadline {
        sleepUntil(d)
    }
    let stamp = hostNow()
    down.timestamp = CGEventTimestamp(stamp)
    print("post \(i + 1) \(stamp) \(ch)")
    down.post(tap: .cghidEventTap)
    posted += 1

    Thread.sleep(forTimeInterval: 0.001)  // 1 ms, then the key-up
    guardFront()  // again: the front can change in that millisecond
    let up = CGEvent(keyboardEventSource: nil, virtualKey: 0, keyDown: false)!
    up.keyboardSetUnicodeString(stringLength: u.count, unicodeString: &u)
    up.flags = []
    up.post(tap: .cghidEventTap)

    // The next post's moment on the schedule: this stamp plus the jittered
    // gap, waited for at the top of the next iteration.
    deadline = (i + 1 < count && gaps[i] > 0) ? stamp + UInt64(gaps[i] * 1_000_000.0) : nil
}
exit(0)
