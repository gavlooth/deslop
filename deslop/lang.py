"""Language detection and per-language metadata."""

from __future__ import annotations

import os
from dataclasses import dataclass


@dataclass(frozen=True)
class LangSpec:
    name: str                       # "clojure" | "julia" | "python" | "generic"
    line_comment: tuple[str, ...]   # ordered comment tokens, longest first


# Extension -> LangSpec. Anything unknown falls back to GENERIC.
CLOJURE = LangSpec("clojure", (";",))
JULIA = LangSpec("julia", ("#",))
PYTHON = LangSpec("python", ("#",))
GENERIC_HASH = LangSpec("generic", ("#",))
GENERIC_SLASH = LangSpec("generic", ("//",))
GENERIC = LangSpec("generic", ("//", "#"))

_BY_EXT: dict[str, LangSpec] = {
    ".clj": CLOJURE, ".cljs": CLOJURE, ".cljc": CLOJURE, ".edn": CLOJURE,
    ".jl": JULIA,
    ".py": PYTHON, ".pyi": PYTHON,
    ".js": GENERIC_SLASH, ".jsx": GENERIC_SLASH, ".ts": GENERIC_SLASH,
    ".tsx": GENERIC_SLASH, ".java": GENERIC_SLASH, ".c": GENERIC_SLASH,
    ".h": GENERIC_SLASH, ".cpp": GENERIC_SLASH, ".cc": GENERIC_SLASH,
    ".go": GENERIC_SLASH, ".rs": GENERIC_SLASH, ".kt": GENERIC_SLASH,
    ".swift": GENERIC_SLASH, ".scala": GENERIC_SLASH,
    ".rb": GENERIC_HASH, ".sh": GENERIC_HASH, ".bash": GENERIC_HASH,
    ".zsh": GENERIC_HASH, ".yaml": GENERIC_HASH, ".yml": GENERIC_HASH,
    ".toml": GENERIC_HASH, ".r": GENERIC_HASH,
}

# Extensions we are willing to scan when a directory is given.
DEFAULT_EXTENSIONS = tuple(_BY_EXT.keys())


def spec_for(path: str) -> LangSpec:
    return _BY_EXT.get(os.path.splitext(path)[1].lower(), GENERIC)
