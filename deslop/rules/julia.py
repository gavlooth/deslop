"""Julia-specific bloat heuristics.

Catches reimplemented-stdlib idioms and redundant explicit returns. Fixable
findings carry exact within-line substitutions for ``deslop fix``.
"""

from __future__ import annotations

import re

from ..findings import Finding

_SIMPLE_RULES = []


def _rule(pattern, rule, severity, message, suggestion, fix=None):
    _SIMPLE_RULES.append((re.compile(pattern), fix, rule, severity, message, suggestion))


_rule(
    r"\blength\(([^()]+?)\)\s*==\s*0\b",
    "reimpl-isempty", "minor",
    "length(x) == 0 reimplements isempty",
    "use isempty(x)",
    fix=lambda m: (m.group(0), f"isempty({m.group(1).strip()})"),
)
_rule(
    r"\blength\(([^()]+?)\)\s*>\s*0\b",
    "reimpl-isempty", "minor",
    "length(x) > 0 reimplements !isempty",
    "use !isempty(x)",
    fix=lambda m: (m.group(0), f"!isempty({m.group(1).strip()})"),
)
_rule(
    r"\bfor\s+\w+\s+(?:in|=)\s+1:length\(([^()]+?)\)",
    "reimpl-eachindex", "minor",
    "1:length(x) reimplements eachindex",
    "iterate with eachindex(x)",
    fix=lambda m: (f"1:length({m.group(1).strip()})", f"eachindex({m.group(1).strip()})"),
)
_rule(
    r"==\s*nothing\b",
    "reimpl-isnothing", "minor",
    "== nothing reimplements isnothing",
    "use isnothing(x) / === nothing",
    fix=None,
)
_rule(
    r"!=\s*nothing\b",
    "reimpl-isnothing", "minor",
    "!= nothing reimplements !isnothing",
    "use !isnothing(x) / !== nothing",
    fix=None,
)


def check(path: str, lines: list[str]) -> list[Finding]:
    out: list[Finding] = []
    for i, line in enumerate(lines):
        code = _strip_comment(line)
        for rx, fix, rule, sev, msg, sug in _SIMPLE_RULES:
            m = rx.search(code)
            if not m:
                continue
            f_old = f_new = None
            if fix:
                f_old, f_new = fix(m)
                if f_old not in line:
                    f_old = f_new = None
            out.append(Finding(
                path=path, line=i + 1, rule=rule, severity=sev,
                message=msg, suggestion=sug, fix_old=f_old, fix_new=f_new,
            ))
    out += _redundant_return(path, lines)
    return out


def _strip_comment(line: str) -> str:
    in_str = False
    quote = ""
    i, n = 0, len(line)
    while i < n:
        ch = line[i]
        if in_str:
            if ch == "\\":
                i += 2
                continue
            if ch == quote:
                in_str = False
        elif ch in ('"', "'"):
            in_str = True
            quote = ch
        elif ch == "#":
            return line[:i]
        i += 1
    return line


_RETURN_RE = re.compile(r"^\s*return\b")
_END_RE = re.compile(r"^\s*end\b")


def _redundant_return(path: str, lines: list[str]) -> list[Finding]:
    """`return x` immediately before the closing `end` is redundant in Julia."""
    out: list[Finding] = []
    for i, line in enumerate(lines):
        if not _RETURN_RE.match(line):
            continue
        # find the next non-blank, non-comment line
        j = i + 1
        while j < len(lines) and (not lines[j].strip() or
                                  lines[j].strip().startswith("#")):
            j += 1
        if j < len(lines) and _END_RE.match(lines[j]):
            out.append(Finding(
                path=path, line=i + 1, rule="explicit-return", severity="info",
                message="explicit return as the final expression",
                suggestion="Julia returns the last expression; the return is optional",
            ))
    return out
