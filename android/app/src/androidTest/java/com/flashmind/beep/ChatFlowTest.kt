package com.flashmind.beep

import android.content.Intent
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.uiautomator.By
import androidx.test.uiautomator.UiDevice
import androidx.test.uiautomator.Until
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class ChatFlowTest {
    @Test fun chatAndBubbleOverRealQuic() {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val live = InstrumentationRegistry.getArguments().getString("liveModel") == "true"
        val firstReply = if (live) "323" else "saved your preference for tea"
        val secondReply = if (live) "42" else "Prefers tea"
        val context = instrumentation.targetContext
        val device = UiDevice.getInstance(instrumentation)
        device.wakeUp()
        device.executeShellCommand("wm dismiss-keyguard")
        // Test-only profile is pushed by the developer; it is never embedded in either APK.
        val raw = device.executeShellCommand("cat /sdcard/Download/beep-test.json")
        val config = ConnectionSettings.fromJson(raw)
        LocalStorage(context).apply { saveSettings(config); saveState(ChatState()) }
        context.startActivity(Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK))
        assertTrue(device.wait(Until.hasObject(By.text("A little help.\nRight here.")), 15000))
        val editor = device.findObject(By.clazz("android.widget.EditText"))
        editor.text = if (live) "What is 17 multiplied by 19? Reply with only the number." else "Remember that I prefer tea."
        device.findObject(By.desc("Send message")).click()
        assertTrue("Expected streamed answer", device.wait(Until.hasObject(By.textContains(firstReply)), 90000))
        // A fresh conversation recalls memory that was stored by a real Flashmind tool.
        device.findObject(By.desc("Conversations")).click()
        assertTrue(device.wait(Until.hasObject(By.text("New conversation")), 5000))
        device.waitForIdle()
        Thread.sleep(400) // Let drawer/menu bounds settle before tapping.
        device.findObject(By.text("New conversation")).click()
        assertTrue(device.wait(Until.gone(By.text("Your conversations")), 5000))
        val secondEditor = device.wait(Until.findObject(By.clazz("android.widget.EditText")), 5000)
        assertNotNull("New conversation should show its composer", secondEditor)
        secondEditor.text = if (live) "What is 21 multiplied by 2? Reply with only the number." else "What do you remember about my drink preference?"
        device.findObject(By.desc("Send message")).click()
        assertTrue(device.wait(Until.hasObject(By.textContains(secondReply)), 90000))
        device.pressHome()
        context.startActivity(Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        assertTrue(device.wait(Until.hasObject(By.textContains(secondReply)), 5000))
        device.executeShellCommand("pm grant com.flashmind.beep android.permission.POST_NOTIFICATIONS")
        // Equivalent to the user allowing bubbles in Android's app settings.
        device.executeShellCommand("cmd notification set_bubbles com.flashmind.beep 1")
        device.findObject(By.desc("Float chat")).click()
        // System bubble launch is asynchronous; dismiss our explanatory dialog if present.
        device.wait(Until.hasObject(By.text("Got it")), 1500)
        device.findObject(By.text("Got it"))?.click()
        Thread.sleep(1500)
        // Actual bubble activity must open, not just a successful notification post.
        val activity = device.executeShellCommand("dumpsys activity activities")
        assertTrue("Expected Android to launch BubbleActivity", activity.contains("com.flashmind.beep/.BubbleActivity"))
        assertTrue(device.wait(Until.hasObject(By.desc("Open full app")), 5000))
        device.takeScreenshot(java.io.File(context.filesDir, "bubble-test.png"))
        assertFalse("Bubble keeps copy actions out of the way", device.hasObject(By.text("Copy reply")))
        device.findObject(By.desc("Chat options")).click()
        assertTrue(device.wait(Until.hasObject(By.text("New conversation")), 5000))
        device.waitForIdle()
        Thread.sleep(400) // Let drawer/menu bounds settle before tapping.
        device.findObject(By.text("New conversation")).click()
        assertTrue(device.wait(Until.hasObject(By.text("Hey. What can I help with?")), 5000))
        assertFalse(device.hasObject(By.text("Remember something")))
        assertNotNull(device.findObject(By.clazz("android.widget.EditText")))
        device.takeScreenshot(java.io.File(context.filesDir, "bubble-empty.png"))
    }
}
