// Read-only diagnostic for a user-controlled macOS Telegram Desktop PoC.
// Never request AXTitle, AXValue, AXDescription or perform an AX action.
import AppKit
import ApplicationServices
import Darwin
import Foundation

private struct ProbeFailure: Error {}

private struct RawNode<T> {
    let pid: pid_t
    let role: String
    let actions: [String]
    let identifier: String?
    let enabled: Bool
    let selected: Bool
    let children: [T]
    let childCount: Int
    let incomplete: Bool
}

private struct NodeRecord: Encodable {
    let path: [Int]
    let role: String
    let actions: [String]
    let id_kind: String
    let enabled: Bool
    let selected: Bool
    let children_at_least: Int
}

private struct TreeReport: Encodable {
    let nodes: [NodeRecord]
    let truncated: Bool
}

private let roles: Set<String> = ["AXApplication", "AXWindow", "AXSheet", "AXGroup",
    "AXButton", "AXStaticText", "AXTextField", "AXList", "AXRow", "AXMenu",
    "AXMenuItem", "AXCheckBox", "AXRadioButton", "AXPopUpButton", "AXLink",
    "AXTabGroup", "AXProgressIndicator"]
private let actions: Set<String> = ["AXPress", "AXShowMenu", "AXConfirm",
    "AXCancel", "AXRaise"]

private func idKind(_ value: String?) -> String {
    guard let value, !value.isEmpty else { return "empty" }
    return value.unicodeScalars.allSatisfy { $0.value >= 48 && $0.value <= 57 }
        ? "numeric" : "opaque"
}

private func capture<T>(_ root: T, pid: pid_t,
                        reader: (T) throws -> RawNode<T>) throws -> TreeReport {
    var pending: [(T, [Int])] = [(root, [])]
    var records: [NodeRecord] = []
    var truncated = false
    while !pending.isEmpty && records.count < 256 {
        let (node, path) = pending.removeFirst()
        let raw = try reader(node)
        guard raw.pid == pid else {
            truncated = true
            continue
        }
        let count = raw.childCount
        records.append(NodeRecord(path: path,
            role: roles.contains(raw.role) ? raw.role : "other",
            actions: Array(Set(raw.actions.filter { actions.contains($0) })).sorted(),
            id_kind: idKind(raw.identifier), enabled: raw.enabled,
            selected: raw.selected, children_at_least: min(count, 65)))
        if raw.incomplete || count > 64 || (count > 0 && path.count >= 6) {
            truncated = true
        }
        if path.count >= 6 { continue }
        let limit = min(64, raw.children.count, 256 - records.count - pending.count)
        if count > limit { truncated = true }
        if limit > 0 {
            for index in 0..<limit { pending.append((raw.children[index], path + [index])) }
        }
    }
    if !pending.isEmpty { truncated = true }
    return TreeReport(nodes: records, truncated: truncated)
}

private func stringAttribute(_ element: AXUIElement, _ key: CFString) throws -> String? {
    var value: CFTypeRef?
    let status = AXUIElementCopyAttributeValue(element, key, &value)
    if status == .attributeUnsupported || status == .noValue { return nil }
    guard status == .success else { throw ProbeFailure() }
    return value as? String
}

private func boolAttribute(_ element: AXUIElement, _ key: CFString) throws -> Bool {
    var value: CFTypeRef?
    let status = AXUIElementCopyAttributeValue(element, key, &value)
    if status == .attributeUnsupported || status == .noValue { return false }
    guard status == .success else { throw ProbeFailure() }
    return (value as? Bool) ?? false
}

private func readAX(_ element: AXUIElement, pid: pid_t) throws -> RawNode<AXUIElement> {
    var owner: pid_t = 0
    guard AXUIElementGetPid(element, &owner) == .success else { throw ProbeFailure() }
    if owner != pid {
        return RawNode(pid: owner, role: "", actions: [], identifier: nil,
            enabled: false, selected: false, children: [], childCount: 0,
            incomplete: true)
    }
    let role = try stringAttribute(element, kAXRoleAttribute as CFString) ?? ""
    let identifier = try stringAttribute(element, kAXIdentifierAttribute as CFString)
    let enabled = try boolAttribute(element, kAXEnabledAttribute as CFString)
    let selected = try boolAttribute(element, kAXSelectedAttribute as CFString)
    var available: CFArray?
    guard AXUIElementCopyActionNames(element, &available) == .success else {
        throw ProbeFailure()
    }
    let actionNames = (available as NSArray?)?.compactMap { $0 as? String } ?? []
    var count: CFIndex = 0
    let status = AXUIElementGetAttributeValueCount(element,
        kAXChildrenAttribute as CFString, &count)
    if status == .attributeUnsupported || status == .noValue {
        return RawNode(pid: owner, role: role, actions: actionNames,
            identifier: identifier, enabled: enabled, selected: selected,
            children: [], childCount: 0, incomplete: false)
    }
    guard status == .success, count >= 0 else { throw ProbeFailure() }
    var children: [AXUIElement] = []
    if count > 0 {
        var values: CFArray?
        guard AXUIElementCopyAttributeValues(element, kAXChildrenAttribute as CFString,
            0, min(count, 65), &values) == .success,
            let array = values as? [AXUIElement] else { throw ProbeFailure() }
        children = array
    }
    return RawNode(pid: owner, role: role, actions: actionNames,
        identifier: identifier, enabled: enabled, selected: selected,
        children: children, childCount: Int(count), incomplete: false)
}

private func parse(_ args: [String]) -> [String: String]? {
    var result: [String: String] = [:]
    var index = 0
    while index < args.count {
        let key = args[index]
        if key == "--observe" || key == "--worker" {
            result[key] = "true"
            index += 1
            continue
        }
        guard ["--pid", "--bundle", "--stage"].contains(key),
              index + 1 < args.count, result[key] == nil else { return nil }
        result[key] = args[index + 1]
        index += 2
    }
    return result
}

private func observe(_ options: [String: String]) throws -> [String: Any] {
    guard let number = options["--pid"], let pid = pid_t(number), pid > 0,
          let bundlePath = options["--bundle"], bundlePath.hasPrefix("/"),
          let stage = options["--stage"],
          ["chat", "menu", "settings", "format", "folder", "progress",
           "done", "cancel"].contains(stage), AXIsProcessTrusted() else {
        throw ProbeFailure()
    }
    let chosen = URL(fileURLWithPath: bundlePath).resolvingSymlinksInPath().standardizedFileURL
    guard chosen.pathExtension == "app", let app = NSRunningApplication(processIdentifier: pid),
          let actual = app.bundleURL?.resolvingSymlinksInPath().standardizedFileURL,
          actual == chosen, let launchDate = app.launchDate else { throw ProbeFailure() }
    let root = AXUIElementCreateApplication(pid)
    guard AXUIElementSetMessagingTimeout(AXUIElementCreateSystemWide(), 0.8) == .success,
          AXUIElementSetMessagingTimeout(root, 0.8) == .success else {
        throw ProbeFailure()
    }
    let report = try capture(root, pid: pid) { try readAX($0, pid: pid) }
    guard let same = NSRunningApplication(processIdentifier: pid),
          same == app, same.launchDate == launchDate,
          same.bundleURL?.resolvingSymlinksInPath().standardizedFileURL == chosen else {
        throw ProbeFailure()
    }
    return ["schema_version": 1, "stage": stage, "pid": Int(pid),
        "application_version": "unknown", "nodes": report.nodes.map { record in
            ["path": record.path, "role": record.role,
             "actions": record.actions, "id_kind": record.id_kind,
             "enabled": record.enabled, "selected": record.selected,
             "children_at_least": record.children_at_least] as [String: Any]
        }, "truncated": report.truncated]
}

private func selfTest() throws {
    struct Fake {
        let role: String
        let id: String
        let actions: [String]
        let children: [Fake]
        let pid: pid_t
    }
    let secret = "Private Customer Alice message 123"
    let root = Fake(role: "AXApplication", id: "session-private", actions: [], children: [
        Fake(role: "AXButton", id: "123456789", actions: ["AXPress", secret],
            children: [], pid: 42),
        Fake(role: secret, id: "chat-private", actions: [], children: [], pid: 42),
        Fake(role: "AXTextField", id: "other-process", actions: [],
            children: [], pid: 43)], pid: 42)
    let reader: (Fake) -> RawNode<Fake> = { item in
        RawNode(pid: item.pid, role: item.role, actions: item.actions,
            identifier: item.id, enabled: true, selected: false,
            children: item.children, childCount: item.children.count, incomplete: false)
    }
    let report = try capture(root, pid: 42, reader: reader)
    let data = try JSONEncoder().encode(report)
    let body = String(decoding: data, as: UTF8.self)
    guard report.nodes.count == 3, report.truncated,
          report.nodes[1].actions == ["AXPress"],
          report.nodes[1].id_kind == "numeric",
          report.nodes[2].role == "other",
          !body.contains(secret), !body.contains("session-private"),
          !body.contains("other-process") else { throw ProbeFailure() }
    let leaf = Fake(role: "AXButton", id: "", actions: [], children: [], pid: 42)
    let wide = Fake(role: "AXApplication", id: "", actions: [],
        children: Array(repeating: leaf, count: 100), pid: 42)
    let wideReport = try capture(wide, pid: 42, reader: reader)
    guard wideReport.nodes.count == 65, wideReport.truncated else { throw ProbeFailure() }
    var deep = leaf
    for _ in 0..<9 {
        deep = Fake(role: "AXGroup", id: "", actions: [], children: [deep], pid: 42)
    }
    let deepReport = try capture(deep, pid: 42, reader: reader)
    guard deepReport.nodes.count == 7, deepReport.truncated else { throw ProbeFailure() }
}

private func emit(_ value: [String: Any], code: Int32) -> Never {
    if let data = try? JSONSerialization.data(withJSONObject: value, options: [.sortedKeys]),
       let line = String(data: data, encoding: .utf8) { print(line) }
    exit(code)
}

let arguments = Array(CommandLine.arguments.dropFirst())
if arguments == ["--self-test"] {
    do { try selfTest(); print("macOS AX probe synthetic checks: PASS"); exit(0) }
    catch { print("macOS AX probe synthetic checks: FAIL"); exit(1) }
}
guard let options = parse(arguments), options["--observe"] == "true" else {
    emit(["error": "observe_required"], code: 1)
}
if options["--worker"] == "true" {
    do { emit(["ok": try observe(options)], code: 0) }
    catch { emit(["error": "probe_failed"], code: 1) }
}

let manager = FileManager.default
let directory = manager.temporaryDirectory.appendingPathComponent(UUID().uuidString)
do {
    try manager.createDirectory(at: directory, withIntermediateDirectories: false,
        attributes: [.posixPermissions: 0o700])
    let output = directory.appendingPathComponent("report.json")
    guard manager.createFile(atPath: output.path, contents: nil,
        attributes: [.posixPermissions: 0o600]) else { throw ProbeFailure() }
    let handle = try FileHandle(forWritingTo: output)
    let worker = Process()
    worker.executableURL = URL(fileURLWithPath: CommandLine.arguments[0]).standardizedFileURL
    worker.arguments = arguments + ["--worker"]
    worker.standardOutput = handle
    worker.standardError = FileHandle.nullDevice
    try worker.run()
    let done = DispatchSemaphore(value: 0)
    DispatchQueue.global().async { worker.waitUntilExit(); done.signal() }
    if done.wait(timeout: .now() + 30) == .timedOut {
        worker.terminate()
        if done.wait(timeout: .now() + 2) == .timedOut {
            kill(worker.processIdentifier, SIGKILL)
            _ = done.wait(timeout: .now() + 2)
        }
        try? handle.close()
        try? manager.removeItem(at: directory)
        emit(["error": "probe_timeout"], code: 1)
    }
    try handle.close()
    guard worker.terminationStatus == 0 else { throw ProbeFailure() }
    let data = try Data(contentsOf: output)
    guard let object = try JSONSerialization.jsonObject(with: data) as? [String: Any],
          object["ok"] != nil else { throw ProbeFailure() }
    print(String(decoding: data, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines))
    try? manager.removeItem(at: directory)
} catch {
    try? manager.removeItem(at: directory)
    emit(["error": "probe_failed"], code: 1)
}
