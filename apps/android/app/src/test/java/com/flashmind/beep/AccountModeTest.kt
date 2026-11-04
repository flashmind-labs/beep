package com.flashmind.beep

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AccountModeTest {
    @Test fun newAccountKeepsWorkspaceHidden() {
        assertFalse(WorkspaceSignals().expanded)
    }

    @Test fun linkedLaptopAloneKeepsWorkspaceHidden() {
        assertFalse(WorkspaceSignals(machines = 1).expanded)
    }

    @Test fun githubConnectionRevealsWorkspaceWithoutAMachine() {
        assertTrue(WorkspaceSignals(github = true).expanded)
    }

    @Test fun existingProjectRevealsWorkspaceWithoutGithub() {
        assertTrue(WorkspaceSignals(projects = true).expanded)
    }

    @Test fun additionalMachineRevealsWorkspace() {
        assertTrue(WorkspaceSignals(machines = 2).expanded)
    }
}
