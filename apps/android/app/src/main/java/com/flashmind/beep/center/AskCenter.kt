package com.flashmind.beep.center

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import com.flashmind.beep.BeepApplication
import com.flashmind.beep.LINEAR_ACCENT
import com.flashmind.beep.MainActivity
import com.flashmind.beep.R
import com.flashmind.beep.ffi.AskOption
import com.flashmind.beep.ffi.MobileNotice
import com.flashmind.beep.ffi.NoticeKind
import com.flashmind.beep.userMessage
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.withTimeout
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import java.util.UUID

/** Which notices the user wants as notifications. Stay connected is off until the user turns it on. */
internal object NotifyPrefs {
    const val ASKS = "notify.asks"
    const val FINISHED = "notify.finished"
    const val CI = "notify.ci"
    const val STAY = "notify.stay"
    private const val FILE = "center"

    fun defaultOn(key: String): Boolean = key != STAY

    fun on(context: Context, key: String): Boolean =
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE).getBoolean(key, defaultOn(key))

    fun set(context: Context, key: String, value: Boolean) =
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE).edit().putBoolean(key, value).apply()

    /** Whether the system would show a notification now. Below Android 13 it always would. */
    fun allowed(context: Context): Boolean = Build.VERSION.SDK_INT < 33 ||
        context.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED
}

/** Notifications for agent questions, with Allow and Deny on them. */
internal object AskAlerts {
    const val CHANNEL = "beep_asks"
    private const val FILE = "center"
    private const val SEEN = "asks.seen"
    private const val KEEP = 200

    private fun manager(context: Context) = context.getSystemService(NotificationManager::class.java)

    fun ensureChannel(context: Context) = manager(context).createNotificationChannel(
        NotificationChannel(CHANNEL, "Agent asks", NotificationManager.IMPORTANCE_HIGH).apply {
            description = "Questions from your coding agents, and what they finished"
        })

    private fun seen(context: Context): List<String> =
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE).getString(SEEN, "").orEmpty().split('\n').filter { it.isNotEmpty() }

    private fun saveSeen(context: Context, keys: List<String>) =
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE).edit().putString(SEEN, keys.takeLast(KEEP).joinToString("\n")).apply()

    /** Posts the question unless it was posted before. A question stays seen while it waits. */
    @Synchronized
    fun post(context: Context, notice: MobileNotice) {
        val key = notice.askKey ?: return
        val before = seen(context)
        if (key in before) return
        saveSeen(context, before + key)
        ensureChannel(context)
        val builder = Notification.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_launcher_monochrome).setColor(LINEAR_ACCENT.toInt())
            .setContentTitle(notice.title).setContentText(notice.detail)
            .setStyle(Notification.BigTextStyle().bigText(notice.detail))
            .setContentIntent(open(context, key.hashCode()))
            .setCategory(Notification.CATEGORY_MESSAGE).setAutoCancel(true)
        notice.options.firstOrNull { !it.allow }?.let { builder.addAction(action(context, notice, it, "Deny")) }
        notice.options.firstOrNull { it.allow }?.let { builder.addAction(action(context, notice, it, "Allow")) }
        manager(context).notify(key.hashCode(), builder.build())
    }

    private fun open(context: Context, code: Int) = PendingIntent.getActivity(context, code,
        Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP), PendingIntent.FLAG_IMMUTABLE)

    private fun action(context: Context, notice: MobileNotice, option: AskOption, label: String): Notification.Action {
        val intent = Intent(context, AskReceiver::class.java)
            .putExtra("machine", notice.machineId).putExtra("agent", notice.agent).putExtra("session", notice.session)
            .putExtra("question", (notice.question ?: 0u).toInt()).putExtra("reply", option.id)
        val code = (notice.askKey + option.id).hashCode()
        val builder = Notification.Action.Builder(null, label,
            PendingIntent.getBroadcast(context, code, intent, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE))
        // Allow needs the phone unlocked (API 31+; earlier versions have no such flag).
        if (option.allow && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) builder.setAuthenticationRequired(true)
        return builder.build()
    }

    /** Lets a question post again, after its answer did not go through. */
    @Synchronized
    fun forget(context: Context, key: String) = saveSeen(context, seen(context) - key)

    /** Forgets the questions that no longer wait. */
    @Synchronized
    fun keepSeen(context: Context, waiting: Set<String>) = saveSeen(context, seen(context).filter { it in waiting })

    fun cancel(context: Context, key: String) = manager(context).cancel(key.hashCode())

    /** Tells the user an answer from a notification did not go through. */
    fun postFailure(context: Context, detail: String) {
        ensureChannel(context)
        manager(context).notify(UUID.randomUUID().hashCode(), Notification.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_launcher_monochrome).setColor(LINEAR_ACCENT.toInt())
            .setContentTitle("Could not send your answer").setContentText(detail)
            .setContentIntent(open(context, 0)).setAutoCancel(true).build())
    }

    fun postFinished(context: Context, notice: MobileNotice) {
        ensureChannel(context)
        manager(context).notify(notice.id.hashCode(), Notification.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_launcher_monochrome).setColor(LINEAR_ACCENT.toInt())
            .setContentTitle(notice.title).setContentText(notice.detail)
            .setContentIntent(open(context, notice.id.hashCode())).setAutoCancel(true).build())
    }
}

/** Sends the Allow or Deny tapped on an ask notification. No path is silent: a failure posts a notice. */
class AskReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val machine = intent.getStringExtra("machine") ?: return
        val agent = intent.getStringExtra("agent") ?: return
        val session = intent.getStringExtra("session") ?: return
        val reply = intent.getStringExtra("reply") ?: return
        val question = intent.getIntExtra("question", 0).toUInt()
        val key = questionKey(machine, agent, session, question)
        val pending = goAsync()
        val repository = (context.applicationContext as BeepApplication).repository
        CoroutineScope(Dispatchers.IO).launch {
            try {
                withTimeout(8_000) { repository.workspaceApi().answerCoding(machine, agent, session, question, reply) }
                AskAlerts.cancel(context, key)
                AskCenter.removeAsk(key)
            } catch (timedOut: TimeoutCancellationException) {
                AskAlerts.forget(context, key)
                AskAlerts.postFailure(context, "The machine did not answer in time.")
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (failed: Exception) {
                AskAlerts.forget(context, key)
                AskAlerts.postFailure(context, userMessage(failed))
            } finally {
                pending.finish()
            }
        }
    }
}

/**
 * Follows the asks feed and keeps the notices the bell lists. Only one feed runs: it is open while
 * the app is in the foreground or the Stay connected service holds it, and closed otherwise.
 */
internal object AskCenter {
    enum class Owner { App, Service }

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val owners = mutableSetOf<Owner>()
    private var job: Job? = null

    private val _log = MutableStateFlow(BellLog())
    val log: StateFlow<BellLog> = _log
    private val _wantsPermission = MutableStateFlow(false)
    /** Set when an ask arrives and the system would not show its notification yet. */
    val wantsPermission: StateFlow<Boolean> = _wantsPermission

    fun permissionAsked() { _wantsPermission.value = false }

    fun start(context: Context, owner: Owner = Owner.App) {
        val app = context.applicationContext
        synchronized(this) {
            owners += owner
            if (job?.isActive == true) return
            job = scope.launch { follow(app) }
        }
    }

    fun stop(owner: Owner = Owner.App) = synchronized(this) {
        owners -= owner
        if (owners.isEmpty()) { job?.cancel(); job = null }
    }

    private suspend fun follow(context: Context) {
        val repository = (context as BeepApplication).repository
        while (true) {
            try {
                val feed = repository.workspaceApi().followAsks()
                try {
                    while (true) receive(context, feed.next() ?: break)
                } finally {
                    feed.close()
                }
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (_: Exception) {
                // The mesh is not linked or the link dropped; try again below.
            }
            delay(5_000)
        }
    }

    fun receive(context: Context, notice: MobileNotice) {
        _log.update { it.add(notice) }
        when (notice.kind) {
            NoticeKind.ASK -> {
                if (!NotifyPrefs.on(context, NotifyPrefs.ASKS) || notice.question == null) return
                if (!NotifyPrefs.allowed(context)) { _wantsPermission.value = true; return }
                AskAlerts.post(context, notice)
            }
            NoticeKind.FINISHED, NoticeKind.RUN -> {
                if (!NotifyPrefs.on(context, if (notice.kind == NoticeKind.FINISHED) NotifyPrefs.FINISHED else NotifyPrefs.CI)) return
                if (NotifyPrefs.allowed(context)) AskAlerts.postFinished(context, notice)
            }
        }
    }

    fun clear() = _log.update { BellLog() }

    /** Drops the questions that no longer wait. */
    fun keepAsks(context: Context, waiting: Set<String>, before: ULong) {
        _log.update { it.keepAsks(waiting, before) }
        // Asks newer than the read stay seen too: the read could not have listed them.
        val recent = _log.value.notices.filter { it.kind == NoticeKind.ASK && it.atMs >= before }.mapNotNull { it.askKey }
        AskAlerts.keepSeen(context, waiting + recent)
    }

    /** The bell row of an answered question leaves. */
    fun removeAsk(key: String) = _log.update { log -> BellLog(log.notices.filter { it.askKey != key }) }

    /** The row leaves the bell at once and comes back if the answer does not go through. */
    suspend fun answer(context: Context, notice: MobileNotice, option: AskOption): String? {
        val question = notice.question ?: return null
        _log.update { it.remove(notice.id) }
        return try {
            (context.applicationContext as BeepApplication).repository.workspaceApi()
                .answerCoding(notice.machineId, notice.agent, notice.session, question, option.id)
            notice.askKey?.let { AskAlerts.cancel(context, it) }
            null
        } catch (cancelled: CancellationException) {
            throw cancelled
        } catch (failed: Exception) {
            _log.update { it.add(notice) }
            userMessage(failed)
        }
    }
}
