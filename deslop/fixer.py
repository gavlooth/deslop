"""Deterministic, offline auto-fixes applied in place.

Only the safe, exact transformations run here:
  * within-line idiom substitutions carried on fixable Findings
    (e.g. ``length(x) == 0`` -> ``isempty(x)``)
  * collapsing runs of >=2 blank lines to a single blank line

Everything else (comment removal, structural rewrites) is left to ``slim`` or
the human, because guessing wrong there silently changes meaning.
"""

from __future__ import annotations

from dataclasses import dataclass

from .findings import FileReport


@dataclass
class FixResult:
    path: str
    applied: int          # number of substitutions applied
    blank_runs: int       # number of blank-line runs collapsed
    changed: bool


def apply_fixes(report: FileReport) -> tuple[list[str], FixResult]:
    """Return (new_lines, result). Does not write to disk."""
    lines = list(report.lines)
    applied = 0

    # 1. within-line substitutions. Apply per line; a finding's fix_old must
    #    still be present (earlier subs may have removed it).
    by_line: dict[int, list] = {}
    for f in report.findings:
        if f.fixable:
            by_line.setdefault(f.line - 1, []).append(f)
    for idx, fs in by_line.items():
        for f in fs:
            if f.fix_old and f.fix_old in lines[idx]:
                lines[idx] = lines[idx].replace(f.fix_old, f.fix_new, 1)
                applied += 1

    # 2. collapse consecutive blank lines.
    collapsed: list[str] = []
    blank_runs = 0
    run = 0
    for line in lines:
        if line.strip() == "":
            run += 1
            if run == 1:
                collapsed.append(line)
            elif run == 2:
                blank_runs += 1
            # runs >=2: skip the extra blanks
        else:
            run = 0
            collapsed.append(line)

    changed = collapsed != list(report.lines)
    return collapsed, FixResult(report.path, applied, blank_runs, changed)
