"""File discovery and rule dispatch."""

from __future__ import annotations

import os

from .findings import FileReport, Finding
from .lang import DEFAULT_EXTENSIONS, spec_for
from .rules import agnostic, clojure, julia

# Directories we never descend into.
EXCLUDE_DIRS = {
    ".git", ".jj", ".hg", ".svn", "target", "node_modules", ".venv", "venv",
    "__pycache__", "dist", "build", ".next", ".cargo", ".serena", ".agents",
    ".mypy_cache", ".pytest_cache", ".gradle", "vendor", ".deslop-bak",
}


def discover_files(paths: list[str], extensions: tuple[str, ...] = DEFAULT_EXTENSIONS) -> list[str]:
    found: list[str] = []
    for p in paths:
        if os.path.isfile(p):
            found.append(p)
        elif os.path.isdir(p):
            for root, dirs, files in os.walk(p):
                dirs[:] = [d for d in dirs if d not in EXCLUDE_DIRS]
                for name in sorted(files):
                    if os.path.splitext(name)[1].lower() in extensions:
                        found.append(os.path.join(root, name))
    # stable, de-duplicated
    seen, out = set(), []
    for f in found:
        rp = os.path.normpath(f)
        if rp not in seen:
            seen.add(rp)
            out.append(rp)
    return out


def read_lines(path: str) -> list[str] | None:
    try:
        with open(path, "r", encoding="utf-8") as fh:
            return fh.read().split("\n")
    except (UnicodeDecodeError, OSError):
        return None


def scan_file(path: str) -> FileReport | None:
    lines = read_lines(path)
    if lines is None:
        return None
    spec = spec_for(path)
    findings: list[Finding] = list(agnostic.check(path, lines, spec))
    if spec.name == "clojure":
        findings += clojure.check(path, lines)
    elif spec.name == "julia":
        findings += julia.check(path, lines)
    findings.sort(key=lambda f: f.sort_key())
    return FileReport(path=path, lang=spec.name, lines=lines, findings=findings)


def scan(paths: list[str], extensions: tuple[str, ...] = DEFAULT_EXTENSIONS) -> list[FileReport]:
    reports: list[FileReport] = []
    for path in discover_files(paths, extensions):
        report = scan_file(path)
        if report is not None:
            reports.append(report)
    return reports
