# QuailSync Android — Features

<!-- GENERATED FILE — do not edit by hand. Regenerate with: pwsh maestro/run.ps1 && python scripts/gen_features.py -->

A feature is listed here only if it has a Maestro flow. Status and screenshots come from a real run against a real device talking to the real backend — nothing on this page is written by hand.

- **Last run:** 2026-09-02T23:23:55Z
- **Maestro:** 2.6.1
- **Device:** emulator-5554

**3 passing · 0 failing · 0 not run**

| Feature | Status | Flow |
| --- | --- | --- |
| [Dashboard overview](#dashboard-overview) | ✅ **PASSING** | `dashboard-overview` |
| [Flock list](#flock-list) | ✅ **PASSING** | `flock-list` |
| [Hatchery list](#hatchery-list) | ✅ **PASSING** | `hatchery-list` |

---

## Dashboard overview

✅ **PASSING**

Opens the Dashboard tab and shows the quick-stats row (Birds, Eggs, Groups, Next Hatch) populated from the server.

- **Flow:** [`maestro/flows/dashboard-overview.yaml`](../maestro/flows/dashboard-overview.yaml)
- **App:** `com.quailsync.app`
- **Last run:** 2026-09-02T23:23:01Z
- **Duration:** 17.8s

![Dashboard overview — dashboard-overview](../maestro/screenshots/dashboard-overview/dashboard-overview.png)

---

## Flock list

✅ **PASSING**

Opens the Flock tab and shows the list of birds loaded from the server.

- **Flow:** [`maestro/flows/flock-list.yaml`](../maestro/flows/flock-list.yaml)
- **App:** `com.quailsync.app`
- **Last run:** 2026-09-02T23:23:19Z
- **Duration:** 18.9s

![Flock list — flock-list](../maestro/screenshots/flock-list/flock-list.png)

---

## Hatchery list

✅ **PASSING**

Opens the Hatchery tab and shows the clutch and chick-group sections loaded from the server.

- **Flow:** [`maestro/flows/hatchery-list.yaml`](../maestro/flows/hatchery-list.yaml)
- **App:** `com.quailsync.app`
- **Last run:** 2026-09-02T23:23:38Z
- **Duration:** 17.5s

![Hatchery list — hatchery-list](../maestro/screenshots/hatchery-list/hatchery-list.png)

---
