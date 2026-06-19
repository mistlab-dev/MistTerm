#!/usr/bin/env python3
"""汇总各 GUI 集成脚本的功能覆盖（需在脚本内启用 CoverageTracker）。"""

from __future__ import annotations

import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))

from gui_coverage import GUI_FEATURES  # noqa: E402


def main() -> int:
    manifest = ROOT / "scripts" / "gui_coverage_manifest.json"
    by_script: dict[str, list[str]] = {k: [] for k in ("smoke", "workflow", "e2e", "connect")}
    for fid, (_, owners) in GUI_FEATURES.items():
        for owner in owners.split("|"):
            if owner in by_script:
                by_script[owner].append(fid)

    manifest.write_text(
        json.dumps({"features": GUI_FEATURES, "by_script": by_script}, ensure_ascii=False, indent=2),
        encoding="utf-8",
    )
    print(f"Wrote {manifest}")
    print("\nFeature owners:")
    for script, ids in by_script.items():
        print(f"  {script}: {len(ids)} items")
    return 0


if __name__ == "__main__":
    sys.exit(main())
