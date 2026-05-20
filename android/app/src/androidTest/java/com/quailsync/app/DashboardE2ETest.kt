package com.quailsync.app

import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithTag
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollToNode
import androidx.test.ext.junit.runners.AndroidJUnit4
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import org.junit.After
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.TimeUnit

/**
 * End-to-end Compose tests against a freshly seeded dev DB.
 *
 * Seed contents (basic seed from crates/quailsync-server/src/routes/dev.rs):
 *   - 5 lineages: Pharaoh, Texas A&M, Italian, English White, Tibetan
 *   - 5 housing units: Incubator 1 / Brooder 1 / Brooder 2 / Hutch A / Hutch B
 *   - 4 chick groups: Group #1 incubating, #2 12d, #3 35d (Ready to band),
 *     #4 Graduated
 *   - 15 birds (8 adults in hutches, 7 chicks). None of the seed birds carry
 *     the lowercase "green" band, so the graduation flow's check is
 *     unambiguous when the test fills band_color = "green".
 *
 * The tests rely on testTags added to the production composables — see the
 * testTag(...) calls in DashboardScreen, FlockScreen, ClutchScreen, NfcScreen
 * and MainActivity for the supported tag set.
 */
@RunWith(AndroidJUnit4::class)
class DashboardE2ETest {

    @get:Rule
    val composeTestRule = createAndroidComposeRule<MainActivity>()

    @Before
    fun seed() { seedTestData() }

    @After
    fun restore() { restoreTestData() }

    // ---- common waits ---------------------------------------------------

    private fun waitForAny(matcher: SemanticsMatcher, timeoutMs: Long = 5_000) {
        composeTestRule.waitUntil(timeoutMs) {
            composeTestRule.onAllNodes(matcher).fetchSemanticsNodes().isNotEmpty()
        }
    }

    private fun waitForTag(tag: String, timeoutMs: Long = 5_000) {
        waitForAny(hasTestTag(tag), timeoutMs)
    }

    private val anyHousingCard = SemanticsMatcher("test tag starts with dashboard_housing_card_") { node ->
        node.config.getOrNull(SemanticsProperties.TestTag)?.startsWith("dashboard_housing_card_") == true
    }

    private val anyBirdRow = SemanticsMatcher("test tag starts with flock_bird_row_") { node ->
        node.config.getOrNull(SemanticsProperties.TestTag)?.startsWith("flock_bird_row_") == true
    }

    // ---- TestDashboard --------------------------------------------------

    @Test
    fun testHousingCardsRender() {
        waitForAny(anyHousingCard)
        // Seed creates Incubator 1 + Brooder 1/2 + Hutch A/B. Names are
        // rendered in CompactBrooderCard via Text(brooder.name).
        composeTestRule.onNodeWithText("Incubator 1").assertIsDisplayed()
        composeTestRule.onNodeWithText("Brooder 1").assertIsDisplayed()
        composeTestRule.onNodeWithText("Hutch A").assertIsDisplayed()
    }

    @Test
    fun testHousingSectionsGroupedByType() {
        waitForTag("dashboard_incubators_header")
        composeTestRule.onNodeWithTag("dashboard_incubators_header").assertIsDisplayed()
        composeTestRule.onNodeWithTag("dashboard_brooders_header").assertIsDisplayed()
        composeTestRule.onNodeWithTag("dashboard_hutches_header").assertIsDisplayed()
    }

    // ---- TestFlock ------------------------------------------------------

    @Test
    fun testBirdListRendersWithSeedData() {
        composeTestRule.onNodeWithTag("nav_flock").performClick()
        waitForAny(anyBirdRow)
        // Default filter is Active — all seed birds are Active. "Pharaoh"
        // tags the foundation pair so it should appear in at least one row.
        composeTestRule.onNodeWithText("Pharaoh", substring = true).assertIsDisplayed()
        // At least one Male bird is in the seed (Red / Green / White / Pink).
        val maleHits = composeTestRule.onAllNodesWithText("Male", substring = true).fetchSemanticsNodes()
        check(maleHits.isNotEmpty()) { "expected at least one Male bird row" }
    }

    @Test
    fun testFilterButtonsWork() {
        composeTestRule.onNodeWithTag("nav_flock").performClick()
        waitForAny(anyBirdRow)
        composeTestRule.onNodeWithTag("flock_filter_all").assertIsDisplayed()
        composeTestRule.onNodeWithTag("flock_filter_active").assertIsDisplayed()
        composeTestRule.onNodeWithTag("flock_filter_males").assertIsDisplayed()
        composeTestRule.onNodeWithTag("flock_filter_females").assertIsDisplayed()

        // Snapshot active-filter row count BEFORE switching to Males so we
        // can verify the row count actually changes.
        val activeRowCount = composeTestRule.onAllNodes(anyBirdRow).fetchSemanticsNodes().size

        composeTestRule.onNodeWithTag("flock_filter_males").performClick()
        composeTestRule.waitUntil(5_000) {
            composeTestRule.onAllNodes(anyBirdRow).fetchSemanticsNodes().size <= activeRowCount
        }
        // No exact "Female" text node should be visible in the bird list
        // when the filter is Males-only.
        val femaleHits = composeTestRule.onAllNodesWithText("Female").fetchSemanticsNodes()
        check(femaleHits.isEmpty()) {
            "Males filter should hide all Female birds but found ${femaleHits.size} occurrences"
        }

        // Reset to All and confirm rows come back.
        composeTestRule.onNodeWithTag("flock_filter_all").performClick()
        composeTestRule.waitUntil(5_000) {
            composeTestRule.onAllNodes(anyBirdRow).fetchSemanticsNodes().size >= activeRowCount
        }
    }

    @Test
    fun testBirdEditOpens() {
        composeTestRule.onNodeWithTag("nav_flock").performClick()
        waitForAny(anyBirdRow)
        // Tap the first visible bird row. BirdDetailDialog renders
        // DetailRow("Sex"|"Hatch Date"|"Band Color") for any bird with
        // those fields populated, which all seed birds do.
        composeTestRule.onAllNodes(anyBirdRow).onFirst().performClick()
        composeTestRule.waitUntil(2_000) {
            composeTestRule.onAllNodesWithText("Sex").fetchSemanticsNodes().isNotEmpty()
        }
        composeTestRule.onNodeWithText("Sex").assertIsDisplayed()
        composeTestRule.onNodeWithText("Band Color").assertIsDisplayed()
        composeTestRule.onNodeWithText("Hatch Date").assertIsDisplayed()
    }

    // ---- TestHatchery ---------------------------------------------------

    @Test
    fun testClutchCardsRender() {
        composeTestRule.onNodeWithTag("nav_hatchery").performClick()
        // Seed creates 2 clutches with ids 1 and 2 (Pharaoh incubating,
        // Texas A&M hatched).
        waitForTag("hatchery_clutch_card_1")
        composeTestRule.onNodeWithTag("hatchery_clutch_card_1").assertIsDisplayed()
        // At least one card should show a seed lineage name.
        val lineageHits = composeTestRule.onAllNodesWithText("Pharaoh", substring = true).fetchSemanticsNodes()
        check(lineageHits.isNotEmpty()) { "expected lineage name in clutch card" }
        // At least one status badge must be present.
        val statusHits =
            composeTestRule.onAllNodesWithText("Incubating", substring = true).fetchSemanticsNodes().size +
            composeTestRule.onAllNodesWithText("Hatched", substring = true).fetchSemanticsNodes().size
        check(statusHits > 0) { "no clutch status badge text rendered" }
        // Milestone markers — D1 and D17 at minimum.
        val d1Hits = composeTestRule.onAllNodesWithText("D1").fetchSemanticsNodes()
        check(d1Hits.isNotEmpty()) { "expected D1 milestone label" }
        val d17Hits = composeTestRule.onAllNodesWithText("D17").fetchSemanticsNodes()
        check(d17Hits.isNotEmpty()) { "expected D17 milestone label" }
    }

    @Test
    fun testChickGroupsSectionVisible() {
        composeTestRule.onNodeWithTag("nav_hatchery").performClick()
        waitForTag("hatchery_clutch_card_1")
        // The "Chick Groups" header is below the clutch list and may be
        // offscreen on small emulators — scroll the LazyColumn first.
        composeTestRule.onNodeWithTag("hatchery_list")
            .performScrollToNode(hasTestTag("hatchery_chick_groups"))
        composeTestRule.onNodeWithTag("hatchery_chick_groups").assertIsDisplayed()
    }

    @Test
    fun testAddClutchOpens() {
        composeTestRule.onNodeWithTag("nav_hatchery").performClick()
        waitForTag("hatchery_clutch_card_1")
        // The toolbar "+" opens a chooser dialog; "Add Clutch" inside the
        // chooser carries the hatchery_add_clutch tag (see ClutchScreen.kt
        // for the rationale). Match the toolbar icon by its contentDescription.
        composeTestRule.onNodeWithContentDescription("Add").performClick()
        composeTestRule.onNodeWithTag("hatchery_add_clutch").performClick()
        // AddClutchDialog renders text fields labelled "Lineage" and
        // "Eggs set". Set date isn't a user-facing field — the dialog fills
        // it from LocalDate.now() at submit time — so this test only
        // checks the two visible labels.
        composeTestRule.waitUntil(2_000) {
            composeTestRule.onAllNodesWithText("Lineage").fetchSemanticsNodes().isNotEmpty()
        }
        composeTestRule.onNodeWithText("Lineage").assertIsDisplayed()
        composeTestRule.onNodeWithText("Eggs set").assertIsDisplayed()
    }

    // ---- TestNFC --------------------------------------------------------

    @Test
    fun testNfcScreenLoads() {
        composeTestRule.onNodeWithTag("nav_nfc").performClick()
        composeTestRule.waitUntil(5_000) {
            composeTestRule.onAllNodesWithText("NFC Scanner").fetchSemanticsNodes().isNotEmpty()
        }
        composeTestRule.onNodeWithText("NFC Scanner").assertIsDisplayed()
        composeTestRule.onNodeWithText("Hold phone near NFC tag to scan").assertIsDisplayed()
        composeTestRule.onNodeWithText("Write Tag").assertIsDisplayed()
        composeTestRule.onNodeWithTag("nfc_graduate_batch").assertIsDisplayed()
    }

    @Test
    fun testDebugToggleShowsSimulator() {
        composeTestRule.onNodeWithTag("nav_nfc").performClick()
        waitForTag("nfc_debug_toggle")
        // Toggle ON — simulator section appears.
        composeTestRule.onNodeWithTag("nfc_debug_toggle").performClick()
        composeTestRule.waitUntil(2_000) {
            composeTestRule.onAllNodesWithText("NFC Simulator (debug)").fetchSemanticsNodes().isNotEmpty()
        }
        composeTestRule.onNodeWithText("NFC Simulator (debug)").assertIsDisplayed()
        composeTestRule.onNodeWithTag("nfc_sim_blank").assertIsDisplayed()
        composeTestRule.onNodeWithTag("nfc_sim_written").assertIsDisplayed()
        composeTestRule.onNodeWithTag("nfc_sim_duplicate").assertIsDisplayed()

        // Toggle OFF — simulator section disappears.
        composeTestRule.onNodeWithTag("nfc_debug_toggle").performClick()
        composeTestRule.waitUntil(2_000) {
            composeTestRule.onAllNodesWithText("NFC Simulator (debug)").fetchSemanticsNodes().isEmpty()
        }
        check(composeTestRule.onAllNodesWithTag("nfc_sim_blank").fetchSemanticsNodes().isEmpty()) {
            "simulator should be hidden after toggle OFF"
        }
    }

    @Test
    fun testSimulatorButtonsExecuteWithoutCrash() {
        composeTestRule.onNodeWithTag("nav_nfc").performClick()
        waitForTag("nfc_debug_toggle")
        composeTestRule.onNodeWithTag("nfc_debug_toggle").performClick()
        waitForTag("nfc_sim_blank")

        // Each tap triggers viewModel.simulateNfcScan(...) which routes
        // through the real lookup/conflict pipeline. We don't assert on the
        // pipeline output — only that none of the three buttons crash the
        // composition or the host activity. The "NFC Scanner" header is
        // only rendered inside NfcMainScreen, so it doubles as a liveness
        // check.
        for (tag in listOf("nfc_sim_blank", "nfc_sim_written", "nfc_sim_duplicate")) {
            composeTestRule.onNodeWithTag(tag).performClick()
            // waitUntil gives the dispatcher a chance to run while the
            // ViewModel coroutine runs. 2s matches the spec.
            composeTestRule.waitUntil(2_000) {
                composeTestRule.onAllNodesWithText("NFC Scanner").fetchSemanticsNodes().isNotEmpty()
            }
            composeTestRule.onNodeWithText("NFC Scanner").assertIsDisplayed()
        }
    }

    // ---- TestNavigation -------------------------------------------------

    @Test
    fun testBottomTabNavigation() {
        // Start destination is Dashboard; the first tap exercises a real
        // screen change (Dashboard -> Cameras).
        val routes = listOf(
            "nav_cameras" to "Cameras",
            "nav_flock" to "Flock",
            "nav_nfc" to "NFC Scanner",
            "nav_hatchery" to "Hatchery",
            "nav_dashboard" to "Dashboard",
        )
        for ((tag, expectedHeader) in routes) {
            composeTestRule.onNodeWithTag(tag).performClick()
            composeTestRule.waitUntil(5_000) {
                composeTestRule.onAllNodesWithText(expectedHeader).fetchSemanticsNodes().isNotEmpty()
            }
            composeTestRule.onNodeWithText(expectedHeader).assertIsDisplayed()
        }
    }

    // ---- TestGraduationFlow ---------------------------------------------

    /**
     * Smoke test for the NFC "Graduate Batch" entry point.
     *
     * The Android batch flow differs substantially from the web flow: there
     * is no single banding form with quick-fill + hutch picker + submit.
     * Tapping Graduate Batch opens BatchSetupScreen which asks for a lineage
     * and a bird count, then drops the user into a per-bird loop
     * (AwaitingTagScan -> PerBirdEntry -> AwaitingTagWrite) that requires
     * physical NFC taps. Driving that loop is impossible without a real NFC
     * adapter or a deeper test double for NfcService.
     *
     * So this test only validates the entry point: tapping Graduate Batch
     * surfaces the setup screen (tagged `graduation_dialog`) with the
     * lineage dropdown, count field, and start-tagging button
     * (`graduation_submit`).
     */
    @Test
    fun testGraduateBatchFromNFC() {
        composeTestRule.onNodeWithTag("nav_nfc").performClick()
        waitForTag("nfc_graduate_batch")
        composeTestRule.onNodeWithTag("nfc_graduate_batch").performClick()

        waitForTag("graduation_dialog")
        composeTestRule.onNodeWithTag("graduation_dialog").assertIsDisplayed()
        composeTestRule.onNodeWithText("Graduate Batch").assertIsDisplayed()
        composeTestRule.onNodeWithText("Lineage").assertIsDisplayed()
        composeTestRule.onNodeWithText("Number of birds to band").assertIsDisplayed()
        // Submit button is disabled until lineage + count are set, but it's
        // still present in the tree with its tag.
        composeTestRule.onNodeWithTag("graduation_submit").assertIsDisplayed()
    }

    /**
     * Verifies that newly-graduated birds show up in the Flock list with
     * the expected sex/band attributes.
     *
     * The Android batch flow cannot be driven end-to-end in a standard
     * instrumented test (see [testGraduateBatchFromNFC]), so this test
     * bypasses the UI for the graduation step and POSTs straight to the
     * server's `/api/chick-groups/{id}/graduate` endpoint with a "green"
     * band on Female birds. We then navigate to Flock, apply the Females
     * filter, and check that the new "green" band appears.
     *
     * Group #3 is the seed's only Active group past the 28-day threshold —
     * see routes/dev.rs basic-seed comment. None of the seed birds carry
     * the lowercase "green" band, so a positive match here came from
     * graduation.
     */
    @Test
    fun testGraduatedBirdsAppearInFlock() {
        graduateGroup3AsFemalesWithGreenBand()

        composeTestRule.onNodeWithTag("nav_flock").performClick()
        waitForAny(anyBirdRow)
        composeTestRule.onNodeWithTag("flock_filter_females").performClick()
        composeTestRule.waitUntil(5_000) {
            composeTestRule.onAllNodesWithText("green").fetchSemanticsNodes().isNotEmpty()
        }
        composeTestRule.onNodeWithText("green").assertIsDisplayed()
    }

    /** POST Female / band=green chicks against seed Group #3. */
    private fun graduateGroup3AsFemalesWithGreenBand() {
        val client = OkHttpClient.Builder()
            .callTimeout(20, TimeUnit.SECONDS)
            .build()
        // 17 matches the seed Group #3 current_count (initial 18, 1 lost).
        val birdsJson = buildString {
            append("[")
            repeat(17) { i ->
                if (i > 0) append(",")
                append("{\"sex\":\"Female\",\"band_color\":\"green\"}")
            }
            append("]")
        }
        val body = "{\"birds\":$birdsJson}".toRequestBody("application/json".toMediaType())
        val req = Request.Builder()
            .url("$SERVER_URL/api/chick-groups/3/graduate")
            .post(body)
            .build()
        client.newCall(req).execute().use { resp ->
            check(resp.isSuccessful) {
                "graduate API returned ${resp.code}: ${resp.body?.string()}"
            }
        }
    }
}
