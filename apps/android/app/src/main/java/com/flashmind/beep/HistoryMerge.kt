package com.flashmind.beep

import com.flashmind.beep.proto.PendingApproval

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

/** What a background card says once its question stopped waiting without an answer from this phone. */
internal const val RESOLVED = "resolved"

/**
 * [messages] with background question [queryId] no longer waiting: answered on another device or
 * withdrawn, which look the same. A card this phone answered keeps its answer.
 */
internal fun markResolved(messages: List<ChatMessage>, queryId: String): List<ChatMessage> = messages.map { m ->
    if (m.approvals.none { it.background && it.waiting && it.queryId == queryId }) m
    else m.copy(approvals = m.approvals.map { if (it.background && it.waiting && it.queryId == queryId) it.copy(answer = RESOLVED) else it })
}

/**
 * [messages] matched to [waiting], the background questions the server lists for the chat, oldest
 * first: each gets a card at the end if the chat has none, and waiting cards it no longer lists stop
 * waiting.
 */
internal fun syncQuestions(messages: List<ChatMessage>, waiting: List<PendingApproval>): List<ChatMessage> {
    val listed = waiting.mapTo(HashSet()) { it.queryId }
    val known = messages.flatMap { m -> m.approvals.map { it.queryId } }.toSet()
    val stale = messages.flatMap { m -> m.approvals.filter { it.background && it.waiting && it.queryId !in listed }.map { it.queryId } }
    val kept = stale.fold(messages) { acc, id -> markResolved(acc, id) }
    return kept + waiting.filter { it.queryId !in known }.map(::questionCard)
}

/** The card a background question shows as at the end of its chat. */
internal fun questionCard(pending: PendingApproval) = ChatMessage("approval-${pending.queryId}", "assistant", "",
    approvals = listOf(ApprovalPrompt(pending.queryId, pending.approval, background = true)))
