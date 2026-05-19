"""End-to-end Playwright tests for the QuailSync dashboard SPA.

All tests run against a freshly seeded dev DB (see conftest.seed_test_data).
Seed contents (from crates/quailsync-server/src/routes/dev.rs insert_basic_seed):
  - 5 lineages: Pharaoh, Texas A&M, Italian, English White, Tibetan
  - 5 housing units: 1 incubator, 2 brooders, 2 hutches (Hutch A, Hutch B)
  - 4 chick groups: Group #1 incubating (future hatch), #2 12d, #3 35d (ready
    to band), #4 already Graduated
  - 15 birds (8 adults in hutches, 7 chicks)

The SPA fades content over ~180ms after a hashchange — `wait_for_timeout(300)`
is used only at those boundaries; everywhere else we rely on expect()'s
auto-wait.
"""

import re

from playwright.sync_api import Page, expect


# 300ms covers the 180ms CSS fade plus a small safety margin for the render
# pass inside setTimeout. Stay at 300 max per spec.
ROUTE_FADE_MS = 300


def _navigate(page: Page, hash_route: str) -> None:
    """Drive the SPA router by writing to location.hash, then wait out the
    fade-in. Using sidebar clicks works too, but the hash-write path is what
    every test except TestNavigation needs."""
    page.evaluate(f"location.hash = '{hash_route}'")
    page.wait_for_timeout(ROUTE_FADE_MS)


# =====================================================================
# Dashboard
# =====================================================================


class TestDashboardPage:
    def test_housing_sections_render(self, page: Page):
        _navigate(page, "#/dashboard")
        # Seed: 1 incubator, 2 brooders, 2 hutches. Section headers are
        # rendered as <h3>{icon} {label} ({count})</h3>.
        expect(page.locator("h3", has_text="Incubators (1)")).to_be_visible()
        expect(page.locator("h3", has_text="Brooders (2)")).to_be_visible()
        expect(page.locator("h3", has_text="Hutches (2)")).to_be_visible()

    def test_housing_cards_show_life_stage_badges(self, page: Page):
        _navigate(page, "#/dashboard")
        # Each housing card has a stage badge inside [data-brooder-id]. Seed:
        # 3 Chick-stage cards (1 incubator + 2 brooders), 2 Adult-stage cards
        # (2 hutches). Use exact text match to avoid grabbing the "Chick"
        # word from elsewhere (e.g. residents pluralization "chicks").
        chick_badges = page.locator(
            "[data-brooder-id] span", has_text=re.compile(r"^Chick$")
        )
        adult_badges = page.locator(
            "[data-brooder-id] span", has_text=re.compile(r"^Adult$")
        )
        expect(chick_badges).to_have_count(3)
        expect(adult_badges).to_have_count(2)

    def test_quick_stats_panel(self, page: Page):
        _navigate(page, "#/dashboard")
        # Header lives in .panel-header-left; we match by substring because
        # the source includes a leading emoji entity.
        expect(
            page.locator(".panel-header-left", has_text="Quick Stats")
        ).to_be_visible()
        stats_body = page.locator("#d-stats")
        expect(stats_body).to_be_visible()
        # Seed creates 15 birds; the first qs-card is "Total Birds". Don't
        # hard-code 15 (the dashboard counts may include future seed tweaks),
        # just assert the slot contains a number.
        nums = stats_body.locator(".qs-num")
        expect(nums.first).to_have_text(re.compile(r"^\d+$"))
        # Sanity: the four qs-cards from _pollStats should all render.
        expect(nums).to_have_count(4)

    def test_alerts_section_visible(self, page: Page):
        _navigate(page, "#/dashboard")
        expect(page.locator(".panel-header-left", has_text="Alerts")).to_be_visible()
        alerts_body = page.locator("#d-alerts")
        expect(alerts_body).to_be_visible()
        # Either the "all clear" empty-state line or rendered alert items —
        # both are valid for a freshly seeded DB. The element should at least
        # have non-whitespace text content once the poll resolves.
        expect(alerts_body).not_to_have_text(re.compile(r"^\s*$"))

    def test_add_housing_button_present(self, page: Page):
        _navigate(page, "#/dashboard")
        expect(
            page.get_by_role("button", name="+ Add Housing")
        ).to_be_visible()


# =====================================================================
# Flock
# =====================================================================


class TestFlockPage:
    def test_bird_table_renders_with_columns(self, page: Page):
        _navigate(page, "#/flock")
        thead = page.locator(".data-table thead")
        expect(thead).to_be_visible()
        # Header text in the DOM is title-case ("Band"); CSS uppercases it
        # for display. Playwright matches DOM text, so use the source form.
        for col in ["ID", "Band", "Sex", "Lineage", "Hatch", "Gen", "Status", "Parents"]:
            expect(thead.locator("th", has_text=re.compile(rf"^{col}$"))).to_be_visible()
        # Seed has 15 birds; at least one data row must be present.
        rows = page.locator("#flock-tbody tr")
        expect(rows.first).to_be_visible()
        assert rows.count() >= 1

    def test_filter_buttons_present(self, page: Page):
        _navigate(page, "#/flock")
        bar = page.locator("#flock-filters")
        expect(bar).to_be_visible()
        for label in ["All", "Active", "Males", "Females"]:
            expect(
                bar.locator(".filter-btn", has_text=re.compile(rf"^{label}$"))
            ).to_be_visible()
        # Lineage buttons follow the four status buttons. Seed defines 5
        # lineages; verify at least one of them shows up.
        expect(
            bar.locator(".filter-btn", has_text=re.compile(r"^Pharaoh$"))
        ).to_be_visible()

    def test_sex_filter_works(self, page: Page):
        _navigate(page, "#/flock")
        bar = page.locator("#flock-filters")
        bar.locator(".filter-btn", has_text=re.compile(r"^Males$")).click()
        # After filter, the SEX column (3rd td) should contain Male in every
        # visible row and never Female. Use locator counts so the assertion
        # waits for re-render.
        rows = page.locator("#flock-tbody tr")
        expect(rows.first).to_be_visible()
        female_cells = page.locator(
            "#flock-tbody tr td:nth-child(3)", has_text=re.compile(r"^Female$")
        )
        expect(female_cells).to_have_count(0)
        # Reset to All and verify rows are restored (seed has both sexes, so
        # total row count should grow back).
        male_only = rows.count()
        bar.locator(".filter-btn", has_text=re.compile(r"^All$")).click()
        # Auto-wait for re-render — assert count strictly greater.
        expect(rows).not_to_have_count(male_only)
        assert rows.count() > male_only

    def test_add_bird_modal_opens(self, page: Page):
        _navigate(page, "#/flock")
        page.get_by_role("button", name="+ Add Bird").click()
        modal = page.locator("#bird-modal-overlay")
        expect(modal).to_be_visible()
        # Labels in the modal — read directly from the modal markup so we
        # don't accidentally match a stray "Sex" elsewhere on the page.
        expect(modal.locator("label", has_text=re.compile(r"^Sex$"))).to_be_visible()
        expect(modal.locator("label", has_text=re.compile(r"^Band Color$"))).to_be_visible()
        expect(modal.locator("label", has_text=re.compile(r"^Hatch Date$"))).to_be_visible()
        # The lineage dropdown should be populated from the seed.
        lineage_select = modal.locator("#m-lineage")
        expect(lineage_select).to_be_visible()
        expect(lineage_select.locator("option", has_text="Pharaoh")).to_have_count(1)
        # Close the modal so we leave the DOM clean.
        modal.get_by_role("button", name="Cancel").click()
        expect(modal).to_have_count(0)


# =====================================================================
# Nursery
# =====================================================================


class TestNurseryPage:
    def test_active_groups_render(self, page: Page):
        _navigate(page, "#/nursery")
        # Active cards live in the first .clutch-grid and are NOT muted.
        active_cards = page.locator(".clutch-card:not(.muted)")
        expect(active_cards.first).to_be_visible()
        # Status badge for active groups carries class "active" and the
        # textContent is "Active" (CSS uppercases it for display).
        expect(
            active_cards.first.locator(".status-badge.active", has_text="Active")
        ).to_be_visible()
        # First active card should show count "23/25" or "24/24" style.
        first_card = active_cards.first
        first_egg_nums = first_card.locator(".egg-num")
        # Three .egg-num slots: count, age, mortality. Match each by format.
        expect(first_egg_nums.nth(0)).to_have_text(re.compile(r"^\d+/\d+$"))
        expect(first_egg_nums.nth(1)).to_have_text(re.compile(r"^-?\d+d$"))
        expect(first_egg_nums.nth(2)).to_have_text(re.compile(r"^\d+%$"))

    def test_completed_groups_show_graduated(self, page: Page):
        _navigate(page, "#/nursery")
        # The Completed section is rendered as an <h3> with "Completed", then
        # a clutch-grid of muted cards. Seed includes one Graduated group.
        expect(page.locator("h3", has_text=re.compile(r"^Completed$"))).to_be_visible()
        graduated_badges = page.locator(
            ".clutch-card.muted .status-badge", has_text="Graduated"
        )
        expect(graduated_badges.first).to_be_visible()

    def test_band_button_on_eligible_groups(self, page: Page):
        _navigate(page, "#/nursery")
        # In the seed only Group #3 (age 35d) qualifies for banding. The
        # button text is exact, so a single visible match is enough.
        band_btn = page.get_by_role("button", name="Band This Group")
        expect(band_btn.first).to_be_visible()


# =====================================================================
# Navigation
# =====================================================================


class TestNavigation:
    # Routes from dashboard/index.html Router.route() — kept in click order.
    ROUTES = [
        ("#/flock", "Flock Management"),
        ("#/clutches", "Clutch Tracker"),
        ("#/nursery", "Nursery"),
        ("#/breeding", "Breeding"),
        ("#/processing", "Processing"),
        ("#/cameras", "Cameras"),
        ("#/nfc/scan", "Scan NFC"),
        ("#/settings", "Settings"),
        ("#/dashboard", "Dashboard"),
    ]

    def test_sidebar_navigation_all_pages(self, page: Page):
        for hash_route, expected_header in self.ROUTES:
            # The sidebar uses data-route to identify each link.
            page.locator(f'.sidebar-item[data-route="{hash_route}"]').click()
            page.wait_for_timeout(ROUTE_FADE_MS)
            assert (
                page.evaluate("location.hash") == hash_route
            ), f"expected hash {hash_route}, got {page.evaluate('location.hash')}"
            expect(page.locator("#page-header")).to_have_text(expected_header)


# =====================================================================
# Graduation flow (full integration)
# =====================================================================


class TestGraduationFlow:
    def test_graduate_chick_group(self, page: Page):
        # ---- 1. Land on nursery and locate the eligible group ----
        _navigate(page, "#/nursery")
        active_cards = page.locator(".clutch-card:not(.muted)")
        expect(active_cards.first).to_be_visible()
        # The "Band This Group" button only appears on groups that have
        # passed the 28-day threshold (seed: just Group #3 at 35d). Find the
        # card that owns the visible button so we can read its title.
        card_with_band = active_cards.filter(has=page.get_by_role("button", name="Band This Group"))
        expect(card_with_band).to_have_count(1)
        group_title_text = card_with_band.locator("h4").inner_text()
        # h4 looks like "Group #3 — Pharaoh Active" — pull the integer that
        # follows the # for the modal title assertion below.
        m = re.search(r"Group #(\d+)", group_title_text)
        assert m, f"could not parse group id from {group_title_text!r}"
        group_id = int(m.group(1))

        # ---- 2. Open the banding modal ----
        card_with_band.get_by_role("button", name="Band This Group").click()
        modal = page.locator("#grad-modal-overlay")
        expect(modal).to_be_visible()
        # Title format: "Band Group #N — X Chicks". X comes from the group's
        # current_count; for the seed's Group #3 it's 17 (initial 18, 1 lost).
        expect(modal.locator("h3")).to_have_text(
            re.compile(rf"^Band Group #{group_id} — \d+ Chicks$")
        )

        # ---- 3. Quick-fill: Female + "blue" ----
        modal.locator("#qf-sex").select_option("Female")
        modal.locator("#qf-band").fill("blue")
        modal.get_by_role("button", name="Apply").click()
        # Each per-chick row should now reflect the apply: sex=Female and
        # band=blue. Check at least the first row.
        rows = modal.locator("#grad-tbody tr")
        expect(rows.first).to_be_visible()
        expect(rows.first.locator(".grad-sex")).to_have_value("Female")
        expect(rows.first.locator(".grad-band")).to_have_value("blue")

        # ---- 4. Pick a hutch (seed has Hutch A / Hutch B) ----
        hutch_select = modal.locator("#grad-target-housing")
        # Options include the "(Don't assign — place later)" sentinel at
        # value="" plus real hutches. Pick the first real hutch.
        real_hutch_values = hutch_select.evaluate(
            "el => Array.from(el.options).filter(o => o.value).map(o => o.value)"
        )
        if real_hutch_values:
            hutch_select.select_option(value=real_hutch_values[0])

        # ---- 5. Submit and wait for the modal to close ----
        modal.get_by_role("button", name="Graduate & Create Birds").click()
        expect(page.locator("#grad-modal-overlay")).to_have_count(0)

        # ---- 6. Verify graduation took effect on the flock page ----
        # submitGraduate() navigates to #/flock on success; wait for the
        # fade, then filter to Females and confirm at least one row has the
        # "blue" band dot that the quick-fill applied.
        page.wait_for_timeout(ROUTE_FADE_MS)
        assert page.evaluate("location.hash") == "#/flock"
        page.locator("#flock-filters .filter-btn", has_text=re.compile(r"^Females$")).click()
        # Each row whose 3rd cell says "Female" and which contains a
        # .band-dot[title="blue"] is a freshly-graduated bird. The seed has
        # zero Female birds with the literal band string "blue" before this
        # test (Blue/Yellow/Orange/Pink seed birds use capital-B "Blue"),
        # so any match here came from the graduation.
        blue_females = page.locator(
            "#flock-tbody tr:has(td:nth-child(3):text-is('Female')):has(.band-dot[title='blue'])"
        )
        # Auto-wait for the post-filter re-render to land a match — using
        # expect() instead of .count() keeps the assertion deflake-safe.
        expect(blue_females.first).to_be_visible()
        assert blue_females.count() >= 1
