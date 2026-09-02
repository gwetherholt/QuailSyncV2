# Maestro UI flows

**A feature exists if and only if it has a passing Maestro flow.**

Each flow is two things at once: a regression test, and the screenshot-backed record of
one feature. `docs/FEATURES.md` is generated from the flow headers plus the results of a
real run — it is never hand-edited.

## Layout

```
maestro/
  flows/<screen>-<action>.yaml   one flow per feature
  screenshots/<flow-name>/       collected by run.ps1, one dir per flow
  run.ps1                        runs every flow, writes results.json
  results.json                   run output (gitignored)
scripts/gen_features.py          flows + results.json -> docs/FEATURES.md
```

## Conventions

**One flow per feature.** If a flow needs "and also", it is two features.

**File name: `<screen>-<action>.yaml`** — lowercase, hyphenated. `flock-list.yaml`,
`dashboard-overview.yaml`. The file stem is the flow's identity: it names the screenshot
directory and the key in `results.json`, so renaming a file orphans its history.

**Header block.** Every flow starts with these four lines, in this order, above the `---`:

```yaml
# feature: Flock list
# description: Opens the Flock tab and shows the list of birds loaded from the server.
appId: com.quailsync.app
name: flock-list
---
```

- `# feature:` — the heading in `docs/FEATURES.md`. Prose, capitalised, no trailing period.
- `# description:` — one sentence, what a reader sees working. Present tense.
- `appId:` — always `com.quailsync.app`. There is one `applicationId` across all build
  variants, so this never varies.
- `name:` — must equal the file stem.

A flow missing `# feature:` still appears in `FEATURES.md`, titled from its file name and
flagged with a warning. Don't rely on that.

**Every flow ends with at least one `assertVisible` and one `takeScreenshot`.** The
assertion is what makes it a test; the screenshot is what makes it a record. A flow that
taps through a screen and asserts nothing proves nothing. Name the screenshot after the
flow (`takeScreenshot: flock-list`); a flow capturing several screens suffixes them
(`flock-list-filtered`).

**Assert on something the target screen alone shows.** This is the easiest mistake to
make here. Every screen's title is *also* its bottom-nav label, visible from every other
tab — so `tapOn: "Flock"` followed by `assertVisible: "Flock"` passes even when the tap
never lands. Watch for near-misses too: the Flock header reads "19 birds", but the
Dashboard's hutch cards read "7 birds", so a bare `[0-9]+ birds` doesn't discriminate
either. Pick an element that exists on one screen only:

| Screen | Discriminating selector |
| --- | --- |
| Dashboard | `Birds` / `Eggs` / `Groups` / `Next Hatch` (stat pills) |
| Flock | `Records` (filter chip) |
| Hatchery | `Clutches` / `Chick Groups` / the empty-state placeholder |

The check: would this assertion still pass if the app were sitting on the Dashboard? If
yes, it isn't testing anything. Worth confirming for real — point a throwaway flow at the
wrong screen and watch the assertion fail before you trust it.

**Flows run against real data.** The app talks to the live backend — the Rust server on
the Pi. Nothing is mocked. So assert on chrome that is always present rather than on
specific birds or counts, which change between runs. Where an assertion has to cover both
a populated and an empty screen, alternate in a regex, as `hatchery-list.yaml` does.

**Read-only for now.** No flow may create, edit, or delete records: they run against
production data, and those paths are known-broken pending KAN-27 / KAN-28 / KAN-29.

## Selectors: text only

The app sets Compose `Modifier.testTag(...)` in several places (`nav_dashboard`,
`flock_bird_list`, `hatchery_list`, …), **but those tags are not visible to Maestro.**
Compose only exposes `testTag` as a UiAutomator resource-id when the app opts in with
`Modifier.semantics { testTagsAsResourceId = true }` on a root composable, and QuailSync
does not set it anywhere. Maestro therefore sees only text and content descriptions.

So: select by visible text. If a screen has an element you cannot reach by text, don't
work around it with brittle coordinates — note the element and stop. Exposing the existing
tags is a one-line app change, but an app change all the same (out of scope for KAN-1).

## Running locally

```powershell
pwsh maestro/run.ps1                        # all flows
pwsh maestro/run.ps1 -Flow dashboard-overview
pwsh maestro/run.ps1 -Device emulator-5554  # when more than one device is attached
python scripts/gen_features.py              # regenerate docs/FEATURES.md
```

Needs an emulator or device with `com.quailsync.app` installed and reachable backend
(`adb devices` to check).

### The CLI on this machine

There is **no `maestro` on PATH**, and Maestro publishes no standalone Windows CLI — the
usual advice is WSL2, and the only WSL distro here is Docker Desktop's, which can't run it.

It turns out Maestro Studio for Windows bundles the entire CLI: `maestro.cli.AppKt` lives
in `studio-server.jar` alongside its own JRE 17. `run.ps1` invokes that directly, so no
WSL and no extra install:

```
%LOCALAPPDATA%\Programs\Maestro Studio\resources\app.asar.unpacked\
    bundled-jvm\windows-x64\bin\java.exe
    dist-server\studio-server.jar          -> maestro.cli.AppKt
```

`run.ps1` prefers, in order: `$env:MAESTRO_CMD`, a `maestro` on PATH, then this bundle. If
a real CLI is installed later, the script picks it up with no edit. This is an
undocumented internal layout, so a Studio upgrade could move it — if `run.ps1` stops
finding the CLI, that's the first place to look.

### Where screenshots go

`takeScreenshot: <name>` writes `<name>.png` into Maestro's **working directory**. The
location is not configurable from YAML or a CLI flag. `run.ps1` works around it: each flow
runs in its own temporary working directory, and the PNGs it drops there are moved into
`maestro/screenshots/<flow-name>/`. Flows stay path-free, and screenshots can't collide
between flows.

## Recording a new flow

1. Record it in Maestro Studio against the emulator.
2. Save it to `maestro/flows/<screen>-<action>.yaml` and add the header block.
3. Confirm it ends with an `assertVisible` and a `takeScreenshot`.
4. `pwsh maestro/run.ps1 -Flow <name>` — it must pass from a cold `launchApp`, not just
   from wherever Studio left the app.
5. `python scripts/gen_features.py` and check the new section in `docs/FEATURES.md`.
