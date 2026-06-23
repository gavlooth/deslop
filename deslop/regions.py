"""Region extraction: given a line of interest, find the enclosing unit.

These helpers let ``slim`` send the smallest meaningful chunk (a top-level
Clojure form, a Julia ``function ... end`` block, or an indentation block) to
the model instead of whole files. All line numbers here are 0-based indices
into a ``list[str]`` of source lines unless noted otherwise.
"""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class Region:
    start: int  # 0-based, inclusive
    end: int    # 0-based, inclusive

    def text(self, lines: list[str]) -> str:
        return "\n".join(lines[self.start : self.end + 1])


# --------------------------------------------------------------------------
# Clojure: paren-balance aware, ignoring strings / comments / char literals.
# --------------------------------------------------------------------------

def clojure_balance(line: str, state: int, in_string: bool) -> tuple[int, bool]:
    """Return (paren_depth_delta_applied_to_state, in_string) after ``line``.

    ``state`` is the running open-paren depth. Handles ``;`` line comments,
    ``"..."`` strings with ``\\`` escapes, and ``\\(`` char literals.
    """
    i = 0
    n = len(line)
    while i < n:
        ch = line[i]
        if in_string:
            if ch == "\\":
                i += 2
                continue
            if ch == '"':
                in_string = False
            i += 1
            continue
        if ch == ";":
            break  # rest of line is a comment
        if ch == '"':
            in_string = True
            i += 1
            continue
        if ch == "\\":  # char literal, e.g. \( \) \space
            i += 2
            continue
        if ch in "([{":
            state += 1
        elif ch in ")]}":
            state -= 1
        i += 1
    return state, in_string


def clojure_toplevel_region(lines: list[str], target: int) -> Region:
    """Enclosing top-level form for the line at index ``target``.

    A top-level form starts where running paren depth is 0 and a delimiter
    opens; it ends when depth returns to 0.
    """
    # Walk from the top tracking depth so we know each top-level form's span.
    depth = 0
    in_string = False
    form_start: int | None = None
    forms: list[Region] = []
    for idx, line in enumerate(lines):
        had_open = False
        if form_start is None:
            # A form begins on the first line that pushes depth above 0.
            before = depth
            depth, in_string = clojure_balance(line, depth, in_string)
            if depth > 0 or (before == 0 and depth == 0 and _opens_and_closes(line)):
                form_start = idx
                had_open = True
        else:
            depth, in_string = clojure_balance(line, depth, in_string)
        if form_start is not None and depth <= 0 and not in_string:
            forms.append(Region(form_start, idx))
            form_start = None
            depth = 0
        _ = had_open
    if form_start is not None:
        forms.append(Region(form_start, len(lines) - 1))

    for region in forms:
        if region.start <= target <= region.end:
            return region
    return window_region(lines, target)


def _opens_and_closes(line: str) -> bool:
    """True if a line both opens and closes a form (single-line top form)."""
    depth, in_string = clojure_balance(line, 0, False)
    return depth == 0 and ("(" in line or "[" in line or "{" in line) and not in_string


# --------------------------------------------------------------------------
# Julia: keyword / end block matching.
# --------------------------------------------------------------------------

_JULIA_OPENERS = (
    "function", "macro", "struct", "module", "begin", "quote",
    "for", "while", "if", "let", "try", "do",
)


def julia_block_region(lines: list[str], target: int) -> Region:
    """Enclosing top-level ``<keyword> ... end`` block for ``target``.

    Prefers the outermost block (a top-level ``function``/``struct``/``module``)
    so the model sees the whole definition.
    """
    import re

    opener_re = re.compile(r"^\s*(?:@\w+\s+)?(" + "|".join(_JULIA_OPENERS) + r")\b")
    end_re = re.compile(r"^\s*end\b")

    # Build a stack of (start_idx, depth) to find the outermost block covering
    # the target line.
    stack: list[int] = []
    best: Region | None = None
    for idx, line in enumerate(lines):
        stripped = line.strip()
        if opener_re.match(line) and not _is_inline_julia_block(line):
            stack.append(idx)
        elif end_re.match(line) and stack:
            start = stack.pop()
            if start <= target <= idx:
                # outermost = smallest stack depth at open time
                if best is None or start < best.start:
                    best = Region(start, idx)
        _ = stripped
    if best is not None:
        return best
    return window_region(lines, target)


def _is_inline_julia_block(line: str) -> bool:
    """Heuristic: a one-line block like ``a = if c x else y end`` closes itself."""
    import re
    return bool(re.search(r"\bend\b", line)) and line.strip().endswith("end")


# --------------------------------------------------------------------------
# Generic fallbacks.
# --------------------------------------------------------------------------

def indent_block_region(lines: list[str], target: int) -> Region:
    """Block defined by indentation: walk up to a header at lesser indent,
    down until the indentation drops back to the header level."""
    def indent(s: str) -> int:
        return len(s) - len(s.lstrip())

    if not lines[target].strip():
        return window_region(lines, target)
    base = indent(lines[target])
    start = target
    for i in range(target - 1, -1, -1):
        if not lines[i].strip():
            continue
        if indent(lines[i]) < base:
            start = i
            base = indent(lines[i])
            break
        start = i
    end = target
    for i in range(target + 1, len(lines)):
        if not lines[i].strip():
            end = i
            continue
        if indent(lines[i]) <= base and i > start:
            break
        end = i
    return Region(start, end)


def window_region(lines: list[str], target: int, radius: int = 6) -> Region:
    return Region(max(0, target - radius), min(len(lines) - 1, target + radius))


def region_for(lang: str, lines: list[str], target: int) -> Region:
    if lang == "clojure":
        return clojure_toplevel_region(lines, target)
    if lang == "julia":
        return julia_block_region(lines, target)
    if lang == "python":
        return indent_block_region(lines, target)
    return window_region(lines, target)
