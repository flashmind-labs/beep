import Foundation

/// What a tool chip says, the same words as on Android.
func toolLabel(_ name: String) -> String {
    if let known = toolLabels[name] { return known }
    let words = name.split(whereSeparator: { "_.-".contains($0) }).map { $0.lowercased() }
    guard let head = words.first else { return "Using a tool" }
    // Connected app tools are named APP_ACTION.
    if name == name.uppercased(), words.count > 1 {
        return head.capitalized + ": " + words.dropFirst().joined(separator: " ")
    }
    let text = words.joined(separator: " ")
    return text.prefix(1).uppercased() + text.dropFirst()
}

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
    "mcp_search": "Finding the right app tool",
    "mcp": "Using a connected app",
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
    "memory_store": "Remembering",
    "memory_recall": "Recalling memories",
    "memory_update": "Updating a memory",
    "memory_forget": "Forgetting a memory",
    "cron_create": "Setting a reminder",
    "schedule_once": "Setting a reminder",
    "cron_edit": "Changing a reminder",
    "cron_list": "Checking reminders",
    "cron_delete": "Deleting a reminder",
    "cron_history": "Checking past reminders",
    "habit_create": "Creating a habit",
    "habit_log": "Logging a habit",
    "habit_status": "Checking habits",
    "habit_delete": "Deleting a habit",
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
]
