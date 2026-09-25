// 列出指定进程 PID 拥有的屏幕窗口（CGWindowList），以 JSON 输出：用于普通包启动证据。
// 用法：swift window-info.swift <pid>
import CoreGraphics
import Foundation

guard CommandLine.arguments.count >= 2, let pid = Int32(CommandLine.arguments[1]) else {
    FileHandle.standardError.write("用法：window-info.swift <pid>\n".data(using: .utf8)!)
    exit(2)
}

let options: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
let list = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] ?? []
var windows: [[String: Any]] = []
for info in list {
    guard let ownerPid = info[kCGWindowOwnerPID as String] as? Int32, ownerPid == pid else { continue }
    let bounds = info[kCGWindowBounds as String] as? [String: Any] ?? [:]
    windows.append([
        "windowId": info[kCGWindowNumber as String] as? Int ?? -1,
        "ownerName": info[kCGWindowOwnerName as String] as? String ?? "",
        "title": info[kCGWindowName as String] as? String ?? "",
        "layer": info[kCGWindowLayer as String] as? Int ?? 0,
        "bounds": bounds,
    ])
}
let data = try JSONSerialization.data(withJSONObject: windows, options: [.prettyPrinted, .sortedKeys])
print(String(data: data, encoding: .utf8)!)
