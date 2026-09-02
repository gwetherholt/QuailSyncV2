#!/usr/bin/env python3
"""Generate docs/FEATURES.md from Maestro flow headers + a real run's results.

A feature exists if and only if it has a passing Maestro flow. This script is the
only writer of docs/FEATURES.md -- never hand-edit that file, it is regenerated
and overwritten.

Inputs:
    maestro/flows/*.yaml   the feature catalogue (`# feature:` / `# description:`
                           header comments, plus appId/name)
    maestro/results.json   written by maestro/run.ps1 from an actual device run

Output:
    docs/FEATURES.md

Usage:
    python scripts/gen_features.py [--check]

    --check  exit 1 if the generated file differs from what is on disk, without
             writing. Useful once this is wired into a pre-commit hook or CI.

Stdlib only: PyYAML is not in the project's requirements, and the header block we
care about is a handful of `key: value` lines, so it is parsed with plain string
handling rather than pulling in a dependency.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
FLOWS_DIR = REPO_ROOT / "maestro" / "flows"
RESULTS_PATH = REPO_ROOT / "maestro" / "results.json"
OUTPUT_PATH = REPO_ROOT / "docs" / "FEATURES.md"


def parse_flow(path: Path) -> dict:
    """Pull the header fields out of a Maestro flow.

    Everything above the first `---` separator is the flow's config block. We read
    two kinds of line from it: `# key: value` comment metadata (feature,
    description) and bare `key: value` Maestro config (appId, name). Steps below
    the separator are ignored -- they are the test, not the record.
    """
    header: dict[str, str] = {}
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if line == "---":
            break
        if line.startswith("#"):
            line = line.lstrip("#").strip()
        if ":" not in line:
            continue
        key, _, value = line.partition(":")
        key = key.strip().lower()
        if key in ("feature", "description", "appid", "name"):
            header[key] = value.strip().strip('"').strip("'")

    return {
        "flow": path.stem,
        "file": f"maestro/flows/{path.name}",
        # Fall back to the file name so a flow missing its header still shows up
        # rather than silently vanishing from the record.
        "feature": header.get("feature") or path.stem.replace("-", " ").title(),
        "description": header.get("description", ""),
        "app_id": header.get("appid", ""),
        "has_header": "feature" in header,
    }


def load_results() -> dict:
    if not RESULTS_PATH.exists():
        return {}
    try:
        return json.loads(RESULTS_PATH.read_text(encoding="utf-8-sig"))
    except json.JSONDecodeError as exc:
        sys.exit(f"error: {RESULTS_PATH} is not valid JSON ({exc}). Re-run maestro/run.ps1.")


def render(flows: list[dict], results: dict) -> str:
    by_flow = {r.get("flow"): r for r in results.get("flows", [])}

    lines: list[str] = []
    lines.append("# QuailSync Android — Features")
    lines.append("")
    lines.append(
        "<!-- GENERATED FILE — do not edit by hand. "
        "Regenerate with: pwsh maestro/run.ps1 && python scripts/gen_features.py -->"
    )
    lines.append("")
    lines.append(
        "A feature is listed here only if it has a Maestro flow. Status and screenshots "
        "come from a real run against a real device talking to the real backend — nothing "
        "on this page is written by hand."
    )
    lines.append("")

    if results:
        lines.append(f"- **Last run:** {results.get('generated_at', 'unknown')}")
        lines.append(f"- **Maestro:** {results.get('maestro_version', 'unknown')}")
        lines.append(f"- **Device:** {results.get('device', 'unknown')}")
    else:
        lines.append(
            "- **Last run:** never — `maestro/results.json` is missing. "
            "Run `pwsh maestro/run.ps1` first; every feature below is shown as *not run*."
        )
    lines.append("")

    # Summary table, so the state of the app is legible without scrolling.
    passed = sum(1 for f in flows if by_flow.get(f["flow"], {}).get("status") == "pass")
    failed = sum(1 for f in flows if by_flow.get(f["flow"], {}).get("status") == "fail")
    unrun = len(flows) - passed - failed
    lines.append(f"**{passed} passing · {failed} failing · {unrun} not run**")
    lines.append("")
    lines.append("| Feature | Status | Flow |")
    lines.append("| --- | --- | --- |")
    for flow in flows:
        result = by_flow.get(flow["flow"])
        badge = status_badge(result)
        anchor = anchor_for(flow["feature"])
        lines.append(f"| [{flow['feature']}](#{anchor}) | {badge} | `{flow['flow']}` |")
    lines.append("")
    lines.append("---")
    lines.append("")

    for flow in flows:
        result = by_flow.get(flow["flow"])
        lines.append(f"## {flow['feature']}")
        lines.append("")
        lines.append(f"{status_badge(result)}")
        lines.append("")
        if flow["description"]:
            lines.append(flow["description"])
            lines.append("")

        lines.append(f"- **Flow:** [`{flow['file']}`](../{flow['file']})")
        if flow["app_id"]:
            lines.append(f"- **App:** `{flow['app_id']}`")
        if result:
            lines.append(f"- **Last run:** {result.get('started_at', 'unknown')}")
            duration = result.get("duration_seconds")
            if duration is not None:
                lines.append(f"- **Duration:** {duration}s")
        else:
            lines.append("- **Last run:** never")
        if not flow["has_header"]:
            lines.append(
                "- ⚠️ Flow has no `# feature:` header — name above is derived from the "
                "file name. See `maestro/README.md`."
            )
        lines.append("")

        # A failing flow is reported, never dropped. The whole point of this page is
        # that it tells the truth about what works.
        if result and result.get("status") == "fail":
            lines.append("Maestro output (tail):")
            lines.append("")
            lines.append("```")
            lines.append(str(result.get("error") or "no output captured").rstrip())
            lines.append("```")
            lines.append("")

        screenshots = (result or {}).get("screenshots") or []
        if screenshots:
            for shot in screenshots:
                caption = Path(shot).stem
                # docs/FEATURES.md -> repo root is one level up.
                lines.append(f"![{flow['feature']} — {caption}](../{shot})")
                lines.append("")
        elif result and result.get("status") == "pass":
            lines.append(
                "_No screenshot captured — every flow must end with a `takeScreenshot`. "
                "See `maestro/README.md`._"
            )
            lines.append("")

        lines.append("---")
        lines.append("")

    # Any results with no matching flow file: the flow was renamed or deleted after
    # the last run, so the run record is stale.
    orphans = sorted(set(by_flow) - {f["flow"] for f in flows})
    if orphans:
        lines.append("## Stale run records")
        lines.append("")
        lines.append(
            "These flows appear in `maestro/results.json` but no longer exist in "
            "`maestro/flows/`. Re-run `maestro/run.ps1` to clear them."
        )
        lines.append("")
        for orphan in orphans:
            lines.append(f"- `{orphan}`")
        lines.append("")

    return "\n".join(lines).rstrip() + "\n"


def status_badge(result: dict | None) -> str:
    if not result:
        return "⚪ **NOT RUN**"
    if result.get("status") == "pass":
        return "✅ **PASSING**"
    return "❌ **FAILING**"


def anchor_for(heading: str) -> str:
    """GitHub-flavoured markdown heading anchor."""
    slug = "".join(c for c in heading.lower() if c.isalnum() or c in " -").strip()
    return slug.replace(" ", "-")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check",
        action="store_true",
        help="exit 1 if docs/FEATURES.md is out of date; do not write",
    )
    args = parser.parse_args()

    if not FLOWS_DIR.is_dir():
        sys.exit(f"error: {FLOWS_DIR} does not exist")

    flow_files = sorted(FLOWS_DIR.glob("*.yaml"))
    if not flow_files:
        sys.exit(f"error: no flows found in {FLOWS_DIR}")

    flows = [parse_flow(p) for p in flow_files]
    flows.sort(key=lambda f: f["feature"].lower())

    content = render(flows, load_results())

    if args.check:
        current = OUTPUT_PATH.read_text(encoding="utf-8") if OUTPUT_PATH.exists() else ""
        if current != content:
            print("docs/FEATURES.md is out of date. Run: python scripts/gen_features.py")
            return 1
        print("docs/FEATURES.md is up to date.")
        return 0

    OUTPUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT_PATH.write_text(content, encoding="utf-8")
    print(f"Wrote {OUTPUT_PATH.relative_to(REPO_ROOT)} ({len(flows)} feature(s))")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
