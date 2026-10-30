package com.flashmind.beep

import org.json.JSONObject

/** The server's memory tool; its `action` says whether it recalls, stores, updates, forgets or reads a history. */
internal const val MEMORY_TOOL = "memory"

/**
 * What a tool call is doing, in words: "Searching the web". A tool in the table says the same on every
 * device. A tool that takes an action says what the server named when the call started, or, for a call
 * read back from history, what its output shows it did. Unknown tools fall back to their name:
 * "GMAIL_SEND_EMAIL" becomes "Gmail: send email".
 */
fun callLabel(call: ToolCall): String {
    toolLabels[call.name]?.let { return it }
    val said = call.description.trimEnd('…', '.', ' ')
    val default = actionDefaults[call.name]
    if (default != null) {
        // Calls cached before the server named actions have the tool's own name as their description.
        if (said.isNotEmpty() && said != humanize(call.name) && !said.startsWith("Using a tool")) return said
        return actionLabel(call.name, call.result) ?: default
    }
    return said.ifEmpty { humanize(call.name) }.ifEmpty { "Using a tool" }
}

/**
 * Gives tool steps from history the description the server kept from when they ran, so a done step
 * reads as the past tense of what it showed live. Steps it kept none for are left as they are.
 */
internal fun labelTools(tools: List<ToolCall>, labels: Map<String, String>): List<ToolCall> =
    tools.map { call -> labels[call.callId]?.takeIf { it.isNotBlank() }?.let { call.copy(description = it) } ?: call }

/** What a finished tool did, in words: "Searched the web". */
fun doneToolLabel(call: ToolCall): String = doneLabel(callLabel(call))

private fun humanize(name: String): String {
    val words = name.split('_', '.', '-').filter { it.isNotBlank() }.map { it.lowercase() }
    if (words.isEmpty()) return ""
    // Connected app tools are named APP_ACTION.
    if (name == name.uppercase() && words.size > 1)
        return words[0].replaceFirstChar { it.uppercase() } + ": " + words.drop(1).joinToString(" ")
    return words.joinToString(" ").replaceFirstChar { it.uppercase() }
}

private val toolLabels = mapOf(
    "web_search" to "Searching the web",
    "decision" to "Weighing the options",
    "get_location" to "Checking your location",
    "get_time" to "Checking the time",
    "open_app" to "Opening an app",
    "set_alarm" to "Setting an alarm",
    "open_browser" to "Opening a page",
    "browser" to "Browsing",
    "node" to "Using your machines",
    "hold_events" to "Holding updates for later",
    "recent_events" to "Checking recent updates",
    "event_topics" to "Checking what to watch for",
    "start_research" to "Starting a research",
    "research_more" to "Researching further",
    "add_scouts" to "Adding scouts to the research",
    "stop_research" to "Stopping the research",
    "mcp_search" to "Finding the right app tool",
    "mcp" to "Using a connected app",
    "open_maps" to "Looking up a place",
    "get_route" to "Planning a route",
    "show_products" to "Showing products",
    "show_cards" to "Showing recommendations",
    "show_stock_chart" to "Charting the price",
    "show_chart" to "Drawing a chart",
    "send_image" to "Sending images",
    "execute_code" to "Running code",
    "download_file" to "Downloading a file",
    "chat_search" to "Searching your chats",
    "chat_read" to "Reading a chat",
    "notify_user" to "Sending a notification",
    "end_routine" to "Ending this routine",
    "place_save" to "Saving a place",
    "place_list" to "Checking your places",
    "place_delete" to "Forgetting a place",
    "place_routine" to "Setting up a place routine",
    "query_contacts" to "Looking up contacts",
    "suggest_replies" to "Suggesting replies",
    "task_progress" to "Updating progress",
    "cron_create" to "Setting a reminder",
    "schedule_once" to "Setting a reminder",
    "cron_edit" to "Changing a reminder",
    "cron_list" to "Checking reminders",
    "cron_delete" to "Deleting a reminder",
    "cron_history" to "Checking past reminders",
    "request_grant" to "Asking to allow a routine",
    // Names from before the server folded each family into one tool, still in older chats.
    "spawn_agent" to "Starting an agent",
    "run_in_background" to "Running a job",
    "jobs" to "Checking background work",
    "wait_job" to "Waiting for background work",
    "cancel_job" to "Stopping background work",
    "message_agent" to "Messaging an agent",
    "memory_store" to "Remembering",
    "memory_recall" to "Recalling memories",
    "memory_update" to "Updating a memory",
    "memory_forget" to "Forgetting a memory",
    "memory_history" to "Checking a memory's history",
    "habit_create" to "Creating a habit",
    "habit_log" to "Logging a habit",
    "habit_status" to "Checking habits",
    "habit_delete" to "Deleting a habit",
)

/** Tools whose label depends on their arguments, with what they say when neither the server nor the output tells. */
private val actionDefaults = mapOf(
    MEMORY_TOOL to "Checking memories",
    "skill" to "Loading a skill",
    "background" to "Checking background work",
    "habit" to "Checking habits",
    "reminder" to "Checking reminders",
    "mac_files" to "Looking at files on your Mac",
    "send_file" to "Sending a file",
)

/** The action a call took, read from its output, for calls from history, which keep no arguments. */
private fun actionLabel(name: String, result: String): String? {
    val text = result.trim()
    val json = if (text.startsWith("{")) runCatching { JSONObject(text) }.getOrNull() else null
    return when (name) {
        MEMORY_TOOL -> when {
            text.startsWith("[") -> if (text.contains("\"replaced\"")) "Checking a memory's history" else "Recalling memories"
            json == null -> null
            json.has("forgotten") -> "Forgetting a memory"
            json.has("stored") || json.has("id") -> "Remembering"
            json.has("updated") -> "Updating a memory"
            else -> null
        }
        "skill" -> when {
            json == null -> null
            json.has("created") -> "Saving a skill"
            json.has("updated") -> "Updating a skill"
            else -> "Loading a skill"
        }
        "habit" -> when {
            json == null -> null
            json.has("created") -> "Creating a habit"
            json.has("logged") -> "Logging a habit"
            json.has("archived") -> "Deleting a habit"
            else -> "Checking habits"
        }
        "reminder" -> when {
            json == null -> null
            json.has("set") -> "Setting a reminder"
            json.has("deleted") -> "Deleting a reminder"
            else -> "Checking reminders"
        }
        "background" -> when {
            text.startsWith("Started agent") -> "Starting an agent"
            text.startsWith("Job ") && text.contains(" is running") -> "Running a job"
            text.startsWith("Stopped") || text.endsWith("had already ended.") -> "Stopping background work"
            text.startsWith("Sent to ") || text.contains("started again with your message") || text.contains("cannot take messages") ->
                "Messaging an agent"
            text.lineSequence().first().let { it.endsWith(" finished:") || it.endsWith(" failed:") } || text.contains(" is still running") ->
                "Waiting for background work"
            else -> null
        }
        else -> null
    }
}

/** "Searching the web…" becomes "Searched the web" once the tool is done. */
internal fun doneLabel(label: String): String {
    val trimmed = label.trimEnd('…', '.', ' ')
    val first = trimmed.substringBefore(' ')
    val past = pastTense[first.lowercase()] ?: return trimmed
    return past + trimmed.substring(first.length)
}

private val pastTense = mapOf(
    "searching" to "Searched", "reading" to "Read", "checking" to "Checked", "looking" to "Looked",
    "fetching" to "Fetched", "opening" to "Opened", "saving" to "Saved", "finding" to "Found",
    "getting" to "Got", "creating" to "Created", "updating" to "Updated", "deleting" to "Deleted",
    "sending" to "Sent", "setting" to "Set", "using" to "Used", "remembering" to "Remembered",
    "loading" to "Loaded", "browsing" to "Browsed", "scheduling" to "Scheduled", "running" to "Ran",
    "weighing" to "Weighed", "recalling" to "Recalled", "forgetting" to "Forgot", "changing" to "Changed",
    "logging" to "Logged", "suggesting" to "Suggested", "starting" to "Started", "showing" to "Showed",
    "moving" to "Moved", "waiting" to "Waited", "stopping" to "Stopped", "messaging" to "Messaged", "holding" to "Held",
    "charting" to "Charted", "drawing" to "Drew", "planning" to "Planned", "downloading" to "Downloaded",
    "ending" to "Ended", "asking" to "Asked", "adding" to "Added", "researching" to "Researched", "typesetting" to "Typeset",
)
