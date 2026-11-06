// latency-key <keycode> [cmd] [alt] [shift]: one key press with modifiers at the HID tap,
// for driving a browser the measurement cannot script (Safari: close all windows,
// quit, between runs). Needs the Accessibility permission.
//
// `--only <bundle id>`: send nothing unless that app is frontmost (exit 2), so a
// key press never lands in another app (a person's browser or editor).
import AppKit
import CoreGraphics
import Foundation
var a = CommandLine.arguments
if let i = a.firstIndex(of: "--only"), i + 1 < a.count {
  let want = a[i + 1]
  let front = NSWorkspace.shared.frontmostApplication?.bundleIdentifier ?? "?"
  guard front == want else {
    FileHandle.standardError.write("latency-key: \(front) is frontmost, not \(want); nothing sent\n".data(using: .utf8)!)
    exit(2)
  }
  a.removeSubrange(i...(i + 1))
}
let code = CGKeyCode(UInt16(a[1])!)
var flags: CGEventFlags = []
if a.contains("cmd") { flags.insert(.maskCommand) }
if a.contains("alt") { flags.insert(.maskAlternate) }
if a.contains("shift") { flags.insert(.maskShift) }
let src = CGEventSource(stateID: .hidSystemState)
for down in [true, false] {
  let e = CGEvent(keyboardEventSource: src, virtualKey: code, keyDown: down)!
  e.flags = flags
  e.post(tap: .cghidEventTap)
  usleep(30000)
}
