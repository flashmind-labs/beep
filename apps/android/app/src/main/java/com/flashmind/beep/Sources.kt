package com.flashmind.beep

import com.flashmind.beep.proto.WebSource
import java.net.URI

/** Source chips shown before the rest fold into a "+N" chip. */
internal const val SHOWN_SOURCES = 3

/**
 * [messages] with each turn's web sources on the last reply of that turn that has text. [sources] is
 * keyed by the `sentAt` of the prompt that started the turn, as the server lists them.
 */
internal fun withSources(messages: List<ChatMessage>, sources: Map<Long, List<WebSource>>): List<ChatMessage> {
    if (sources.isEmpty()) return messages
    // The index of each turn's last reply with text, by the turn's prompt time.
    val replyAt = HashMap<Long, Int>()
    var turn: Long? = null
    messages.forEachIndexed { i, m ->
        when {
            m.role == "user" -> turn = m.sentAt
            m.role != "assistant" -> turn = null
            m.text.isNotBlank() -> turn?.let { replyAt[it] = i }
        }
    }
    val byIndex = replyAt.entries.associate { (at, i) -> i to sources[at].orEmpty() }
    return messages.mapIndexed { i, m -> byIndex[i]?.takeIf { it.isNotEmpty() }?.let { m.copy(sources = it) } ?: m }
}

/** The host [url] names without a leading "www.", or [url] itself when it has none. */
internal fun sourceHost(url: String): String =
    runCatching { URI(url).host }.getOrNull()?.removePrefix("www.")?.takeIf { it.isNotEmpty() } ?: url

/** The chips to show for [count] sources: how many sources, and how many fold into "+N" (0 for none). */
internal fun sourceFold(count: Int, expanded: Boolean): Pair<Int, Int> =
    if (expanded || count <= SHOWN_SOURCES) count to 0 else SHOWN_SOURCES to count - SHOWN_SOURCES
