#!/usr/bin/env python3
"""Decide whether a main commit should get a release tag.

Merging a release PR is the trigger. The commit is eligible when all of
these hold:

* Cargo.toml and Info.plist agree on a final X.Y.Z (no prerelease)
* that version is strictly newer than every existing final vX.Y.Z tag
* docs/release/notes/vX.Y.Z.zh.md and .en.md both exist and are non-empty
* vX.Y.Z does not already point at another commit

An existing tag is never moved. If it already points at this commit, the
result is noop so a retry can start the build without creating anything.
Pushing the tag with the workflow's GITHUB_TOKEN does not start
`.github/workflows/build.yml`; the workflow must dispatch that workflow
on the tag ref. A manually pushed tag still starts the build directly.

This script only prints a decision. It does not create refs.

    python3 scripts/plan-release-tag.py --self-test
    python3 scripts/plan-release-tag.py
"""

from __future__ import annotations

import argparse
import importlib.util
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FINAL_RE_TEXT = r"^\d+\.\d+\.\d+$"


@dataclass(frozen=True)
class Decision:
    action: str  # create, noop, skip, refuse
    version: str
    tag: str
    reason: str

    def lines(self) -> str:
        return (
            f"action={self.action}\n"
            f"version={self.version}\n"
            f"tag={self.tag}\n"
            f"reason={self.reason}\n"
        )


def _version_key(version: str) -> tuple[int, int, int]:
    parts = version.split(".")
    if len(parts) != 3 or not all(part.isdigit() for part in parts):
        raise ValueError(version)
    return tuple(int(part) for part in parts)  # type: ignore[return-value]


def _load_checker():
    path = ROOT / "scripts" / "check-version-consistency.py"
    spec = importlib.util.spec_from_file_location("check_version_consistency", path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def plan(
    *,
    version: str,
    consistent: bool,
    consistency_error: str,
    notes_present: bool,
    latest_other: str | None,
    existing_commit: str | None,
    head_sha: str,
) -> Decision:
    """Pure decision. `existing_commit` is what v<version> peels to, if any."""
    tag = f"v{version}" if version else ""
    if not version or len(version.split(".")) != 3 or not all(
        part.isdigit() for part in version.split(".")
    ):
        return Decision("skip", version, tag, "not_final_version")
    # An existing tag is never moved. Later commits of an already tagged
    # version, including ones with a plist drift, must not fail the tagger.
    if existing_commit is not None and existing_commit == head_sha:
        return Decision("noop", version, tag, "tag_already_at_head")
    if existing_commit is not None:
        return Decision("skip", version, tag, "tag_exists_elsewhere")
    if latest_other is not None and _version_key(version) <= _version_key(latest_other):
        return Decision("skip", version, tag, "not_newer_than_existing_tag")
    if not consistent:
        detail = consistency_error.replace("\n", " ").strip() or "version_mismatch"
        return Decision("refuse", version, tag, f"inconsistent:{detail}")
    if not notes_present:
        return Decision("refuse", version, tag, "missing_release_notes")
    return Decision("create", version, tag, "version_bump")


def notes_present(root: Path, version: str) -> bool:
    for lang in ("zh", "en"):
        path = root / "docs" / "release" / "notes" / f"v{version}.{lang}.md"
        if not path.is_file():
            return False
        if not path.read_text(encoding="utf-8").strip():
            return False
    return True


def _git(root: Path, *args: str) -> str:
    proc = subprocess.run(
        ["git", "-C", str(root), *args],
        check=False,
        capture_output=True,
        text=True,
    )
    if proc.returncode != 0:
        raise RuntimeError((proc.stderr or proc.stdout or "git failed").strip())
    return proc.stdout


def load_repository(root: Path, head: str = "HEAD") -> Decision:
    checker = _load_checker()
    head_sha = _git(root, "rev-parse", f"{head}^{{commit}}").strip()
    cargo_text = _git(root, "show", f"{head_sha}:Cargo.toml")
    plist_data = subprocess.run(
        ["git", "-C", str(root), "show", f"{head_sha}:Info.plist"],
        check=False,
        capture_output=True,
    )
    if plist_data.returncode != 0:
        raise RuntimeError("Info.plist missing at HEAD")
    try:
        version = checker.cargo_version(cargo_text)
    except ValueError as exc:
        return Decision("refuse", "", "", f"inconsistent:{exc}")
    errors = checker.check(cargo_text, plist_data.stdout, f"v{version}")
    listing = _git(root, "for-each-ref", "--format=%(refname:short) %(objectname) %(*objectname)", "refs/tags")
    latest: tuple[int, int, int] | None = None
    latest_text: str | None = None
    existing: str | None = None
    for line in listing.splitlines():
        parts = line.split()
        if len(parts) < 2:
            continue
        name, obj = parts[0], parts[1]
        peeled = parts[2] if len(parts) > 2 and parts[2] else obj
        if not name.startswith("v"):
            continue
        bare = name[1:]
        if bare == version:
            existing = peeled
            continue
        if len(bare.split(".")) != 3 or not all(part.isdigit() for part in bare.split(".")):
            continue
        key = _version_key(bare)
        if latest is None or key > latest:
            latest = key
            latest_text = bare
    return plan(
        version=version,
        consistent=not errors,
        consistency_error=errors[0] if errors else "",
        notes_present=notes_present(root, version),
        latest_other=latest_text,
        existing_commit=existing,
        head_sha=head_sha,
    )


def self_test() -> int:
    head = "a" * 40
    other = "b" * 40

    def decide(**kwargs: object) -> Decision:
        base = dict(
            version="1.3.16",
            consistent=True,
            consistency_error="",
            notes_present=True,
            latest_other="1.3.15",
            existing_commit=None,
            head_sha=head,
        )
        base.update(kwargs)
        return plan(**base)  # type: ignore[arg-type]

    created = decide()
    assert created.action == "create" and created.tag == "v1.3.16" and created.reason == "version_bump"
    assert decide(version="1.3.9", latest_other="1.3.15").action == "skip"
    assert decide(version="1.10.0", latest_other="1.9.0").action == "create"
    assert decide(version="1.3.16-rc1").action == "skip"
    assert decide(version="1.3.16-rc1").reason == "not_final_version"
    assert decide(existing_commit=head).action == "noop"
    assert decide(existing_commit=other).action == "skip"
    assert decide(existing_commit=other).reason == "tag_exists_elsewhere"
    # Current main after v1.3.15: the tag points at the release commit, not HEAD.
    steady = decide(version="1.3.15", latest_other="1.3.14", existing_commit=other)
    assert steady.action == "skip" and steady.reason == "tag_exists_elsewhere"
    assert decide(existing_commit=other, notes_present=False, consistent=False).action == "skip"
    assert decide(notes_present=False).action == "refuse"
    assert decide(notes_present=False).reason == "missing_release_notes"
    refused = decide(consistent=False, consistency_error="Info.plist mismatch")
    assert refused.action == "refuse" and refused.reason.startswith("inconsistent:")
    assert decide(latest_other=None).action == "create"
    assert decide(version="1.3.15", latest_other="1.3.15").reason == "not_newer_than_existing_tag"
    # Equal version already tagged at this commit stays a retryable noop,
    # even though it is not newer than the published tag.
    assert decide(version="1.3.15", latest_other="1.3.14", existing_commit=head).action == "noop"

    actions = {created.action, "noop", "skip", "refuse"}
    assert "move" not in actions and "force" not in actions

    workflow = (ROOT / ".github" / "workflows" / "tag-release.yml").read_text(encoding="utf-8")
    required = (
        "branches: [main]",
        "contents: write",
        "actions: write",
        "scripts/plan-release-tag.py",
        'git push origin "refs/tags/${TAG}:refs/tags/${TAG}"',
        'gh workflow run build.yml --ref "$TAG"',
        "github.token",
    )
    for snippet in required:
        if snippet not in workflow:
            raise AssertionError(f"tag-release.yml missing {snippet}")
    if "--force" in workflow or "git push --tags" in workflow or "refs --delete" in workflow:
        raise AssertionError("tag workflow can move or delete tags")
    if "workflow_dispatch:" not in (ROOT / ".github" / "workflows" / "build.yml").read_text(encoding="utf-8"):
        raise AssertionError("build.yml cannot be dispatched after a GITHUB_TOKEN tag push")
    print("plan-release-tag self-test: ok")
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--head", default="HEAD")
    parser.add_argument("--root", type=Path, default=ROOT)
    args = parser.parse_args(argv)
    if args.self_test:
        return self_test()
    try:
        decision = load_repository(args.root, args.head)
    except RuntimeError as exc:
        print(f"action=refuse\nversion=\ntag=\nreason=inconsistent:{exc}", file=sys.stderr)
        print(f"::error::{exc}")
        return 1
    sys.stdout.write(decision.lines())
    if decision.action == "refuse":
        print(f"::error title=Release tag not created::{decision.reason}")
        return 1
    if decision.action == "skip" and decision.reason == "not_newer_than_existing_tag":
        print(f"not tagging {decision.tag}: an equal or newer final tag already exists")
    return 0


if __name__ == "__main__":
    sys.exit(main())
