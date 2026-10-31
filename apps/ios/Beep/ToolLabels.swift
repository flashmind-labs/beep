import Foundation

/// What a tool chip says while the call runs, the same words as on Android. `description` is
/// the server's own words for the call, which only a live run carries; a tool that takes an
/// action argument says what that action does with them. From the history, a memory call's
/// `output` shows what it did.
func toolLabel(_ name: String, description: String = "", output: String = "") -> String {
    if describedTools.contains(name) || name.hasPrefix("mcp_") {
        let described = description.trimmingCharacters(in: CharacterSet(charactersIn: "….").union(.whitespaces))
        if !described.isEmpty { return described }
    }
    if let known = toolLabels[name] { return known }
    if name == "memory", let done = memoryLabel(output) { return done }
    if let family = actionToolLabels[name] { return family }
    let words = name.split(whereSeparator: { "_.-".contains($0) }).map { $0.lowercased() }
    guard let head = words.first else { return "Using a tool" }
    // Connected app tools are named APP_ACTION.
    if name == name.uppercased(), words.count > 1 {
        return head.capitalized + ": " + words.dropFirst().joined(separator: " ")
    }
    let text = words.joined(separator: " ")
    return text.prefix(1).uppercased() + text.dropFirst()
}

/// Whether a memory call wrote something, from its output: a stored, updated or forgotten memory.
func wroteMemory(_ name: String, output: String) -> Bool {
    guard name == "memory", let label = memoryLabel(output) else { return false }
    return label != "Recalling memories" && label != "Checking a memory's history"
}

/// What a memory call did, read from its output: an object for a write, a list for a read.
private func memoryLabel(_ output: String) -> String? {
    guard let data = output.data(using: .utf8), let json = try? JSONSerialization.jsonObject(with: data) else { return nil }
    if let list = json as? [[String: Any]] {
        return list.first?["replaced"] != nil ? "Checking a memory's history" : "Recalling memories"
    }
    guard let object = json as? [String: Any] else { return nil }
    if object["stored"] != nil { return "Remembering" }
    if object["updated"] != nil { return "Updating a memory" }
    if object["forgotten"] != nil { return "Forgetting a memory" }
    return nil
}

/// Tools whose words from the server say more than the name: the action ones, and those that
/// name the machine, file or app they work on. Other tools' server words just list arguments.
private let describedTools: Set<String> = [
    "memory", "background", "skill", "habit", "reminder", "node", "mac_files",
    "exec", "process", "desktop", "code_agent", "send_file",
]

/// Tools that take an action argument, as the server folds them, for a call from the history:
/// it keeps neither the arguments nor the server's words for the call.
private let actionToolLabels: [String: String] = [
    "memory": "Checking memories",
    "background": "Working in the background",
    "skill": "Using a skill",
    "habit": "Tracking a habit",
    "reminder": "Working on reminders",
    "exec": "Running a command",
    "process": "Checking background commands",
    "desktop": "Using the Mac",
    "code_agent": "Using a coding agent",
    "send_file": "Sending a file",
]

/// Tools that get no chip: what they do is already on screen, or is bookkeeping.
let quietTools: Set<String> = ["suggest_replies", "show_products", "show_cards", "notify_user", "task_progress"]

private let toolLabels: [String: String] = [
    "web_search": "Searching the web",
    "decision": "Weighing the options",
    "get_location": "Checking your location",
    "get_time": "Checking the time",
    "open_app": "Opening an app",
    "set_alarm": "Setting an alarm",
    "open_browser": "Opening a page",
    "browser": "Browsing",
    "node": "Using your machines",
    "mac_files": "Looking at your Mac's files",
    "event_topics": "Checking which updates you get",
    "hold_events": "Holding updates for later",
    "recent_events": "Checking recent updates",
    "open_maps": "Looking up a place",
    "place_save": "Saving a place",
    "place_list": "Checking your places",
    "place_delete": "Forgetting a place",
    "place_routine": "Setting up a place routine",
    "query_contacts": "Looking up contacts",
    "suggest_replies": "Suggesting replies",
    "show_products": "Showing products",
    "show_cards": "Showing recommendations",
    "task_progress": "Updating progress",
    "cron_create": "Setting a reminder",
    "schedule_once": "Setting a reminder",
    "cron_edit": "Changing a reminder",
    "cron_list": "Checking reminders",
    "cron_delete": "Deleting a reminder",
    "end_routine": "Ending this routine",
    "request_grant": "Asking to allow a routine",
    "get_route": "Planning a route",
    "send_image": "Sending pictures",
    "send_file": "Sending a file",
    "show_chart": "Drawing a chart",
    "show_stock_chart": "Charting a price",
    "execute_code": "Running code",
    "download_file": "Downloading a file",
    "chat_search": "Searching your chats",
    "chat_read": "Reading a chat",
    "notify_user": "Sending a notification",
    "start_research": "Starting a research",
    "research_more": "Researching further",
    "add_scouts": "Adding researchers",
    "stop_research": "Stopping the research",
    "post_message": "Posting to the research room",
    "read_room": "Reading the research room",
    "add_source": "Adding a source",
    "add_note": "Adding a note",
    "read_notes": "Reading the notes",
    "read_draft": "Reading the draft",
    "write_draft": "Writing the draft",
]

/// The first letter a chip shows in its circle: the app's for app tools.
func toolInitial(_ name: String) -> String {
    String(toolLabel(name).prefix(1)).uppercased()
}

/// A finished chip's label in the past tense, as on Android: "Searching the web" becomes "Searched the web".
func doneLabel(_ label: String) -> String {
    var text = label
    while text.hasSuffix(".") || text.hasSuffix("…") { text.removeLast() }
    let space = text.firstIndex(of: " ") ?? text.endIndex
    let head = String(text[..<space])
    guard let past = pastTense[head.lowercased()] else { return text }
    return past + text[space...]
}

private let pastTense: [String: String] = [
    "searching": "Searched", "reading": "Read", "checking": "Checked", "looking": "Looked",
    "fetching": "Fetched", "opening": "Opened", "saving": "Saved", "finding": "Found",
    "getting": "Got", "creating": "Created", "updating": "Updated", "deleting": "Deleted",
    "sending": "Sent", "setting": "Set", "using": "Used", "remembering": "Remembered",
    "loading": "Loaded", "browsing": "Browsed", "scheduling": "Scheduled", "running": "Ran",
    "weighing": "Weighed", "recalling": "Recalled", "forgetting": "Forgot", "changing": "Changed",
    "logging": "Logged", "suggesting": "Suggested", "showing": "Showed",
    "starting": "Started", "messaging": "Messaged", "moving": "Moved", "waiting": "Waited",
    "stopping": "Stopped", "holding": "Held", "ending": "Ended", "planning": "Planned",
    "drawing": "Drew", "charting": "Charted", "downloading": "Downloaded", "posting": "Posted",
    "adding": "Added", "writing": "Wrote", "researching": "Researched", "working": "Worked",
    "tracking": "Tracked", "deciding": "Decided", "typesetting": "Typeset", "linking": "Linked",
    "following": "Followed", "unfollowing": "Unfollowed", "taking": "Took", "answering": "Answered",
    "asking": "Asked", "copying": "Copied", "clicking": "Clicked", "typing": "Typed", "pressing": "Pressed", "scrolling": "Scrolled",
]
