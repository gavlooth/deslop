"""Language-agnostic bloat heuristics driven by comment tokens + structure.

These run on every file. They are intentionally conservative: false positives
erode trust faster than a few missed findings.
"""

from __future__ import annotations

import re

from ..findings import Finding
from ..lang import LangSpec

# Telltale openers of narration comments that just restate the next line of
# code ("# loop through the items", "; increment the counter", ...).
_NARRATION = re.compile(
    r"^(import|imports|initialize|initialise|define|defining|create|creating|"
    r"loop|looping|iterate|iterating|return|returning|set|setting|get|getting|"
    r"check|checking|increment|decrement|instantiate|call|calling|declare|"
    r"assign|print|printing|update|updating|add|adds|adding|remove|removing|"
    r"now|first|then|next|finally|step\s*\d+|handle|handling|store|storing|"
    r"compute|computing|calculate|calculating|convert|converting|build|"
    r"building|setup|configure|configuring|start|starting|end|begin|"
    r"this\s+(function|method|block|loop|line|variable|code)|"
    r"we\s+(now|then|will|need)|let'?s)\b",
    re.IGNORECASE,
)

# Comments that are pure section banners / dividers, common in LLM output.
_BANNER = re.compile(r"^[\s\-=*#/~_]{6,}$")


def _strip_comment(line: str, spec: LangSpec) -> tuple[str, int] | None:
    """Return (comment_text, start_col) if ``line``'s content is a comment.

    Matches both own-line comments and trailing inline comments. Returns None
    when there is no comment token. Naive about tokens inside strings, which is
    acceptable for a heuristic.
    """
    for tok in spec.line_comment:
        idx = line.find(tok)
        if idx == -1:
            continue
        return line[idx + len(tok):].strip(), idx
    return None


def check(path: str, lines: list[str], spec: LangSpec) -> list[Finding]:
    out: list[Finding] = []
    out += _narration_comments(path, lines, spec)
    out += _comment_blocks(path, lines, spec)
    out += _blank_runs(path, lines)
    out += _duplicate_blocks(path, lines)
    return out


def _narration_comments(path: str, lines: list[str], spec: LangSpec) -> list[Finding]:
    out: list[Finding] = []
    for i, line in enumerate(lines):
        parsed = _strip_comment(line, spec)
        if not parsed:
            continue
        text, col = parsed
        if not text or _BANNER.match(text):
            continue
        words = text.split()
        if len(words) > 9:  # long comments are usually real explanations
            continue
        if _NARRATION.match(text):
            code_before = line[:col].strip()
            kind = "inline" if code_before else "standalone"
            out.append(Finding(
                path=path, line=i + 1, rule="narrating-comment",
                severity="minor",
                message=f'comment restates the code ({kind}): "{text}"',
                suggestion="drop the comment; the code already says this",
            ))
    return out


def _comment_blocks(path: str, lines: list[str], spec: LangSpec) -> list[Finding]:
    """Runs of >=4 consecutive full-line comments (not the file header)."""
    out: list[Finding] = []
    run_start: int | None = None
    seen_code = False
    for i, line in enumerate(lines):
        stripped = line.strip()
        parsed = _strip_comment(line, spec)
        is_full_comment = bool(parsed) and stripped.startswith(spec.line_comment)
        if is_full_comment:
            if run_start is None:
                run_start = i
        else:
            if run_start is not None:
                _emit_block(out, path, run_start, i - 1, seen_code)
                run_start = None
            if stripped:
                seen_code = True
    if run_start is not None:
        _emit_block(out, path, run_start, len(lines) - 1, seen_code)
    return out


def _emit_block(out, path, start, end, seen_code):
    if not seen_code:  # leading file header / license block — leave alone
        return
    if end - start + 1 >= 4:
        out.append(Finding(
            path=path, line=start + 1, end_line=end + 1,
            rule="comment-block", severity="info",
            message=f"{end - start + 1}-line comment block — likely narration",
            suggestion="keep the 'why', delete the play-by-play",
        ))


def _blank_runs(path: str, lines: list[str]) -> list[Finding]:
    out: list[Finding] = []
    run_start: int | None = None
    for i, line in enumerate(lines):
        if line.strip() == "":
            if run_start is None:
                run_start = i
        else:
            if run_start is not None and i - run_start >= 2:
                out.append(Finding(
                    path=path, line=run_start + 1, end_line=i,
                    rule="consecutive-blank-lines", severity="info",
                    message=f"{i - run_start} consecutive blank lines",
                    suggestion="collapse to a single blank line",
                ))
            run_start = None
    return out


def _duplicate_blocks(path: str, lines: list[str], window: int = 6) -> list[Finding]:
    """Flag a >=``window``-line sequence that appears verbatim more than once."""
    if len(lines) < window * 2:
        return []
    norm = [l.strip() for l in lines]
    seen: dict[str, int] = {}
    out: list[Finding] = []
    reported_until = -1
    for i in range(len(lines) - window + 1):
        chunk = norm[i : i + window]
        if sum(1 for c in chunk if c) < window - 1:  # mostly blank → skip
            continue
        key = "\n".join(chunk)
        if key in seen and i > reported_until:
            first = seen[key]
            out.append(Finding(
                path=path, line=i + 1, end_line=i + window,
                rule="duplicate-block", severity="major",
                message=f"{window}+ lines duplicate the block at line {first + 1}",
                suggestion="extract a shared function/binding",
            ))
            reported_until = i + window - 1
        elif key not in seen:
            seen[key] = i
    return out
