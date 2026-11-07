package com.flashmind.beep.center

import android.text.format.DateFormat
import android.text.format.DateUtils
import androidx.compose.foundation.layout.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import com.flashmind.beep.Beep
import com.flashmind.beep.WorkspaceError
import com.flashmind.beep.ffi.AskOption
import com.flashmind.beep.ffi.MobileNotice
import com.flashmind.beep.ffi.NoticeKind
import kotlinx.coroutines.launch

/** The last notices from the asks feed, with the options of a question to answer it here. */
@Composable
internal fun BellScreen(back: () -> Unit, open: (MobileNotice) -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val log by AskCenter.log.collectAsState()
    var error by remember { mutableStateOf("") }
    val today = log.notices.filter { DateUtils.isToday(it.atMs.toLong()) }
    val earlier = log.notices.filter { !DateUtils.isToday(it.atMs.toLong()) }
    Column(Modifier.fillMaxSize()) {
        CcTop("Notifications", back) {
            if (log.notices.isNotEmpty()) TextButton(onClick = AskCenter::clear) { Text("Clear", fontSize = 12.sp, color = Beep.colors.brand) }
        }
        CcBody {
            WorkspaceError(error.ifEmpty { null })
            if (log.notices.isEmpty()) CcPanel { CcEmpty("No notifications", "Questions and finished work show here.") }
            else {
                fun answer(notice: MobileNotice, option: AskOption) = scope.launch { error = AskCenter.answer(context, notice, option).orEmpty() }
                if (today.isNotEmpty()) CcSection("Today") { CcPanel { today.forEach { NoticeRow(it, open, ::answer) } } }
                if (earlier.isNotEmpty()) CcSection("Earlier") { CcPanel { earlier.forEach { NoticeRow(it, open, ::answer) } } }
            }
        }
    }
}

@Composable
private fun NoticeRow(notice: MobileNotice, open: (MobileNotice) -> Unit, answer: (MobileNotice, AskOption) -> Unit) {
    val time = DateFormat.getTimeFormat(LocalContext.current).format(notice.atMs.toLong())
    val timeText: @Composable RowScope.() -> Unit = { Text(time, fontSize = 11.5.sp, color = Beep.colors.muted) }
    when (notice.kind) {
        NoticeKind.ASK -> Column {
            CcRow(notice.title, notice.detail, onClick = { open(notice) }, lead = { CcAgentTile(notice.agent) }, right = timeText)
            val primary = notice.options.firstOrNull { it.allow }?.id
            val short = notice.options.size <= 2 && notice.options.all { it.name.length <= 12 }
            if (short) Row(Modifier.fillMaxWidth().padding(start = 11.dp, end = 11.dp, bottom = 10.dp), horizontalArrangement = Arrangement.spacedBy(6.dp, androidx.compose.ui.Alignment.End)) {
                (notice.options.filter { it.id != primary } + notice.options.filter { it.id == primary }).forEach { option ->
                    CcButton(option.name, if (option.id == primary) CcButtonStyle.Primary else CcButtonStyle.Plain) { answer(notice, option) }
                }
            } else Column(Modifier.padding(start = 11.dp, end = 11.dp, bottom = 10.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                notice.options.forEach { option ->
                    CcButton(option.name, if (option.id == primary) CcButtonStyle.Primary else CcButtonStyle.Plain, leading = true) { answer(notice, option) }
                }
            }
        }
        NoticeKind.FINISHED -> CcRow(notice.title, notice.detail, onClick = { open(notice) }, lead = { CcKindTile("✓", CcKind.Ok) }, right = timeText)
        NoticeKind.RUN -> {
            val failed = notice.title.contains("failed")
            CcRow(notice.title, notice.detail, lead = { CcKindTile(if (failed) "✕" else "✓", if (failed) CcKind.Bad else CcKind.Ok) }, right = timeText)
        }
    }
}
