package com.flashmind.beep

/**
 * The turns [fresh] from the server merged with [before], what the phone showed: messages already on
 * screen keep their ids, or the list drops and re-adds every row while it is drawn, and scores the
 * server did not keep stay on. Background work's questions are not in the history, so those still
 * waiting stay at the end. History keeps no tool arguments, so calls the phone saw start keep what the
 * server said they did. No two messages share an id: the list keys its rows by them.
 */
internal fun mergeHistory(before: List<ChatMessage>, fresh: List<ChatMessage>): List<ChatMessage> {
    val waiting = before.filter { m -> m.approvals.any { it.background && it.waiting } }
    val waitingIds = waiting.mapTo(HashSet()) { it.id }
    // Turns from before the server kept Jev scores only have the one the phone saw live.
    val scores = before.filter { it.memoryScore != null }.associate { it.text to it.memoryScore }
    // A waiting background card is kept as it is, so no turn may take its id.
    val kept = before.filter { it.id !in waitingIds }.groupBy { it.role to it.text }.mapValues { ArrayDeque(it.value) }
    val said = before.flatMap { it.toolCalls }.filter { it.callId.isNotEmpty() && it.description.isNotEmpty() }
        .associate { it.callId to it.description }
    val merged = fresh.map { m -> m.copy(toolCalls = m.toolCalls.map { c -> said[c.callId]?.takeIf { c.description.isEmpty() }?.let { c.copy(description = it) } ?: c }) }
        .map { m -> if (m.memoryScore != null) m else scores[m.text]?.let { m.copy(memoryScore = it) } ?: m }
        .map { m -> kept[m.role to m.text]?.removeFirstOrNull()?.let { m.copy(id = it.id) } ?: m }
        .plus(waiting)
    return merged.distinctBy { it.id }
}
