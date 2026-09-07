# Research limitations (P0)

Status: P0 in progress. Updated at releases per `docs/RESEARCH_PLAN.md` P0.
Companion to `docs/RESEARCH.md` (bibliography) and `docs/RESEARCH_PROTOCOL.md`
(study questions). M8 frozen numbers are unchanged: challenger accuracy 0.5700
(95% 0.5134–0.6248), ECE 0.0764, disposition `evidence_only`
(`docs/M8_MODEL_CARD.md`).

## 1. What deslop does not claim

- No test, coverage report, or mutation score proves arbitrary behavioral
  equivalence. Passing checks is evidence about the checks run; see
  `mathai-2026-trim` (§III): the minimal behavior-preserving patch is an
  ideal, and practice approximates it via the available environment and test
  suite. Stronger proof language requires a justified semantic argument plus
  satisfied preconditions.
- No smell establishes removability. A finding is a cleanup hypothesis; the
  `verify`/`apply` gate decides on the pinned snapshot.
- No authorship attribution. Findings carry no provenance claim: clean code is
  not declared clean "regardless of author" as a precision statement, and no
  detector identifies who or what wrote code. Review applies without regard
  to provenance.
- No readability label. M8 failed its frozen ship bar (lower bound < 0.60,
  ECE > 0.05, language-holdout failures); all eight axes stay visible as
  evidence only.
- No 1-MLOC performance result, no signing trust root, no human-preference
  validation (M10 terminal downgrades stand).

## 2. Generalization limits

- Smell-taxonomy evidence (`paul-2025-smells`, `liu-2023-refining`,
  `zhang-2024-copilot`) covers Java/Python and the studied models (Gemini Pro,
  ChatGPT/Codex/Falcon, ChatGPT GPT-3.5, Copilot). Do not generalize to other
  languages, current agents, or trajectory settings.
- `orlanski-2026-slopcodebench` figures are version-sensitive: cite only the
  v1 sections actually read (see `docs/RESEARCH.md` §1 for reviewed sections).
  deslop's erosion/structural mass is a modified method, not a replication.
- `velasco-2024-smells` PSC is a next-token propensity probe on two 7B models,
  not a detector-precision result.
- Readability evidence that has been read in full text
  (`posnett-2011-readability`, `scalabrino-2018-readability`) is
  Java/short-snippet oriented; `buse-2010-readability` remains unread (see
  `docs/RESEARCH.md` §1/§5). Entropy work (`torres-2025-entropy`) targets
  change anomaly, not readability; comprehension data
  (`bergum-2024-comprehension`) is Java atoms-of-confusion
  timing/correctness, distinct from preference.
- Source access changes over time. The exact per-source access status (which
  sections were read, what remains metadata-only, and next attempts) lives
  ONLY in `docs/RESEARCH.md` §1 and §5; this file does not duplicate that
  list. P0 source review is NOT complete.

## 3. Unavailable evidence

- Dataset licenses for all six arXiv papers (paper CC licenses cover papers,
  not data); no benchmark dataset has been imported under P0.
- Independent annotators, confirmatory sample sizes, frozen P1 protocol
  (see `docs/RESEARCH_PROTOCOL.md` — study questions only, NOT frozen).
- No new pilot validation exists yet for any language, including Rust/Python:
  the planned P1 first slice has not started, so nothing in this ledger
  implies existing Rust/Python validation (plan P1).

## 4. Known counterexamples

Intentional duplication, public wrappers, compatibility layers, domain
constants, explanatory comments, generated files, test fixtures, reflection,
macros, and required defensive checks can all trigger smell-shaped findings
while being correct. Specific traps already verified: Paul §3.2.1
documentation smell (insufficient comments) does not justify deleting
narration comments; Copilot-Chat fixes can introduce new smells
(`zhang-2024-copilot` §5); trajectory minimization must not shrink tests or
protected contracts (`mathai-2026-trim` scope is the agent's patch, not the
test suite).
