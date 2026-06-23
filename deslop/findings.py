"""Core data types shared across the scanner, rules and fixers."""

from __future__ import annotations

from dataclasses import dataclass, field

# Lower number == less serious. Used for filtering and ordering.
SEVERITY_ORDER = {"info": 0, "minor": 1, "major": 2}


@dataclass(frozen=True)
class Finding:
    """A single piece of suspected LLM bloat located in a file.

    Line numbers are 1-based and inclusive. ``end_line`` defaults to ``line``
    for single-line findings.

    A finding is *deterministically fixable* when it carries a ``fix_old`` /
    ``fix_new`` pair: an exact substring on ``line`` that ``deslop fix`` can
    rewrite without an LLM. Findings without a fix are report-only (they are
    candidates for ``deslop slim``, which uses a model).
    """

    path: str
    line: int
    rule: str
    severity: str
    message: str
    suggestion: str = ""
    end_line: int | None = None
    fix_old: str | None = None
    fix_new: str | None = None

    @property
    def last_line(self) -> int:
        return self.end_line if self.end_line is not None else self.line

    @property
    def fixable(self) -> bool:
        return self.fix_old is not None and self.fix_new is not None

    @property
    def loc(self) -> str:
        if self.end_line and self.end_line != self.line:
            return f"{self.path}:{self.line}-{self.end_line}"
        return f"{self.path}:{self.line}"

    def sort_key(self) -> tuple:
        return (self.path, self.line, -SEVERITY_ORDER.get(self.severity, 0), self.rule)


@dataclass
class FileReport:
    """All findings for one file plus the source it was derived from."""

    path: str
    lang: str
    lines: list[str]
    findings: list[Finding] = field(default_factory=list)
