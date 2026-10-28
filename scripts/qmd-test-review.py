"""Generate the advisory QMD review inventory with the canonical shared checker.

The shared CLI collects directories. This adapter also selects exact files so a
single build prerequisite does not expand the inventory to every vendor test.
It delegates selector discovery, hashes, rationale and review status to vigil-utils.
"""

from __future__ import annotations

import argparse
import json
import sys
import tomllib
from dataclasses import asdict
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--checker-root", type=Path, required=True)
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument("--sync-register", action="store_true")
    modes.add_argument("--check", action="store_true")
    args = parser.parse_args()
    sys.path.insert(0, str(args.checker_root.resolve()))

    from tools.doc_checks.config import load_config
    from tools.doc_checks.examine import Examination
    from tools.doc_checks.inventory import render_inventory
    from tools.doc_checks.rationale import (
        iter_test_files,
        missing_labels,
        missing_rationale,
        read_selectors,
        selector_id,
    )
    from tools.doc_checks.register import read_register, sync_register

    root = Path(__file__).resolve().parent.parent
    config_path = root / "docs/qmd-test-review.toml"
    config = load_config(config_path)
    explicit = tomllib.loads(config_path.read_text(encoding="utf-8"))["inventory"][
        "files"
    ]
    if not isinstance(explicit, list) or not all(
        isinstance(path, str) for path in explicit
    ):
        parser.error("inventory.files must be a list of paths")
    exam = Examination(root)
    paths = set()
    for name in config.test_dirs:
        directory = root / name
        if not directory.is_dir() or not directory.resolve().is_relative_to(root):
            parser.error(f"test directory missing or outside repository: {directory}")
        paths.update(iter_test_files(directory, config.ignore_parts, exam))
    paths.update(root / name for name in explicit)
    if not paths:
        parser.error("no test files selected")
    selectors = []
    for path in sorted(paths):
        if not path.resolve().is_relative_to(root):
            parser.error(f"selected file escapes repository: {path}")
        selectors.extend(read_selectors(path))
        exam.ok(path)
    if exam.unexamined:
        parser.error("; ".join(exam.lines()))

    register_path = root / "TEST_REVIEW.csv"
    if args.sync_register:
        sync_register(
            register_path, [selector_id(selector, root) for selector in selectors]
        )
    register = read_register(register_path)
    reviewed = [
        selector
        for selector in selectors
        if register.get(selector_id(selector, root))
        and register[selector_id(selector, root)].reviewed
    ]
    failures = [
        f"{selector_id(selector, root)}: {reason}"
        for selector, reason in missing_rationale(reviewed, config.min_words)
    ]
    failures.extend(
        f"{selector_id(selector, root)}: {reason}"
        for selector in reviewed
        for reason in missing_labels(selector, config.require_labels, root)
    )
    out = root / "TEST_INVENTORY.md"
    rendered, totals = render_inventory(
        selectors,
        register,
        root,
        out,
        config.min_words,
        config.reviewers,
        strict_commits=True,
    )
    print(json.dumps({"examined_files": exam.examined, **asdict(totals)}))
    if failures:
        print("\n".join(failures), file=sys.stderr)
    if args.check:
        if out.read_text(encoding="utf-8") != rendered:
            print("TEST_INVENTORY.md is stale", file=sys.stderr)
            return 1
    else:
        out.write_text(rendered, encoding="utf-8", newline="\n")
    return int(
        bool(failures or totals.stale or totals.invalid or totals.stale_register_rows)
    )


if __name__ == "__main__":
    raise SystemExit(main())
