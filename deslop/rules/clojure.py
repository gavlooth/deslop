"""Clojure-specific bloat heuristics.

Line-oriented regexes catch the common reimplemented-stdlib idioms; a small
paren scanner catches single-use ``let`` bindings (a hallmark of LLM-generated
intermediate variables). Fixable findings carry an exact within-line
substitution so ``deslop fix`` can apply them offline.
"""

from __future__ import annotations

import re

from ..findings import Finding
from ..regions import clojure_balance

# (compiled regex, fix builder|None, rule, severity, message, suggestion)
# fix builder: match -> (old_substr, new_substr) for a safe within-line rewrite.

_SIMPLE_RULES = []


def _rule(pattern, rule, severity, message, suggestion, fix=None):
    _SIMPLE_RULES.append((re.compile(pattern), fix, rule, severity, message, suggestion))


_rule(
    r"\(not\s+\(=\s",
    "reimpl-not=", "minor",
    "(not (= ...)) reimplements not=",
    "use (not= ...)",
    fix=lambda m: ("(not (= ", "(not= "),
)
_rule(
    r"\(not\s+\(nil\?\s",
    "reimpl-some?", "minor",
    "(not (nil? x)) reimplements some?",
    "use (some? x)",
    fix=lambda m: ("(not (nil? ", "(some? "),
)
_rule(
    r"\(=\s+\(count\s+([^()]+?)\)\s+0\)",
    "reimpl-empty?", "minor",
    "(= (count x) 0) reimplements empty?",
    "use (empty? x)",
    fix=lambda m: (m.group(0), f"(empty? {m.group(1).strip()})"),
)
_rule(
    r"\(>\s+\(count\s+([^()]+?)\)\s+0\)",
    "reimpl-seq", "minor",
    "(> (count x) 0) reimplements seq",
    "use (seq x) or (not (empty? x))",
    fix=lambda m: (m.group(0), f"(seq {m.group(1).strip()})"),
)
_rule(
    r"\(if\s+([^()]+?)\s+true\s+false\)",
    "reimpl-boolean", "minor",
    "(if x true false) is just a boolean coercion",
    "use (boolean x) or x directly",
    fix=lambda m: (m.group(0), f"(boolean {m.group(1).strip()})"),
)
_rule(
    r"\(reduce\s+conj\s+\[\]\s",
    "reimpl-vec", "minor",
    "(reduce conj [] coll) reimplements vec/into",
    "use (vec coll) or (into [] coll)",
    fix=None,  # second arg spans parens; leave the rewrite to slim/human
)
_rule(
    r"\(when\s+[^\n]*?\(do\b",
    "redundant-do", "minor",
    "(when ... (do ...)) — when already wraps an implicit do",
    "drop the inner (do ...)",
    fix=None,
)
_rule(
    r"\(when-not\s+[^\n]*?\(do\b",
    "redundant-do", "minor",
    "(when-not ... (do ...)) — when-not already wraps an implicit do",
    "drop the inner (do ...)",
    fix=None,
)


def check(path: str, lines: list[str]) -> list[Finding]:
    out: list[Finding] = []
    in_string = False
    depth = 0
    for i, line in enumerate(lines):
        # Skip lines that begin inside a multi-line string.
        if not in_string:
            code = _strip_line_comment(line)
            for rx, fix, rule, sev, msg, sug in _SIMPLE_RULES:
                m = rx.search(code)
                if not m:
                    continue
                f_old = f_new = None
                if fix:
                    f_old, f_new = fix(m)
                    if f_old not in line:  # be safe: only attach exact fixes
                        f_old = f_new = None
                out.append(Finding(
                    path=path, line=i + 1, rule=rule, severity=sev,
                    message=msg, suggestion=sug,
                    fix_old=f_old, fix_new=f_new,
                ))
        depth, in_string = clojure_balance(line, depth, in_string)
    out += _single_use_let(path, lines)
    out += _deep_nesting_calls(path, lines)
    return out


def _strip_line_comment(line: str) -> str:
    """Drop a trailing ``;`` comment, ignoring ``;`` inside strings/char lits."""
    i, n, in_str = 0, len(line), False
    while i < n:
        ch = line[i]
        if in_str:
            if ch == "\\":
                i += 2
                continue
            if ch == '"':
                in_str = False
        elif ch == '"':
            in_str = True
        elif ch == "\\":
            i += 2
            continue
        elif ch == ";":
            return line[:i]
        i += 1
    return line


_LET_RE = re.compile(r"\(let\s*\[")


def _single_use_let(path: str, lines: list[str]) -> list[Finding]:
    """Flag (let [x expr] body) where x is referenced exactly once in body."""
    text = "\n".join(lines)
    out: list[Finding] = []
    line_starts = _line_starts(lines)
    for m in _LET_RE.finditer(text):
        let_open = m.start()
        vec_open = text.index("[", m.start())
        vec_close = _match_delim(text, vec_open)
        if vec_close is None:
            continue
        bindings = text[vec_open + 1 : vec_close]
        forms = _top_forms(bindings)
        if len(forms) != 2:  # exactly one symbol + one value
            continue
        sym = forms[0].strip()
        if not re.fullmatch(r"[A-Za-z_][\w\-?!*+./<>=]*", sym):
            continue
        let_close = _match_delim(text, let_open)
        if let_close is None:
            continue
        body = text[vec_close + 1 : let_close]
        uses = len(re.findall(r"(?<![\w\-?!*+./<>=])" + re.escape(sym) +
                              r"(?![\w\-?!*+./<>=])", body))
        if uses == 1:
            ln = _offset_to_line(line_starts, let_open)
            out.append(Finding(
                path=path, line=ln, rule="single-use-let", severity="minor",
                message=f"let binding `{sym}` is used only once",
                suggestion="inline the expression and drop the let",
            ))
    return out


_NESTED_CALL = re.compile(r"\(([\w\-?!*+./<>=]+)\s+\(([\w\-?!*+./<>=]+)\s+\("
                          r"([\w\-?!*+./<>=]+)\s+[^()]+\)\)\)")


def _deep_nesting_calls(path: str, lines: list[str]) -> list[Finding]:
    out: list[Finding] = []
    for i, line in enumerate(lines):
        if _NESTED_CALL.search(_strip_line_comment(line)):
            out.append(Finding(
                path=path, line=i + 1, rule="threading-opportunity",
                severity="info",
                message="3+ levels of nested single-arg calls",
                suggestion="consider a -> or ->> threading macro",
            ))
    return out


# --- tiny paren utilities (string/comment aware) -------------------------

def _match_delim(text: str, open_idx: int) -> int | None:
    pairs = {"(": ")", "[": "]", "{": "}"}
    close = pairs[text[open_idx]]
    opener = text[open_idx]
    depth, i, n, in_str = 0, open_idx, len(text), False
    while i < n:
        ch = text[i]
        if in_str:
            if ch == "\\":
                i += 2
                continue
            if ch == '"':
                in_str = False
        elif ch == '"':
            in_str = True
        elif ch == "\\":
            i += 2
            continue
        elif ch == ";":
            j = text.find("\n", i)
            if j == -1:
                break
            i = j
            continue
        elif ch == opener:
            depth += 1
        elif ch == close:
            depth -= 1
            if depth == 0:
                return i
        i += 1
    return None


def _top_forms(s: str) -> list[str]:
    """Split a binding body into top-level forms (symbols / s-exprs)."""
    forms: list[str] = []
    i, n, depth, start, in_str = 0, len(s), 0, None, False
    while i < n:
        ch = s[i]
        if in_str:
            if ch == "\\":
                i += 2
                continue
            if ch == '"':
                in_str = False
            i += 1
            continue
        if ch == '"':
            in_str = True
            if start is None:
                start = i
            i += 1
            continue
        if ch in "([{":
            if depth == 0 and start is None:
                start = i
            depth += 1
        elif ch in ")]}":
            depth -= 1
            if depth == 0:
                forms.append(s[start : i + 1])
                start = None
        elif ch.isspace():
            if depth == 0 and start is not None:
                forms.append(s[start:i])
                start = None
        else:
            if start is None:
                start = i
        i += 1
    if start is not None:
        forms.append(s[start:])
    return [f for f in forms if f.strip()]


def _line_starts(lines: list[str]) -> list[int]:
    starts, total = [], 0
    for l in lines:
        starts.append(total)
        total += len(l) + 1  # +1 for the join newline
    return starts


def _offset_to_line(starts: list[int], offset: int) -> int:
    import bisect
    return bisect.bisect_right(starts, offset)
