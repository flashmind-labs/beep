package com.flashmind.beep.center

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.flashmind.beep.ffi.MobileHome
import com.flashmind.beep.ffi.MobileNotice
import com.flashmind.beep.ffi.Need
import com.flashmind.beep.ffi.NoticeKind
import com.flashmind.beep.proto.AgentOption
import com.flashmind.beep.proto.MachineSession
import com.flashmind.beep.userMessage
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch

/** The key a question is known by, in the bell, on Home and in the seen list. */
internal fun questionKey(machine: String, agent: String, session: String, question: UInt): String =
    "$machine:$agent:$session:$question"

internal val MobileNotice.askKey: String?
    get() = question?.let { questionKey(machineId, agent, session, it) }

internal val Need.key: String
    get() = when (this) {
        is Need.Ask -> questionKey(session.machineId, session.agent, session.id, session.question?.id ?: 0u)
        is Need.Pull -> "${item.host}/${item.repo}#${item.number}"
        is Need.Conflict -> "$project:$path"
    }

/** The notices the bell lists, newest first. It keeps the last hundred. */
internal data class BellLog(val notices: List<MobileNotice> = emptyList()) {
    fun add(notice: MobileNotice): BellLog =
        BellLog((notices.filter { it.id != notice.id } + notice).sortedByDescending { it.atMs }.take(LIMIT))

    fun remove(id: String) = BellLog(notices.filter { it.id != id })

    /** Drops questions that no longer wait, given the keys that still do. Only those older than
     *  `before` go: a newer one may have arrived after the read that listed `waiting`. */
    fun keepAsks(waiting: Set<String>, before: ULong) =
        BellLog(notices.filter { it.kind != NoticeKind.ASK || it.atMs >= before || it.askKey in waiting })

    companion object { const val LIMIT = 100 }
}

internal typealias AnswerCoding = suspend (machine: String, agent: String, session: String, question: UInt, reply: String) -> Unit

/** What Home shows. An answered question leaves the list at once and comes back if the send fails. */
internal class HomeModel(
    private val load: suspend () -> MobileHome,
    private val send: AnswerCoding,
    /** Answers run here, so a row leaving the screen does not cancel its send. */
    private val scope: CoroutineScope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate),
) {
    var home by mutableStateOf<MobileHome?>(null)
        private set
    var error by mutableStateOf("")
        private set
    /** Questions being answered now. */
    private var sending by mutableStateOf(emptySet<String>())
    /** Answered questions, with the number of refreshes begun when the answer landed. A refresh
     *  that began before it may still list the question, so the key stays hidden until a later one. */
    private var answered by mutableStateOf(emptyMap<String, Int>())
    private var begun = 0

    val needs: List<Need> get() = home?.needs.orEmpty().filter { it.key !in sending && it.key !in answered }
    /** The number on the bell. */
    val badge: Int get() = needs.size
    /** The questions that still wait. */
    val waiting: Set<String> get() = home?.needs.orEmpty().map { it.key }.toSet()

    suspend fun refresh() {
        begun += 1
        val mine = begun
        try {
            home = load()
            answered = answered.filterValues { it >= mine }
            error = ""
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (failed: Exception) {
            error = userMessage(failed)
        }
    }

    fun submit(session: MachineSession, option: AgentOption): Job = scope.launch { answer(session, option) }

    suspend fun answer(session: MachineSession, option: AgentOption) {
        val question = session.question ?: return
        val key = Need.Ask(session).key
        sending = sending + key
        try {
            send(session.machineId, session.agent, session.id, question.id, option.id)
            home = home?.let { read -> read.copy(needs = read.needs.filter { it.key != key }) }
            answered = answered + (key to begun)
            error = ""
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (failed: Exception) {
            error = userMessage(failed)
        } finally {
            sending = sending - key
        }
    }
}
