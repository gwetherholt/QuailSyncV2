package com.quailsync.app

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.test.assertIsNotSelected
import androidx.compose.ui.test.assertIsSelected
import androidx.compose.ui.test.assertTextEquals
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.test.ext.junit.runners.AndroidJUnit4
import com.quailsync.app.ui.screens.CameraModeControl
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Isolated Compose test for [CameraModeControl] — the incubator|brooder toggle.
 *
 * Unlike [DashboardE2ETest] (which drives the whole app against a live seeded
 * server), this mounts just the stateless control with a tiny stateful wrapper,
 * so it verifies the toggle *reflects* the current assignment and *updates* it
 * on click — including the derived model label following the selection — without
 * needing a backend. The derived-model mapping in the wrapper mirrors the
 * server's (`incubator` → "incubation", `brooder` → "chick").
 */
@RunWith(AndroidJUnit4::class)
class CameraModeControlTest {

    @get:Rule
    val composeTestRule = createComposeRule()

    private fun modelFor(assignment: String) =
        if (assignment == "brooder") "chick" else "incubation"

    @Test
    fun reflectsInitialAssignmentAndDerivedModel() {
        composeTestRule.setContent {
            CameraModeControl(
                assignment = "incubator",
                activeModel = modelFor("incubator"),
                enabled = true,
                onSelect = {},
            )
        }

        composeTestRule.onNodeWithTag("camera_mode_incubator").assertIsSelected()
        composeTestRule.onNodeWithTag("camera_mode_brooder").assertIsNotSelected()
        composeTestRule.onNodeWithTag("camera_mode_active_model")
            .assertTextEquals("Model: incubation")
    }

    @Test
    fun clickingBrooderUpdatesSelectionAndModel() {
        composeTestRule.setContent {
            var assignment by remember { mutableStateOf("incubator") }
            CameraModeControl(
                assignment = assignment,
                activeModel = modelFor(assignment),
                enabled = true,
                onSelect = { assignment = it },
            )
        }

        // Starts on incubator.
        composeTestRule.onNodeWithTag("camera_mode_incubator").assertIsSelected()
        composeTestRule.onNodeWithTag("camera_mode_active_model")
            .assertTextEquals("Model: incubation")

        // Switch to brooder -> selection moves and the derived model follows.
        composeTestRule.onNodeWithTag("camera_mode_brooder").performClick()
        composeTestRule.onNodeWithTag("camera_mode_brooder").assertIsSelected()
        composeTestRule.onNodeWithTag("camera_mode_incubator").assertIsNotSelected()
        composeTestRule.onNodeWithTag("camera_mode_active_model")
            .assertTextEquals("Model: chick")

        // And back to incubator.
        composeTestRule.onNodeWithTag("camera_mode_incubator").performClick()
        composeTestRule.onNodeWithTag("camera_mode_incubator").assertIsSelected()
        composeTestRule.onNodeWithTag("camera_mode_active_model")
            .assertTextEquals("Model: incubation")
    }
}
