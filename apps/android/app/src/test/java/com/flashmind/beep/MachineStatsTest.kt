package com.flashmind.beep

import com.flashmind.beep.proto.MachineDisk
import com.flashmind.beep.proto.MachineStats
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.Instant

class MachineStatsTest {
    private fun stats(disks: List<MachineDisk>, used: ULong = 9_318uL, total: ULong = 32_768uL, load: Double? = 1.25) =
        MachineStats("m", Instant.EPOCH, disks, used, total, load)

    @Test fun bytesMatchTheDesktop() {
        assertEquals("512 B", bytes(512uL))
        assertEquals("9.1 GB", bytes(9_100_000_000uL))
        assertEquals("212 GB", bytes(212_400_000_000uL))
        assertEquals("1.0 TB", bytes(1_000_000_000_000uL))
    }

    @Test fun diskShowsTheFirstVolume() {
        val disk = diskFree(stats(listOf(MachineDisk("/srv/work", 212_000_000_000uL, 1_000_000_000_000uL), MachineDisk("/home", 1uL, 10uL))))
        assertEquals("212 GB free", disk?.text)
        assertFalse(disk!!.low)
    }

    @Test fun diskUnderATenthFreeIsLow() =
        assertTrue(diskFree(stats(listOf(MachineDisk("/", 90_000_000_000uL, 1_000_000_000_000uL))))!!.low)

    @Test fun noVolumeNoDisk() {
        assertNull(diskFree(stats(emptyList())))
        assertNull(diskFree(stats(listOf(MachineDisk("/", 0uL, 0uL)))))
        assertNull(diskFree(null))
    }

    @Test fun memoryIsUsedOfTotal() {
        assertEquals("9.1 / 32 GB", memoryUse(stats(emptyList())))
        assertNull(memoryUse(stats(emptyList(), used = 0uL, total = 0uL)))
    }

    @Test fun loadNamesTheCores() {
        assertEquals("1.25 on 8 cores", loadText(1.25, 8u))
        assertEquals("0.50", loadText(0.5, null))
        assertNull(loadText(null, 8u))
    }
}
