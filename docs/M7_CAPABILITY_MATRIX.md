# M7 verification capability matrix

| Capability | Required authority | Complete path | Incomplete/conflict path |
|---|---|---|---|
| Impact selection | Work-order resources plus complete impact coverage | Dependency-closed matching checks | Project-wide build/lint/type/test fallback |
| Adapter precondition | Current adapter artifact on exact snapshot | May contribute Proven/Disproven | Missing/stale is Unknown |
| Compiler precondition | Current compiler artifact on exact snapshot | May contribute Proven/Disproven | Disagreement is Conflict; no rank winner |
| Language-server precondition | Current LSP artifact on exact snapshot | May contribute Proven/Disproven | Disagreement is Conflict; no rank winner |
| Parse/format/build/lint/type | Selected check and policy-bound artifact | Passing typed evidence | Failed/unknown/missing blocks |
| Targeted tests | Complete dependency selection or conservative fallback | Passing typed evidence | Missing coverage widens selection |
| Coverage | Provider/command artifact | Retained as typed evidence | Unknown remains explicit |
| Characterization | Captured and approved on pinned pre-change snapshot | Matching behavior artifact may support review | Post-authorship/stale/missing rejects risky change |
| Differential check | Selected command/oracle artifact | Passing typed evidence | Mismatch is counterexample/demotion |
| Mutation check | Selected command/provider artifact | Passing typed evidence | Survivor/unknown remains explicit or blocks when selected |
| Graph delta | Recipe expected delta plus authoritative reanalysis | Exact patched, formatted, and live comparison | Mismatch rejects, demotes, and rolls back if live |
| Command execution | Valid resource/filesystem/environment/network policy | Namespace sandbox, cleared env, bounded time/output/files | Sandbox/policy unavailable fails closed |
| Commit | Exact current bytes and Automatic evidence decision | Fsynced journal + replacement + live reanalysis | Error rolls back; crash journal is recovered |
| Undo | Committed manifest and unchanged replacement digest | Exact original bytes restored | Drift/corrupt artifact rejects |
| Recipe eligibility | No active demotion record | May proceed through verification | Active counterexample blocks until explicit supersession |

Safety boundary:

- `SafeAuto`: Automatic only when the verifier plan is Ready and every selected check passes.
- `AnalyzerConfirmed`, `SafeWithPrecondition`, `RiskySuggest`, and `LlmOnly`: review-only at best, with explicit
  residual uncertainty; risky classes additionally require approved pre-change characterization.
- `NeverAuto`: rejected at the verification boundary.
