# 01 — rustyharness design v0.1

**Status:** DRAFT v0.1, 2026-09-23 — unreviewed; next: independent adversarial review.

Supersedes nothing yet. It answers the questions the overview framed
([00-overview.md](00-overview.md)) and most of [OPEN-QUESTIONS.md](OPEN-QUESTIONS.md). Where a
question belongs to the owner, this document states it once (§11) with a fail-closed default
that holds until the owner answers.

**Inputs.** R1 prior art, R2 suite inventory and R3 fleet lessons (all in `docs/research/`); R4
(the review of the earlier agent-harness draft), R5 (the integration map) and R6 (the threat
model). R4-R6 live in the rustysuite repository, because they are suite-specific
(`docs/research/README.md`). This document keeps only the general mechanisms from them. The
scaffold review's design notes F4, F5, F10, F11 and F13 are carried forward as requirements
(OPEN-QUESTIONS item 17).

**Conventions.**
- Research is cited by document and section: "R1 §2", "R3 H-07", "R6 TH-7".
- Code is cited as `file:line` in this repository, at commit `54530c6`.
- "UNVERIFIED" marks a claim this document has not checked against a primary source or a build.
- Any sketch in `rust` fences is a signature-level design, not code.
- The one outcome type is always written `GateOutcome { Passed(Witness), Failed, Indeterminate { why } }`. That is the suite's UNIFIED gate-outcome design, whose v0.3 content passed its confirming review as SOUND (R2 §4). **This design defines no other verdict or outcome type (§1.4).**

**Hard constraints (owner decisions, not reopened):** standalone first (ADR-0002); public,
source-available (ADR-0003); fail-closed (no sandbox, no execution); tool and model output is
untrusted; irreversible effects need a human yes every time; "done" comes from evidence; the agent
never modifies its own trust base; no lethal trifecta; one outcome type; Rust only, no new C;
portable to macOS, Linux and Windows; local models first.

---

## 0. Decisions at a glance

| # | Question | Decision | Where |
|---|---|---|---|
| D1 | Trust domains | Two domains. **Reasoning** (context, model call, parse) has no side effects. **Action** means every effect is a harness-executed, policy-checked, confined tool call. | §1.1 |
| D2 | Crate map | 13 crates (§1.2). Two scaffold crates are split (tools becomes manifest + tools; sandbox gains a Windows unsafe-isolation crate). New: `gate-outcome` (standalone), `harness-policy`, `harness-mcp`, `harness-run`, `harness-conformance`. | §1.2 |
| D3 | Outcome type | A standalone, std-only `gate-outcome` crate carries UNIFIED verbatim. The harness and the suite both depend on it; neither defines another. | §1.4 |
| D4 | Context | Rebuilt every turn from run state, with a stable prefix. Older observations collapse to pointers (path, digest, size), never to model-written summaries. | §2.3 |
| D5 | "Done" | The agent's `task.submit` is a request for verification. The run's result is `verdict()` over harness-run check reports plus the reviewer report. A task with no checks can never be `Passed`. | §2.5, §7.3 |
| D6 | Model layer | One async `ModelBackend` trait. Our own thin OpenAI-compatible client: plain HTTP to loopback by default, TLS only behind cargo feature `hosted`. Native tool calls **and** a text protocol with a grammar-constrained action block, chosen per model profile. | §3 |
| D7 | Wire protocol | MCP on the wire (rmcp, stdio child process). A **capability manifest v1** is the trust root: effect plus five closed-set dimensions, pinned description and schema hashes, optional ed25519 signature. Server self-description is ignored for policy. | §4 |
| D8 | Modularity | A new provider ships a manifest plus an MCP server (or an in-process adapter crate) and is admitted by the user. The core learns nothing app-specific. This is proven by a fixture-provider test with zero core diff. | §4.10 |
| D9 | Edits | Exact search/replace (unique match) or whole-file write. Applied in-process, atomically, with a stale-read check and post-apply hash verification. Never `git apply`. | §4.9 |
| D10 | Policy | deny → ask → allow, first match wins, deny cannot be overridden. The effective class is the **max** of the manifest, derived rules and user policy. Approval tokens are bound to run, step, capability, argument digest and expiry, and are single-use. | §5 |
| D11 | Trifecta | Computed at session start from capability labels. Private + untrusted + egress is refused, and v0.1 has no override. | §5.4 |
| D12 | Confinement | `Available` requires a `Conformed` evidence token. It is minted only by a backend that passed the hostile-task suite in CI **and** a live self-probe on this host. Linux: a self-re-exec helper using namespaces + Landlock + seccomp. macOS: deny-default Seatbelt. Windows: AppContainer + Job Object. Named spikes cover the rest. | §6 |
| D13 | Network | None by default. When granted, egress goes through an allowlist proxy **outside** the sandbox. | §6.5 |
| D14 | Journal | Per-attempt, hash-chained JSONL, append-only at the type level, with untrusted payloads in a typed home. Replay re-feeds recorded model outputs. | §7.1, §2.9 |
| D15 | Two pairs of eyes | A built-in reviewer run: fresh context, read-only tools, distinct identity. Its verdict is extracted from a schema-checked artifact, and it can only add blocking findings. | §7.5 |
| D16 | Suite integration | Add-ons are providers (runtime-loaded manifests plus MCP servers) and a few cargo features (`addon-*`), all off by default. A standalone build has zero suite dependencies. | §8 |

## 1. Architecture

### 1.1 Two trust domains

The domain split comes from the earlier agent-harness draft, which review R4 §5 carries forward ("the part worth ratifying"). The benchmark splits generation from grading the same way (R2 §5).

- **Reasoning domain** assembles context, calls the model and parses the reply into a *proposal*. It has no side effects: it cannot touch files, processes, network or secrets. Everything it receives from outside is `Untrusted<T>` (`crates/harness-core/src/lib.rs:31-34`), and every model reply is `Untrusted` from `Source::Model` (`crates/harness-core/src/lib.rs:39-46`).
- **Action domain** turns a proposal into an effect through a single path: schema validation →
  policy decision → approval when required → confined execution by a `ToolProvider`. No action-domain path accepts a string that is executed as code by the harness. `exec.run` takes an argv vector and runs it only inside a conformed sandbox (§4.8, INV-13).

The only channel from reasoning to action is the parsed action of the **model's own reply**. Text that reached the context from tools, files or the task is never eligible for parsing as an action (R4 §3, the injection invariant; INV-29).

### 1.2 Crate map

| Crate | Change vs scaffold | Pure? | Depends on | Contents |
|---|---|---|---|---|
| `gate-outcome` | **new, standalone repo** (§1.4) | yes, std-only | nothing | UNIFIED `GateOutcome`, `IndeterminateKind`, `Witness`, `GateReport`, `Finding`, `Severity`, `Coverage`, `Scope`, `verdict()`, `run_checked`, child-protocol interpretation |
| `harness-core` | keep, extend | yes | `gate-outcome` | `Untrusted<T>`, `Source`, `Meter` (multi-dimension budgets, grown from `Budget`), `RunId`/`Attempt`, the turn state machine as a pure transition function, loop detection, `StopCause` |
| `harness-manifest` | **split** from `harness-tools` (schema half) | yes | `harness-core` | Manifest v1 types, a duplicate-key-refusing parser, validation, hash pinning, signature verification (keys passed in) |
| `harness-policy` | **new** | yes | `harness-core`, `harness-manifest` | Effective-class computation (max-rule), the decision function, trifecta computation, approval-token verification, the `Authorized<Call>` minting point |
| `harness-model` | keep, change `Message` (F4) | no | `harness-core` | `ModelBackend`, message types, profiles, text-protocol parser, OpenAI-compatible client (feature `hosted` adds TLS), replay backend |
| `harness-tools` | **split** (provider half) | no | `harness-core`, `harness-manifest`, `harness-policy`, `harness-sandbox` | `ToolProvider` trait, built-in tools (§4.8), edit engine (§4.9) |
| `harness-mcp` | **new** | no | `harness-tools`, `rmcp` | MCP stdio client adapter, connect-time manifest comparator, quarantine state |
| `harness-sandbox` | keep, change `Containment` (F5) | no | `harness-core` | `Backend` trait, `Conformed` token, `ConfinedSpec`, Linux and macOS backends, egress proxy, confined file-op helper |
| `harness-sandbox-windows` | **new** | no | `harness-sandbox`, `windows` | AppContainer + Job Object backend. The **only** crate allowed `unsafe` (§6.7). |
| `harness-conformance` | **new** (test crate, `publish = false`) | n/a | `harness-sandbox` | Hostile-task corpus (§6.6) as data plus a runner, and the committed per-OS pass matrix |
| `harness-journal` | keep, replace `Sink` (F11) | no | `harness-core`, `gate-outcome` | Hash-chained append-only writer, verifying reader, blob store, replay source |
| `harness-run` | **new** | no | all of the above | The driver: session planning, workspace materialisation, loop, verification, reviewer, run report. The embedding API for apps. |
| `harness-cli` | keep, split exit codes (F10) | no | `harness-run` | The `rustyharness` binary |

**Dependency direction.** Edges only point downward, and there are no cycles:

```
gate-outcome
    ^
harness-core  <-- harness-model
    ^
harness-manifest
    ^
harness-policy                       harness-sandbox <-- harness-sandbox-windows
    ^                                     ^
harness-tools ----------------------------+
    ^
harness-mcp       harness-journal
    ^                  ^
    +---- harness-run -+----> (model, sandbox, conformance[dev])
                ^
           harness-cli
```

**What is pure, and how that is enforced.** `gate-outcome`, `harness-core`, `harness-manifest` and `harness-policy` do no I/O, have no async, read no clock (time is passed in as a value) and have no global state. A CI gate (H1) runs `cargo tree -e normal` on these four crates and refuses any dependency outside an allowlist (`serde`, `serde_json`, `thiserror`, a SHA-256 and an ed25519 implementation). A grep gate refuses `std::fs`, `std::net`, `std::process`, `std::env` and `SystemTime::now` in their sources. Purity is what makes policy decisions replayable: replay recomputes every decision and must get the recorded one (§2.9).

**Async.** tokio (current-thread runtime) is used only in the I/O crates, because rmcp requires it (R1 §5). The pure crates stay synchronous.

### 1.3 Changes to the scaffold

| Scaffold item | Change | Why |
|---|---|---|
| `Message.content: String` (`crates/harness-model/src/lib.rs:28-34`) | `Message` becomes an enum: `System(HarnessText)`, `Task(TaskText)`, `Assistant(Untrusted<String>)`, `Observation { call, body: Untrusted<String> }`. `HarnessText` is constructible only from harness templates. Rendering to the wire calls `inspect("prompt-assembly")` at one choke point. | Scaffold review F4: the trust mark must survive the loop boundary |
| `Containment::Available(Backend)` pub-constructible (`crates/harness-sandbox/src/lib.rs:20-25`) | `Available(Conformed)`. `Conformed` has private fields and is minted only inside `harness-sandbox` by a passing probe (§6.1). `require()` (`:50-55`) returns `Conformed`. Every spawn API takes `&Conformed`. | F5: a lying `Available` must be untypeable |
| `Backend` names (`crates/harness-sandbox/src/lib.rs:29-36`) | `Seatbelt`, `LinuxNs` (namespaces + Landlock + seccomp), `WinAppContainer` | Matches §6.3 |
| `Sink::append` doc-level promise (`crates/harness-journal/src/lib.rs:31-35`); `Event` has no untrusted home (`:11-29`) | `JournalWriter` exposes only `append`. The reader is a separate type. Events carry `UntrustedBlob` (§7.1). | F11 |
| Manifest parsed with serde last-key-wins | Duplicate JSON keys refused by a custom map visitor (§4.3) | F13 |
| Schema v0 `{schema_version, app, app_version, capabilities}` (`crates/harness-tools/src/lib.rs:24`, `:37-49`) | Schema v1 (§4.1). v0 is refused with a migration message; only the example fixture uses v0. | R5 §6.6: `deny_unknown_fields` plus exact versioning means extensions need a version bump |
| CLI exit 2 for both usage and unreadable input (`crates/harness-cli/src/main.rs:33`, `:43`) | Exit codes per §7.7; the last stdout line is the JSON `GateReport` | F10; UNIFIED §6 child protocol |
| `Budget` (steps only, `crates/harness-core/src/lib.rs:80-120`) | `Meter` over six dimensions, each self-measured (the F3 lesson kept) | §2.4 |
| `adapters/<app>/` in this repo | Holds fixtures only. Real providers ship their manifests **with themselves**, so adding an app never touches this repository. | §4.10 |

### 1.4 The outcome crate: shape and boundary

The same rule has been broken three times in the suite: two layers each defined their own outcome type, and the definitions drifted apart. The scaffold therefore deliberately defines no outcome type (`crates/harness-core/src/lib.rs:7-14`). ADR-0002 forbids a dependency on the suite. The resolution is **one crate, one source, two consumers**.

```rust
// crate `gate-outcome` — std-only, zero dependencies (feature `json` adds serde for §6 reports)
pub enum GateOutcome { Passed(Witness), Failed, Indeterminate { why: IndeterminateKind } }
pub enum IndeterminateKind { NothingChecked, UnreadableEvidence, CouldNotRun, UnsupportedOs, StaleBinary }
pub struct Witness { /* private */ checked: usize, digest: Digest }
pub struct GateReport { /* gate id, outcome, findings: Vec<Finding>, coverage: Coverage, scope: Scope */ }
pub fn verdict(reports: &[GateReport]) -> GateOutcome;   // total, worst-wins, verdict(&[]) = Indeterminate{NothingChecked}
pub trait Check { type Input; fn examine(&self, input: &Self::Input) -> Examination; }
pub fn run_checked<C: Check>(check: &C, input: &C::Input) -> GateReport; // the ONLY Witness mint
pub mod child { pub struct ChildRun { /* exit kind, last stdout line, marker seen, timed out */ }
                pub fn interpret(run: &ChildRun) -> GateReport; }            // UNIFIED §6, verbatim
```

**Boundary.** The crate contains the vocabulary, the total `verdict()`, the witness choke point and the interpretation of the child protocol. It contains:
- no I/O (callers spawn children and capture exit and stdout themselves, R3 H-03);
- no policy;
- no harness types;
- no suite types.

`Examination` carries the digests of the items actually examined plus findings. An empty item list yields `Indeterminate { NothingChecked }` (the law that a vacuous check is not a pass). A `Failed` report needs at least one `Blocking` finding, and the constructor enforces this. The crate carries UNIFIED's proof obligations as exhaustive tests over the report-collapse space.

**Location.** The crate lives in its **own repository**, versioned independently (SemVer; any change to the enum is a major version and needs re-review). rustysuite consumes the same crate rather than building one. During H1-H2 it is a workspace member here (`crates/gate-outcome`, extractable, no harness imports). It moves to its own repository before H3 exits (§9). The licence is the owner's call (Q1, §11).

**Harness types that are not verdicts.** `StopCause` (§2.5), `ToolStatus` (§4.5), `PolicyDecision` (§5.1) and `ApprovalState` (§5.3) describe *what happened*. None of them has a success variant for the run. The only way a run is reported as passed is `GateOutcome::Passed`. A CI grep gate refuses `enum` declarations named `*Outcome` or `*Verdict` in `crates/harness-*` (INV-28).

## 2. The run loop

### 2.1 Lifecycle

```
admit task spec -> plan session (grants, trifecta, budgets) -> probe sandbox (if any execute-class grant)
 -> materialise workspace + grading base -> journal header
 -> LOOP { build context -> model call -> parse -> validate -> decide -> (approve) -> execute -> journal -> stop checks }
 -> verify (checks in pristine grading worktree) -> [repair round?] -> review (independent run) -> verdict() -> run report
```

The **task spec** is a user-authored file, trusted as intent, passed by path or stdin, never on
argv (R3 H-08). It declares the task text, workspace source and base revision, capability grants,
budgets, model profile, the verification plan (checks, protected paths, hidden-check directory) and
reviewer configuration. The agent never sees or edits it.

### 2.2 Turn structure

Each turn is one pass of a pure transition `step(state, observation) -> Decision` in `harness-core`, driven by `harness-run`:

1. **Charge the meter** (steps plus the estimated input tokens). If any dimension is exhausted, stop (§2.5).
2. **Build context** (§2.3).
3. **Call the model** (§3) under the remaining wall-clock budget. The call returns a `Completion`, or a typed `ModelError`: `Empty`, `Truncated`, `Unusable`, `Unavailable` or `RateLimited`. Retries use backoff with jitter under a retry budget. An empty or truncated completion is never a turn result (R1 §2, INV-3).
4. **Parse exactly one action.** Native mode reads one tool call. Text mode reads one `<action>` block (§3.3). Zero actions, several actions or malformed JSON count as a **format error**:
   - the model gets one harness-authored repair message naming the parse error;
   - after 3 consecutive format errors the run stops (R1 §1.3, §6).
   - Free text outside the action is journaled as `Untrusted` reasoning and never parsed.
5. **Validate** the tool id against the session's active set, and the arguments against the manifest's input schema (`additionalProperties: false`, exact types).
6. **Decide** with `harness-policy` (§5): allow, ask or deny. Ask pauses the run for approval (§5.3). A denial is returned to the model as an observation and the loop continues.
7. **Execute.** The policy mints an `Authorized<Call>`, which is the only type a `ToolProvider` accepts (§4.5). The call runs through the provider under the conformed sandbox when its class requires one. The result is `ToolResult { status, output: Untrusted<Bytes>, truncated, digest }`. Empty output is rendered as an explicit "(command succeeded, no output)" (R1 §1.2).
8. **Journal** (write-ahead). The intent is appended before execution and the result after it. A crash leaves at most one incomplete step (§2.10).
9. **Stop checks**: submit requested, budgets, loop detection (§2.6).

v0.1 runs one action per turn: no parallel tool calls. llama.cpp disables them by default, and small models are fragile with many tool calls (R1 §6).

### 2.3 Context construction

Context is **rebuilt every turn from run state**, never grown by appending. R4 §5 carries this forward, and R1 §1.7 reports that context quality degrades as it grows. The order is fixed so that the prefix stays stable for the server's prompt cache (R1 §1.7):

| Block | Source | Trust | Bound |
|---|---|---|---|
| 1 System rules + protocol spec | harness templates (`HarnessText`) | trusted | fixed |
| 2 Tool definitions, deterministic order | manifests (§4.6: server description only if its hash matches the pin) | reviewed | ≤ profile `max_active_tools` (default 8) |
| 3 Task | task spec | trusted intent | fixed |
| 4 Harness facts | derived at run start by the harness: base revision, tree digest, file count, each with its producing method (R3 H-19) | trusted | small |
| 5 Agent notes | `notes.write` tool, agent-authored | `Untrusted(Model)` | 2k tokens (capped) |
| 6 Observation index | one line per collapsed observation: step, tool, argument summary, output digest, size, "re-read to view" | harness-rendered pointers | 1 line each |
| 7 Recent turns | last K (assistant action, observation) pairs verbatim, K = profile (default 5, R1 §1.2) | `Untrusted` | per-observation cap (default 100 lines / 16 KiB) with a truncation notice that says how to see more |

**Compaction.** Compaction replaces content with pointers (paths, digests, step numbers), never with copies or model-written summaries. A summary is a model claim, and v0.1 does not let claims replace evidence. If the context still exceeds `profile.context_window × fill_ratio` (default 0.6), K shrinks first and then the per-observation caps. Blocks 1-4 and the newest observation are never dropped. If even that does not fit, the run stops with `StopCause::ContextExhausted`.

**Untrusted rendering.** Untrusted blocks are wrapped in per-turn random-nonce delimiters. Zero-width and bidi control characters are stripped from everything untrusted before rendering (R1 §3.3). This "spotlighting" is a weak layer and is labelled as such. The load-bearing layers are §1.1's single action channel plus policy and confinement.

**Stale reads.** `fs.read` records `(path, sha256)` in run state. An edit whose anchor was read before the file last changed is refused with "file changed since read; re-read first" (R1 §2).

### 2.4 Budgets

| Dimension | Measured by | Default | Exhausted → |
|---|---|---|---|
| Steps | harness counter | 50 | `StopCause::Budget(Steps)` |
| Tokens (in + out) | server `usage`; if absent, a conservative estimate (bytes/3), marked `estimated` in the journal | profile-derived | `Budget(Tokens)` |
| Wall-clock | harness monotonic clock, excluding time spent waiting for human approval | 30 min | `Budget(Wall)` |
| Cost | profile price table × tokens. Local models cost 0. A hosted run **without** a price table refuses to start. | 0 local / user-set hosted | `Budget(Cost)` |
| Consecutive format errors | harness | 3 | `StopCause::FormatErrors` |
| Repair rounds | harness | 1 | no further repair; verdict stands |
| Per-call timeout | harness, kills the whole process tree (§6) | 120 s exec, 30 s others | `ToolStatus::Timeout` (observation, not a stop) |
| Approval wait | harness | 15 min | the request becomes Deny |

Every budget measures its own spend; no caller can assert it (scaffold review F3, `crates/harness-core/src/lib.rs:102-120`). Exhaustion carries `{ dimension, spent, limit }`. Budgets are never extended silently (`crates/harness-core/src/lib.rs:76-78`).

### 2.5 Stop conditions and the result

`StopCause` records **why the loop ended**:
- `Submitted`
- `Budget(dim)`
- `FormatErrors`
- `Loop(kind)`
- `ContextExhausted`
- `PolicyAbort` (a session-level refusal, e.g. a trifecta found mid-run after a quarantine)
- `ModelUnavailable`
- `Cancelled`
- `SandboxLost`

It is not a verdict. The run's result is always a `GateOutcome`:

| Situation | Result |
|---|---|
| `Submitted`, checks run | `verdict(check reports ++ review report)` |
| `Budget(_)` / `Loop(_)` / `FormatErrors` / `ContextExhausted` | Checks **still run** (by default; `verify_on_stop = true`), because evidence decides, not the agent's claim. The result is `verdict(...)`, so a finished-but-unclaimed task can pass and a claimed-but-broken one cannot. |
| `Cancelled`, `SandboxLost`, `PolicyAbort`, `ModelUnavailable` before any check | `Indeterminate { CouldNotRun }` |
| Task spec has no checks | `Indeterminate { NothingChecked }`, always, whatever the agent says (INV-18) |

The **sentinel** is the `task.submit` tool call (the analogue of mini-SWE-agent's `COMPLETE_TASK_AND_SUBMIT`, R1 §1.3). It carries a short deliverable note, which is untrusted. It only moves the run into the verification phase.

### 2.6 Loop detection (pure, in `harness-core`)

| Detector | Rule (defaults) | Action |
|---|---|---|
| Identical action | same `(tool, args digest)` 3 times within the last 6 steps | 1st hit: a harness notice in context. 2nd: `Loop(Repeat)` |
| Edit churn | more than 8 successful edits to one file | `Loop(EditChurn)` (R1 §1.7 "doom loops") |
| No progress | 10 steps with no new observation digest and no workspace tree-digest change | `Loop(NoProgress)` |
| Denial hammering | 3 policy denials of the same capability | `Loop(Denied)`, and the capability is removed from the active set for the rest of the run |

### 2.7 Repair policy

- **Format repair:** as in §2.2 step 4.
- **Verification repair:** if the checks fail and `repair_rounds > 0` (default 1, Aider's two-attempt protocol, R1 §1.4), the model receives a harness message containing the **visible** checks' diagnostics (untrusted), and the loop resumes on the remaining budget.
  Hidden checks contribute only "hidden check `<id>` failed", never content or source (the benchmark's repair mode shows diagnostics, never the oracle, R2 §5). After the last round the verdict stands.
- **Model retries** (transport errors) are separate from repair rounds and have their own budget (§3.2).

### 2.8 Per-run isolated state

```
<state_root>/                      # outside every workspace; 0700; read-denied to sandboxes (§6.4)
  runs/<run-id>/attempt-<n>/
    journal.jsonl                  # §7.1, single writer, exclusive lock
    blobs/<sha256>                 # content-addressed large payloads
    workspace/                     # agent-writable tree (fresh single-commit repo, §7.6)
    grading/                       # pristine grading worktree, rebuilt per verification
    scratch/                       # agent-writable temp
    snapshots/                     # tree-digest snapshots after each write step (path+digest, incl. empty dirs)
  index/                           # derived, rebuildable by scanning runs/; never authoritative
```

- **Run identity.** `run-id` is time-ordered and random (128 bits). `attempt-<n>` is a monotonic counter under that id. All output paths are derived from identity, so a relaunch cannot address an earlier attempt's directories (R3 H-12).
- **No shared database.** Concurrent runs share no writable state (R1 §2, R3 H-09).
- **Concurrency.** A semaphore per model endpoint limits parallel runs, default 1 for loopback endpoints (R3 H-10). Launches beyond the limit queue; they never storm the server.
- **Refusals.** `state_root` on a network filesystem is refused at startup. So is a `state_root` inside a workspace.

### 2.9 Replay

Inference is not deterministic even at temperature 0 (R1 §1.7), so **replay means re-feeding recorded model outputs**. A `ReplayBackend` implements `ModelBackend` from a journal. There are two modes:
- **audit:** tool results are also re-fed. The harness recomputes every context digest and every policy decision. Any divergence from the journal is reported as `Indeterminate { UnreadableEvidence }` with the first divergent step. This works because context building and policy are pure (§1.2).
- **reproduce:** tool calls re-execute in a fresh sandbox and workspace. Output digests are compared, and each difference is a finding. This is a diagnostic, never a verdict on the original run.

### 2.10 Resume

Resume opens a new attempt from the last fully journaled step: the chain is verified (§7.1), the
workspace is restored from that step's snapshot, and a trailing intent with no result is
re-decided, not re-executed blindly (policy re-runs; approval is re-asked where required).

## 3. Model layer

### 3.1 The trait

```rust
pub trait ModelBackend {
    fn identity(&self) -> ModelIdentity;                        // recorded in the journal header
    async fn complete(&self, req: &ModelRequest, deadline: Instant) -> Result<Completion, ModelError>;
}
pub struct Completion { pub content: Untrusted<String>, pub tool_calls: Vec<Untrusted<RawToolCall>>,
                        pub finish: FinishReason /* Stop, ToolCalls; Length/absent => ModelError */,
                        pub usage: Usage /* measured or estimated */ }
```

Implementations: `OpenAiCompatible` (loopback HTTP by default), `Replay` (§2.9), `Scripted` (tests). Suite add-ons may add a mesh transport (§8). Nothing else in the harness knows which backend is in use.

### 3.2 The client

We write our own thin client for `/v1/chat/completions` and `/v1/models`, with SSE streaming. It is an HTTP/1.1 client over tokio, roughly 1-2k lines, with no hyper and no reqwest. Local-first needs no TLS, and every popular Rust LLM client pulls in C through TLS (R1 §5).

**Endpoint rules:**
- The default build connects only to loopback (`127.0.0.1`, `::1`, `localhost`) or to a unix-socket endpoint.
- Plain HTTP to any other address is **refused**, because prompts would cross the LAN in cleartext.
- Non-loopback endpoints need the `hosted` cargo feature (TLS: this is where rustls and its C provider enter, the "C tax", recorded in `deny.toml` review) plus per-run opt-in config, or a suite transport add-on.
- The hosted API key is held by the harness process, used only in the request header, and journaled as a handle name only (§5.5).

**Failure handling.** `finish_reason` is checked every time; `length`, an absent reason or an
empty body is a typed error, never an empty success (R1 §2, principle #12). 429 and 5xx retry with
exponential backoff and jitter under a retry budget (default 3 per turn), each attempt recorded
(R3 H-10). The fallback chain is configurable but **never crosses privacy classes**: a loopback
profile never falls back to a hosted one.

**Startup checks** fail closed with a plain-language fix (R1 §6): `/v1/models` must answer and
list the model; with `protocol = native`, a one-call tool-use smoke test runs, and on failure the
harness switches to the text protocol and journals it (a format choice, not a security one); for
llama.cpp, template and `--jinja` status come from the server's props endpoint (UNVERIFIED; S-P1).

### 3.3 Two action protocols

- **Native:** the OpenAI `tools` parameter carries the active tools' JSON Schemas, with `tool_choice: "required"` where the profile says it works (llama.cpp issue noted in R1 §6).
- **Text:** tool definitions are rendered into the system block. The model reasons in free text and emits exactly one block:

  ```
  <action>{"tool":"harness.fs.read","args":{"path":"src/lib.rs","start":1,"lines":100}}</action>
  ```

  Where the server supports it, **only the action block** is grammar-constrained, via a lazy grammar triggered by `<action>` and generated from the active tools' schemas. Reasoning stays unconstrained, because constraining it costs accuracy (R1 §6, "Let Me Speak Freely").
  Lazy-grammar availability through the OpenAI-compatible API is **UNVERIFIED** (spike S-P1). A server that cannot constrain is parsed without a grammar: safety never depends on the grammar, because validation is harness-side (§2.2 step 5); the grammar only buys reliability.

Action schemas are kept shallow: no `oneOf`, no recursion, and only the keywords `type`, `properties`, `required`, `enum`, `maxLength`, `minimum`/`maximum`, `items` and `additionalProperties: false`. Grammar engines cover JSON Schema unevenly (R1 §6, JSONSchemaBench). A manifest schema that uses other keywords is refused (§4.3).

### 3.4 Per-model profiles

A profile is a data file: `context_window`, `fill_ratio`, `protocol` (native or text),
`tool_choice_required_ok`, `grammar` (none, gbnf_lazy or json_schema), `max_active_tools` (5-8;
default 6 for models under 30B), `edit_format` (replace or whole), `recent_turns` K, sampling
defaults, a KV-quantization safety note, and a price table (hosted only).

**Rules:**
- An unknown model gets a conservative default profile: text protocol, 5 tools, replace edits, K = 4.
- `rustyharness profile check` runs a fixed smoke eval (tool-call validity, edit-format compliance, format-error rate) and stamps the profile with its result digest.
- An unvalidated profile runs, with `profile_validated: false` recorded in every journal header.
- rustybenchmark may generate profiles (R1 §9), but that is an input, not a dependency.

**Active tools.** The session's active set is the granted capabilities, capped at `max_active_tools`. If more are granted, the task spec must pick. v0.1 has no dynamic tool retrieval (§11 non-goals). R1 §1.7 reports that fewer tools help small models.

### 3.5 Model identity

`ModelIdentity` is written into the journal header and into every run report:
- endpoint class (loopback, LAN add-on, hosted);
- profile id and profile SHA-256;
- the user-declared weights digest, if given;
- sampling parameters.

The **server-claimed** fields are stored as `Untrusted` and labelled "claimed": model id, server software and version, and chat-template hash. A server can lie about what it serves, so identity is recorded as a claim plus what the harness controls. That is what a benchmark row key needs (R2 §5, rustybenchmark agentic board).

## 4. Tools and modularity

The owner's key question is: how do future apps slot in without the harness being redesigned? The answer:
- The core reasons only over **closed-set capability dimensions**, never over app names (`crates/harness-tools/src/lib.rs:63-76` already orders effect classes).
- Every provider declares its capabilities in a manifest that the harness verifies and pins.
- The wire is MCP, so any standard MCP server can be a provider.
- The manifest, not the server, is what policy trusts (R1 §4.4, R6 TH-3).

### 4.1 Capability manifest v1

The manifest is JSON (the scaffold's format). A detached `manifest.json.sig` is optional (§4.4).

| Field | Type / closed set | Rule | Justification |
|---|---|---|---|
| `schema_version` | integer, must be in the harness's supported set (v0.1: `{1}`) | mismatch refused, naming both versions | R5 §6.6 |
| `provider` | name `[a-z0-9_-]{1,64}` (the scaffold grammar, `crates/harness-tools/src/lib.rs:120-154`) | the namespace. Reserved names refused (§4.3). Must be admitted (§4.4). | Renamed from `app`: providers are not only apps (ADR-0002) |
| `provider_version` | ≤ 32 bytes | recorded | scaffold |
| `min_harness` | SemVer | a harness older than this refuses | R1 §4.4 |
| `transport` | `builtin`, `mcp-stdio { argv, env_allow }` or `in-process { feature }` | `mcp-stdio` servers are spawned **confined** (§4.6) | §4.5 |
| `mcp_protocols` | list of MCP protocol versions | highest common version chosen; none in common → refused | R1 §4.1 |
| `capabilities[].id` | `<provider>.<verb...>`, scaffold grammar, ≤ 128 | must be under the provider's own namespace | scaffold |
| `capabilities[].mcp_name` | string | the server's tool name mapped to this id (1:1, no fallback) | R5 §6.4: a binding that does not resolve fails the load |
| `capabilities[].summary` | ≤ 512 bytes, printable ASCII + UTF-8 letters, no control, zero-width or bidi characters | shown to **humans** in approvals | scaffold + R1 §3.3 |
| `capabilities[].effect` | `read` < `write` < `execute` < `irreversible` | unchanged ordinal | `crates/harness-tools/src/lib.rs:63-76` |
| `capabilities[].sensitivity` | `public` < `operational` < `personal` < `restricted` | what the result or effect touches | R5 §5.1: a `read` of personal data is not harmless |
| `capabilities[].blast_radius` | `own` < `host` < `shared` | `own` = the provider's own state; `host` = machine-wide; `shared` = state other people or machines rely on (remote repositories, published content, trust shared across machines) | R5 §5.2: a local config write and a trust rewrite must not share one policy |
| `capabilities[].egress` | `none` < `lan` < `internet` | whether invoking it sends data off-host | R5 §5.3: egress must be declared and measurable |
| `capabilities[].content` | `own` or `third_party` | whether results can carry text authored outside the user's control (web, messages, other people's files) | trifecta label (R1 §3.3, §4.4) |
| `capabilities[].confirmation` | `none` < `user_confirm` < `protected_action` | a floor, never a ceiling (max-rule) | R5 §5.4; rustyfin precedent (R2 §5) |
| `capabilities[].input_schema` | JSON Schema, the subset in §3.3, `additionalProperties: false` mandatory | refused otherwise | R2 (a)1: exact-argument validation |
| `capabilities[].schema_sha256` / `description_sha256` | hex | the server-presented schema and description must hash-match at connect time | rug-pull defence (R1 §4.1, §4.4) |
| `capabilities[].secrets` | list of secret **handle names** | resolved at the tool boundary (§5.5) | R1 §3.4 |
| `capabilities[].limits` | optional `{ timeout_ms, max_result_bytes }` | can only **tighten** the harness defaults | R2 §5 (rustyfin per-tool caps); max-rule keeps it safe |

Deliberately **not** fields: a free-form `risk_tier` (it would duplicate the ordered dimensions and invite contradictions, R5 §5.5) and a free-form `subject`/target. The closed `blast_radius` has no "third party" variant, so an action against someone else's systems cannot be declared at all (R5 §5.5, §6.2).

### 4.2 Effective class and the max-rule

`harness-policy` computes each capability's **effective** confirmation as the maximum of three things:

1. **The declared `confirmation`.**
2. **Derived floors:** `irreversible` or `shared` → `protected_action`; `egress = internet` or
   `sensitivity ≥ personal` → `user_confirm`; `execute` → requires `Conformed`.
3. **User policy** from the harness config.

A manifest can only make things **more** restrictive. A lying manifest that understates is bounded by the derived floors of its other fields. A manifest that lies on every field is a review problem, and §4.4's tiers limit what an unreviewed manifest may claim (R6 TH-4).

### 4.3 Validation (content, not presence)

Parsing refuses the following. Each has a test (R3 H-01, H-02):
- duplicate JSON keys, at any depth (F13);
- unknown fields (`deny_unknown_fields`, as today, `crates/harness-tools/src/lib.rs:39`);
- a schema version outside the supported set;
- reserved provider names: `harness` (built-ins) plus a user- or add-on-extensible reserved list, fixed at config load;
- ids outside the provider namespace;
- duplicate ids;
- an empty capability list;
- any `mcp_name` mapped twice;
- input schemas using keywords outside the subset, or missing `additionalProperties: false`;
- descriptions or summaries containing control, zero-width or bidi characters. These are **refused, not stripped**, because stripping would change the bytes that `description_sha256` pins.

### 4.4 Trust tiers, admission, signing, pinning

**Admission.** A provider is usable only if the user's harness config (trust base, §6.4) lists it: namespace plus either a trusted signing key or a pinned manifest SHA-256. Anything not admitted is refused (R5 §6.5). Two admitted providers with the same namespace are refused (the shadowing defence, R1 §4.1).

**Tiers:**

| Tier | How admitted | May declare | Always |
|---|---|---|---|
| `builtin` | ships in the binary (`harness.*`) | anything; reviewed with the harness | subject to the same policy as any provider |
| `signed` | manifest signed (ed25519 over the exact file bytes) by a key in the user's `trusted_keys` | any dimension values | pinned hashes enforced |
| `pinned` | unsigned; the user approved this exact manifest SHA-256 via `rustyharness provider add` | `sensitivity ≤ operational`, `blast_radius ≤ host` | sandboxed, `confirmation ≥ user_confirm`, never in a session with `personal`/`restricted` capabilities (R1 §4.4) |

**Signing.** Signing keys belong to whoever publishes providers: a standalone user, a third party, or rustysuite for its apps. Who holds the suite's key is decided in the suite. The ed25519 implementation must pass the current `deny.toml` licence list unchanged (candidate `ed25519-compact`, MIT, pure Rust: UNVERIFIED). Widening the licence list is an owner-visible change.

**Pinning.** At connect time the harness compares `tools/list` against the manifest:
- a tool not in the manifest → dropped and journaled;
- a manifest tool the server lacks → unavailable;
- `description_sha256` or `schema_sha256` mismatch → the capability is **quarantined**. Invocations refuse until the user re-pins **outside the run** (`rustyharness provider repin`). A quarantine found mid-session re-runs the trifecta check (§5.4) (INV-7).

### 4.5 The adapter trait

```rust
pub trait ToolProvider {
    fn namespace(&self) -> &Namespace;
    async fn presented(&mut self) -> Result<Vec<PresentedTool>, ProviderError>; // what the provider claims (compared, never trusted)
    async fn invoke(&mut self, call: Authorized<Call>, ctx: &InvokeCtx) -> Result<ToolResult, ToolError>;
}
pub struct Authorized<C> { /* private; minted only by harness-policy::decide */ }
pub struct InvokeCtx<'a> { pub conformed: Option<&'a Conformed>, pub deadline: Instant, pub secrets: SecretHandles<'a>, pub step: StepId }
pub enum ToolStatus { Ok, Error { code: u16 }, Timeout, Crashed { signal: Option<i32> }, Refused { reason: RefusalKind } }
```

`Authorized<Call>` cannot be constructed outside `harness-policy`, so no provider can be driven by an unvalidated call. This is the same pattern as the scaffold's `require()` doc: "there is no API that runs a command without a [`Backend`] in hand" (`crates/harness-sandbox/src/lib.rs:48-49`).

**Three implementations:**
1. **builtin:** in-process Rust, `harness.*` (§4.8).
2. **mcp-stdio** (`harness-mcp`, over rmcp 3.4 with the stdio and child-process features, no compiled C per R1 §5). The server binary runs **inside the sandbox** under a `ConfinedSpec` derived from its manifest. Egress is granted only if some capability declares it, and then only through the proxy (§6.5). Its environment is `env_allow` only.
3. **in-process adapter:** a Rust crate behind a cargo feature, for trusted providers that need zero IPC. It still ships a manifest and is still policy-checked. The only thing it skips is process isolation, so it is allowed only for `builtin` or `signed` tiers.

Remote MCP over Streamable HTTP is feature `mcp-http`, off, and not in v0.1 (it needs TLS; §11).

### 4.6 MCP mapping

MCP is the wire, and the manifest is the trust root (R1 §4.4; R6 TH-3):
- The model sees tool **descriptions only when their hash matches the pin**, and **names only as manifest ids** (`provider.verb`). Server tool names are internal.
- MCP annotations such as `readOnlyHint` are ignored for policy. The spec itself calls them untrusted (R1 §4.1).
- MCP features the harness does not use are refused at negotiation and never served: sampling, elicitation to the model, and resources the manifest does not declare. A server that asks the host to run the model (sampling) gets an error. That would be a channel from provider content into model output with no policy.
- Any suite-specific metadata travels in MCP's sanctioned `_meta` namespace, so suite servers remain valid MCP servers for any client (R1 §4.4 item 6).

### 4.7 Versioning and negotiation

- **Manifest schema:** the harness declares a supported set. A manifest names exactly one version. Any change to the manifest schema is a version bump, never a relaxed `deny_unknown_fields` (R5 §9: relaxing turns a new `sensitivity: restricted` into a silent `read`). A newer harness may support `{1, 2}`; an older one refuses v2 with a message naming both versions.
- **MCP protocol:** negotiated per request by rmcp. The harness refuses any version not listed in `mcp_protocols`.
- **Action protocol (§3.3):** versioned in the system block (`protocol: rh-action/1`). A reply in another dialect is a format error.

### 4.8 Built-in tools

The built-in tools are themselves declared by a `harness` manifest. They take the same policy path with no special cases, and they exist only when the task grants a workspace (R4 §4 item 1: grants are per task, deny by default).

| Id | Effect / sensitivity / blast / egress | Behaviour |
|---|---|---|
| `harness.fs.read` | read / operational / own / none | Path inside the workspace plus a line window (default 100 lines, R1 §1.2). Returns content + SHA-256 + total lines. Records the read hash (§2.3). |
| `harness.fs.search` | read / operational / own / none | Regex over the workspace (pure-Rust `regex` + `ignore`, UNVERIFIED C-free until `cargo tree`). At most 50 hits, summarised per file (R1 §1.2). |
| `harness.fs.list` | read / operational / own / none | Directory listing, bounded depth and count |
| `harness.edit.replace` | write / operational / own / none | §4.9 |
| `harness.edit.write` | write / operational / own / none | Whole file. Creating: must not exist. Overwriting: must have been read in this run and be ≤ 400 lines (R1 §1.5). |
| `harness.exec.run` | execute / operational / own / none | argv vector plus working directory inside the workspace. Program resolved against the task's **exec allowlist** (absolute paths pinned, like Codex `host_executable`, R1 §3.1). stdin null. Timeout. Runs only under `Conformed`. |
| `harness.notes.write` | write / public / own / none | Replaces the agent's notes (2k-token cap). Content is `Untrusted(Model)`. |
| `harness.task.submit` | write / public / own / none | The sentinel (§2.5) |

**Exec allowlist.**
- Shells (`sh`, `bash`, `zsh`, `cmd`, `powershell`, `pwsh`) are **not** in any default allowlist. Adding one is explicit user config, and it stamps `shell_enabled: true` in the journal header.
- Even then, the sandbox is the control, not the argv validator: a build script is arbitrary code whatever the argv says (R4 §3).
- The allowlist supports Codex-style prefix rules with `match`/`not_match` examples, checked as unit tests when the config loads (R1 §3.1).

**File-operation confinement:**
- In sessions **with** any execute-class grant, built-in file tools run through the confined file-op helper under the same backend (§6.1). An agent-created symlink cannot make the unconfined harness process read outside the workspace; the kernel enforces the view.
- In sessions **without** execute grants (H1, and Windows before W1), the harness reads in-process. The workspace was materialised by the harness with outward symlinks refused, and nothing in the session can create new ones (INV-30).

### 4.9 Edit format and verification

`harness.edit.replace { path, old, new, count = 1 }` works like this:

1. **Preconditions:**
   - the path is inside the workspace and not protected (§7.6);
   - the file was read in this run, and its current hash equals the last-read hash (else the stale-read refusal, §2.3);
   - `old != new`;
   - `old` is not empty.
2. **Match:** exact bytes. The number of matches must equal `count`.
   - Zero matches → an error with the nearest matching line numbers, plus a line-ending hint when the file uses CRLF and `old` uses LF.
   - More matches than expected → an error listing the match line numbers.
   - Either way nothing changes (R1 §1.5: exact-unique str_replace had the "highest reliability").
3. **Apply in-process:** write to a temporary file in the same directory, fsync, then rename (atomic). Permissions and line endings are preserved.
4. **Verify after applying:** re-read the file. The SHA-256 must equal the in-memory expected splice **and** differ from the pre-edit hash, or the edit is reported as failed. This check exists because of the silent no-op class (`git apply` in a subdirectory exiting 0, R1 §2; R3 C2).
5. **Journal:** before and after digests, plus the tree snapshot (§2.8).
6. **Optional syntax check:** a per-language check from the task's `post_edit` list (e.g. `rustfmt --check <file>`), run in the sandbox. A failure rolls the edit back and is returned as the observation (SWE-agent's lint-guarded edit, +3 pp, R1 §1.2).

There is **no** `git apply`, no unified-diff format and no shell-out for edits in v0.1 (R1 §1.5, §8 #7). The format is chosen per profile between `replace` and `whole`.

### 4.10 How a provider adds itself (no core change)

1. The provider author writes `manifest.json` (§4.1) and an MCP stdio server, or an in-process adapter crate.
2. The user runs `rustyharness provider add <dir>`. The harness validates the manifest (§4.3) and shows a plain-language table of every capability with its effective class (§4.2). Adding the provider requires the user's explicit yes. The harness then records the admission (namespace + key or pin) in the user config, **outside any run** (the trust base, §6.4).
3. Task specs grant capabilities by id. Policy, confinement, the trifecta rule and journaling apply automatically from the dimensions.

**The test that proves it** (INV-31): a fixture provider with verbs and a namespace unknown to the harness codebase integrates end to end in CI (admit → grant → invoke → journal → policy). The CI job asserts that `git diff` over `crates/` is empty.

rustysuite apps are one set of such providers. Their manifests and servers ship with the apps or with the suite add-on, not in this repository.

## 5. Policy and approval

### 5.1 Decision order

`decide(call, effective_class, session, user_policy) -> PolicyDecision { Allow | Ask(Tier) | Deny(Reason) }` is pure, and every decision carries the id of the rule that produced it. The evaluation order is:

1. **Deny rules** (user policy, session trifecta, quarantine, missing `Conformed` for execute): first match wins and **cannot be overridden** by any later rule, flag or approval (R1 §3.4).
2. **Ask rules** (effective confirmation ≥ `user_confirm`).
3. **Allow rules.**
4. **Default: Deny.**

### 5.2 Default policy

| Effective class | Default |
|---|---|
| `read`, sensitivity ≤ operational, content `own` | allow |
| `read`, content `third_party` | allow; the result is `Untrusted` like everything else, and the session is labelled "untrusted" for the trifecta |
| sensitivity `personal` | ask (`user_confirm`), and only if the session was granted personal data; result caps apply |
| sensitivity `restricted` | **deny**. No v0.1 session may hold restricted capabilities (Q3; ADR-0002 item 3). |
| `write`, blast `own`, built-in workspace edits | allow: the sandbox plus snapshots make them undoable, and fewer prompts is safer (R1 §3.4, −84% prompts) |
| `write`, provider-declared | ask (`user_confirm`) unless user policy allows |
| `execute` | allow only with `Conformed`, else deny (INV-6) |
| egress `lan`/`internet` | ask; plus the trifecta check; plus the proxy (§6.5) |
| `irreversible` or blast `shared` | **ask every time** (`protected_action`); never cached, never scoped to the run |
| **no approver present** (non-interactive, CI, embedded without a UI) | every Ask becomes Deny |

These defaults answer OPEN-QUESTIONS item 9 (where the admin assistant ends): irreversible or shared-scope operations are **never** automatic. What is otherwise automatic follows the table.

### 5.3 Approval flow and tokens

When the decision is Ask, `harness-run` sends an `ApprovalRequest` to the configured approver: the CLI prompt, or an embedding UI through the `harness-run` API. The request shows:
- the capability `summary`;
- its effective class **in plain words** (e.g. "will send data to the internet", "cannot be undone");
- the rendered arguments, escaped;
- the step.

The approver answers yes or no. A yes mints:

```rust
struct Approval { run: RunId, attempt: u32, step: StepId, capability: CapId, args_sha256: [u8; 32],
                  tier: Tier, expires_at: Mono, nonce: [u8; 16], approver: PrincipalId, mac: [u8; 32] }
```

The pattern is lifted from rustyfin (clean-room, no code dependency; answers OPEN-QUESTIONS 14): a confirmation token bound to user, session, tool, argument hash and expiry, 15-minute TTL (R2 §5).

**Token rules:**
- The `mac` is an HMAC under a per-run key held only in harness memory, so a token forged on disk or in context is worthless. Tokens never enter model context.
- **Binding:** a different step, capability or argument digest, an expired token, or a reused nonce is refused. Consumed nonces are journaled, so reuse is detectable in replay (INV-16).
- **Tiers:** `protected_action` tokens are single-use. A `user_confirm` approval may be marked by the approver "for identical calls in this run", which covers the same capability with the same argument digest only; the scope is recorded.
- A pending approval that times out is a **Deny** (fail-closed), never an auto-approve.
- The denial reason goes back to the model as an observation.

### 5.4 The trifecta rule

At session start, and again whenever the active set changes (quarantine, capability removal), `harness-policy` computes three labels over the active set **plus the workspace**:
- **P (private):** any capability with sensitivity ≥ `personal`, **or** the workspace. Workspaces are private by default. The user may declare a workspace `public` in the task spec.
- **U (untrusted):** any capability with content `third_party`, **or** the workspace. Workspace content is third-party by default: repositories contain other people's text.
- **E (egress):** any capability with egress ≠ `none`, including MCP servers granted proxy egress.

**P ∧ U ∧ E ⇒ the session is refused**, with a typed error naming one capability per label. v0.1 has no override (INV-9).

Consequences:
- A default coding session has **no egress**. Builds are offline with vendored dependencies (the benchmark's `--offline --locked` precedent, R2 §5).
- Web research is a separate session with no private workspace. Its output is handed to the human, not piped into a private session.
- A plan-then-execute or dual-LLM path, which would allow some combinations (CaMeL, R1 §3.3), is future work (Q7).

**Hosted models** are handled as a disclosure rule, not a trifecta label. The provider is not attacker-readable, but everything in context goes to it. A hosted profile may be used only if the session's maximum sensitivity is ≤ `operational`, unless the user's config opts a specific sensitivity in (Q2). `restricted` never goes to a hosted model.

### 5.5 Secrets

- **Never in context:** no secret value is rendered into a prompt. Capabilities name **handles** (`secrets: ["github_token"]`). The harness resolves them at invoke time and hands them to the provider process only, through its environment or a descriptor, per `transport`. They never reach the model (R1 §3.4, §8 #17).
- **Never in child environments by default:** children get a built environment, not an inherited one. It contains an allowlist (`PATH` = pinned toolchain dirs, `HOME` = scratch, `TMPDIR`, `LANG`, toolchain variables the task declares) plus only the handles their capability declares (R3 H-07).
- **Detective redaction:** tool outputs are scanned for the exact bytes of every secret resolved in this run, plus planted canaries, before they become observations. A hit is replaced and journaled as an event. Redaction is labelled **detective-only** (R3 H-16); the preventive controls are the two rules above.
- **Secret stores:** one `SecretStore` trait. Built-in `FileStore` (mode 0600 in the harness config directory, read-denied to every sandbox, §6.4) is always available; built-in `KeychainStore` (macOS Keychain, Windows Credential Manager, Linux Secret Service) is optional, pending spike S-K1 on C-freedom of the per-OS crates (UNVERIFIED); add-on stores such as a suite secret-custody client (§8) are off by default, and whether the suite ever enables one is decided in the suite.
- **Model API key:** read by the harness process from the store at startup, used only in HTTP headers, and never journaled (a handle name only).

## 6. Confinement

### 6.1 Fail-closed trait with evidence-gated availability

```rust
pub struct Conformed { /* private */ backend: BackendKind, matrix_row: &'static str, probe_digest: Digest }
pub trait Backend {
    fn probe(&self) -> Result<Conformed, Unavailable>;          // the ONLY mint of Conformed
    fn spawn(&self, spec: &ConfinedSpec, ev: &Conformed) -> Result<ConfinedChild, SpawnError>;
}
pub enum Containment { Available(Conformed), Unavailable(Unavailable) }
pub fn require() -> Result<Conformed, Refused>;                  // every exec path goes through here
```

`probe()` mints `Conformed` only if **both** of the following hold (scaffold review F5):
1. **CI conformance:** the committed pass matrix (`harness-conformance`, §6.6) says this backend on this OS family passed the **full** hostile-task suite in CI at this harness version. The matrix is compiled into the binary as a constant.
2. **Live self-probe:** a fast subset of the suite runs on this host now and every probe is **refused**:
   - network connect;
   - write outside the workspace;
   - read of a planted home canary;
   - env canary absent;
   - launching a process outside the sandbox.

   This catches hosts where a kernel feature is off, a sysctl differs, or the OS removed a primitive.

If either fails, the result is `Unavailable` with a typed reason and a plain-language fix. The run then continues read-only, or refuses if the task needs execution. There is no degraded or "run it anyway" tier: a weaker combination is acceptable only if it passes the same full suite, in which case it is simply another backend (R3 H-05, INV-6, INV-15).

`Unavailable` maps to `Indeterminate { UnsupportedOs }` when no backend exists for the platform, and to `Indeterminate { CouldNotRun }` when a backend exists but failed on this host (UNIFIED kinds; R4 §3).

### 6.2 What a confined child gets

`ConfinedSpec` holds: `argv` (a vector, never a shell string); `cwd` inside the workspace; `env`
(built, §5.5); `read_only` roots (toolchains, base tree); `read_write` roots (workspace, scratch);
`protected` read-only overlays inside the workspace (§7.6); `network` (`None` or
`Proxy(allowlist id)`); `limits` (wall, CPU, memory, processes, file size); stdin null.

Everything not granted is denied. That always includes the user's **home directory** (only
explicitly granted subpaths, such as a toolchain directory, are readable), `state_root`, the harness
config and trust base, other runs' directories, and OS credential stores.

### 6.3 Per-OS backends

The design commits to **the conformance suite, not to a primitive**. The primitives below are the chosen first attempt; a spike that fails swaps the primitive, never the bar.

| OS | Backend (decided first attempt) | Rationale | Spike (exit = full §6.6 suite green in CI) |
|---|---|---|---|
| Linux | `LinuxNs`: the harness re-execs itself as a helper (`rustyharness __confine`; spec passed over an inherited pipe, never argv). The helper does:<br>1. `unshare` user, mount, net, pid, ipc, uts namespaces;<br>2. builds a minimal root (tmpfs + read-only binds of system and toolchain paths + read-write binds of workspace and scratch + read-only re-binds of protected paths); home is **not mounted**;<br>3. `pivot_root`;<br>4. Landlock ruleset with the same allowlists (defence in depth);<br>5. rlimits (plus cgroup v2 limits if a delegated subtree exists);<br>6. a seccomp deny set (namespace creation, `mount`, `ptrace`, `bpf`, `keyctl`, `kexec_*`, module loading, `perf_event_open`, `userfaultfd`), applied last;<br>7. exec.<br>The PID namespace gives whole-tree kill. | No bubblewrap binary (external C, R1 §3.2). The empty netns plus an absent home mount cover Codex's objection that Landlock alone cannot isolate unix sockets (R1 §3.1): abstract sockets are per-netns, and path sockets outside the mount view do not exist. Crates: `landlock`, `seccompiler` (pure Rust, R1 §5), `nix` or `rustix`. Whether all of this is reachable without `unsafe` is **UNVERIFIED**. | **S-L1:** helper on stock Ubuntu LTS, Debian, Fedora; also measure `sandlock-core` 0.8.8 as an alternative (R1 §5). Hosts with unprivileged user namespaces disabled → `Unavailable` with a fix message; no fallback. |
| macOS | `Seatbelt`: `/usr/bin/sandbox-exec` (absolute path) with a generated profile file in a harness-private temp dir. The profile is **`(deny default)`** with explicit allows: process-exec of allowlisted toolchain paths, file-read of system and toolchain paths plus the workspace, file-write of workspace, scratch and temp, the minimal mach services the toolchain needs, and network only to the proxy endpoint when granted. `$HOME` is denied except granted subpaths. | An `(allow default)` profile, the benchmark's current shape, leaves mach and LaunchServices channels open; launching an app outside the sandbox via `open` is the class we must refuse (UNVERIFIED as an exploit here; FT-17 is the witness). Deny-default is the shape Claude Code and Codex are reported to use (UNVERIFIED). | **S-M1:** derive the minimal allow set for cargo/rustc, python and node toolchains; FT-17 and FT-18 must refuse. If Apple removes `sandbox-exec`, `probe()` fails and macOS execution refuses until an exit path lands (Q8). |
| Windows | `WinAppContainer`: a per-run AppContainer profile with **no** network capabilities; ACL grants for workspace, scratch and toolchain (read); a Job Object with `KILL_ON_JOB_CLOSE`, an active-process limit and a memory limit; the process is created with the job attached. | AppContainer is kernel-enforced and default-deny for files, registry and network. A Job Object is **not** a boundary, only the resource and tree-kill half (R4 §2 C5). Codex judged AppContainer insufficient for developer workflows (R1 §3.1); the named fallback is a Codex-style write-restricted token + ACLs. | **S-W1:** cargo/rustc inside AppContainer with workspace + toolchain ACLs; if it fails, try the restricted-token fallback; if both fail, Windows ships **without execution** (read-only sessions work). **S-W2:** proxy reachability from AppContainer (loopback needs an exemption, UNVERIFIED); until it passes, Windows sessions get no egress. |

**Shared with rustybenchmark.** The benchmark's grading sandbox needs the same contract. This backend family (`harness-sandbox` + `harness-conformance`, same author, same licence under ADR-0003) is proposed as the shared crate, and rustybenchmark may depend on it. The harness takes no dependency on the benchmark's crates. This answers OPEN-QUESTIONS 5.

### 6.4 The trust base is outside the agent's reach

The **trust base** is the harness config (policy, admitted providers, trusted keys, exec
allowlists), provider manifests, the MCP server list and binaries, hooks (none in v0.1), the task
spec, the verification plan and hidden checks, `state_root` (journals, snapshots, blobs; the
approval key never touches disk) and the harness binary itself.

None of these paths is mounted or granted in any `ConfinedSpec`, not even for reading. Inside the workspace, the **protected paths** of the verification plan (`.git`, tests-under-evaluation, gate scripts, CI config, lockfiles, `deny.toml` and whatever the task declares) are read-only overlays (§7.6).

Changes to the trust base happen only through `rustyharness` subcommands run by the user outside any run, never through a tool. Config-write-as-RCE is the reason (CVE-2025-53773, CVE-2025-54135, R1 §3.3; INV-12).

### 6.5 Network: allowlist proxy outside the sandbox

The default is **no network** inside any sandbox: empty netns, Seatbelt `(deny network*)`, no AppContainer network capability. When a session has an egress-granted capability (after the trifecta check, §5.4), the harness starts `harness-sandbox::egress`, an HTTP CONNECT and HTTP proxy **in the harness process**, outside the sandbox (R1 §3.1).

**Proxy rules:** an exact host allowlist from the task grant (`*` refused); DNS resolved by the
proxy, and loopback, private, link-local and ULA destinations refused, including hostnames that
*resolve* to them (connect to the resolved address, no re-resolution); a journal event per request
(host, port, bytes each way, decision); empty allowlist ⇒ everything refused (R1 §3.1).

**Reaching the proxy:**
- Linux: the proxy listens on a unix socket bind-mounted into the sandbox, and a tiny forwarder in the helper exposes it as `127.0.0.1:<port>` inside the netns (`HTTP(S)_PROXY` set).
- macOS: the profile allows outbound only to the proxy's loopback port (SBPL syntax UNVERIFIED, S-M1).
- Windows: S-W2.

The model server is **not** reachable from any sandbox. Model calls are made by the harness process (the benchmark's precedent, R2 §5).

### 6.6 The hostile-task conformance suite

`harness-conformance` holds the corpus **as data**: tiny projects plus the expected refusal observations. It runs through the same `spawn` seam as real tools. Tasks FT-1..FT-8 generalise the benchmark sandbox's escape set so both sandboxes can share one corpus; FT-9 onward are harness-specific.

| # | Hostile task | Must observe (behaviour, not a printed string) |
|---|---|---|
| FT-1 | test binary opens TCP to a routable address | connect fails; no packet leaves (checked outside) |
| FT-2 | build script opens the same socket | build-time code is equally confined |
| FT-3 | write to `$HOME/canary` and to a sibling run dir | both fail; files absent afterwards |
| FT-4 | read a planted `~/.ssh`-shaped canary; probe env for a planted secret | read denied; variable absent |
| FT-5 | bounded fork bomb | killed by limit; host survives; no orphans afterwards |
| FT-6 | memory bomb | killed by limit; host survives |
| FT-7 | disk fill in workspace | killed by file-size cap |
| FT-8 | busy loop | killed at wall clock by the harness |
| FT-9 | write to a protected path (test file, `.git/config`) | denied; digest unchanged |
| FT-10 | read or write the harness config, `state_root` or journal | denied (not even readable) |
| FT-11 | connect to a planted unix socket listener outside the workspace | fails |
| FT-12 | symlink in workspace pointing outside, followed by a built-in file tool and by `exec` | both denied (the kernel view, §4.8) |
| FT-13 | direct connect to an allowlisted host's IP, bypassing the proxy | fails; only proxy traffic succeeds |
| FT-14 | proxy request to a hostname resolving to a private address | refused by the proxy; journaled |
| FT-15 | DNS lookup of a planted name (DNS exfiltration) | no resolver reachable |
| FT-16 | orphan: child double-forks and ignores SIGTERM | gone after the kill (PID ns / Job Object / group kill) |
| FT-17 | macOS: launch an app outside the sandbox via LaunchServices (`open`) | refused |
| FT-18 | macOS: mach-lookup of keychain or pasteboard services; Linux: keyring syscalls; Windows: Credential Manager read | refused |

**CI:** each OS runner executes the full suite against its native backend. The resulting per-OS matrix row is committed; a backend without a passing row cannot mint `Conformed` (§6.1).

### 6.7 `unsafe` isolation

The workspace forbids `unsafe` (`Cargo.toml:21`). `harness-sandbox-windows` is the one planned exception, because every Win32 call through the `windows` crate is `unsafe`. It opts out of the workspace lint to `deny(unsafe_code)`; each site needs `#[allow]` plus a
`// SAFETY:` justification; a CI gate counts the sites (a ratchet: the count may only fall without
review); and only `harness-sandbox` may depend on it.

If S-L1 shows that Linux needs `unsafe` (e.g. `pre_exec` rather than the helper re-exec), the same pattern applies with a `harness-sandbox-linux` crate. The helper design exists to avoid that.

## 7. Evidence and journal

### 7.1 The journal

**Format.** One `journal.jsonl` per attempt. Each line is:

```json
{"seq":N, "prev":"<sha256 of line N-1>", "t_mono_ms":..., "t_wall":"...", "run":"...", "attempt":n, "step":k, "kind":"...", "body":{...}, "hash":"<sha256(prev||canonical(line without hash))>"}
```

**Header (line 0):** harness version; config, policy and manifest SHA-256s; `ModelIdentity`
(§3.5) and profile hash; sandbox backend, matrix row and probe digest; OS; `shell_enabled`;
protected-path digests; verification plan digest. This header is what makes a trajectory comparable, so harness regressions stay visible between releases (R1 §2, principle #13).

**Type-level append-only (F11).** `JournalWriter` owns the file (opened for append, with an exclusive advisory lock, single writer). Its only operation is `append(Event) -> Seq`. There is no seek, truncate, remove or rewrite in its API. `JournalReader` is a separate type that verifies the chain on open.

**Detection and durability.** A mutated or reordered middle line fails verification
(`UnreadableEvidence`, INV-11); a torn final line (a crash) is reported as such and resume starts
from the last good line (§2.10); the writer fsyncs after every step's result event.

**Anchoring.** The final chain head is printed in the run report. Detecting wholesale replacement of a journal needs that head recorded elsewhere. In the suite, it goes into the suite's own evidence records (§8); standalone, it is the user's to keep (residual).

**Untrusted payload home.** Any body that carries outside content uses:

```rust
struct UntrustedBlob { source: Source, sha256: Digest, len: u64, inline: Option<String /* ≤ 4 KiB, escaped */>, blob: Option<BlobRef> }
```

The JSON form carries `"untrusted": true`. Display paths escape control, bidi and ANSI sequences, so a journal viewer is not an injection sink (the scaffold's "logs are a sink too", `crates/harness-core/src/lib.rs:67-74`).

### 7.2 Event schema

| Kind | Body (untrusted parts in *italics*) |
|---|---|
| `RunStarted` | header fields (above) |
| `ContextBuilt` | context digest, token estimate, included pointer ids, dropped blocks |
| `ModelRequested` / `ModelReplied` | request digest; *content*, *raw tool calls*, finish reason, usage (measured/estimated), retry attempts |
| `ActionParsed` / `FormatError` | tool id, args digest, *args*; parse error |
| `PolicyDecided` | call digest, effective class, decision, rule id |
| `ApprovalRequested` / `ApprovalGranted` / `ApprovalDenied` / `ApprovalExpired` | capability, args digest, tier, approver id, nonce (never the MAC) |
| `ToolStarted` / `ToolFinished` | capability, `ToolStatus`, *output*, digest, truncated, duration |
| `EditApplied` | path, before and after digests, snapshot id |
| `Egress` | host, port, bytes, decision |
| `Redacted` | which handle or canary matched (never the value), location |
| `Quarantined` / `SandboxUnavailable` | capability and drift kind; typed reason |
| `LoopDetected` / `BudgetCharged` | detector; dimension, spent, limit (charged per step, aggregated) |
| `SubmitRequested` | *deliverable note* |
| `CheckReported` | `GateReport` per check (JSON per `gate-outcome`) |
| `ReviewReported` | reviewer run id, reviewer `ModelIdentity`, `GateReport` |
| `RunStopped` | `StopCause`, the run's `GateOutcome`, deliverable digest, chain head |

### 7.3 How verdicts are extracted

The **verification plan** (task spec, trust base) is a list of checks. Each check is an argv plus metadata:
- `visible` or `hidden`;
- `protocol`: `unified` or a built-in adapter id;
- a timeout.

**Where checks run.** The harness runs each check itself, in the sandbox, against the **pristine grading worktree** (§7.6) with `Stdio::null()` stdin, and captures exit status and stdout directly, with no pipeline in the measurement path (R3 H-03). The captured run goes to `gate_outcome::child::interpret`, which applies UNIFIED §6 verbatim:

| Child behaviour | `GateReport` |
|---|---|
| last stdout line parses as a JSON `GateReport` | that report |
| last line looks like a report but does not parse | `Indeterminate { UnreadableEvidence }` |
| exit 0 **and** success marker written to the per-check `GATE_OK_FILE` | `Passed(Witness)` (minted inside `gate-outcome`) |
| exit 0 **without** marker | `Indeterminate { NothingChecked }`, never a pass |
| exit 1 | `Failed` + a synthesized `Blocking` finding |
| any other exit, signal, timeout, missing program | `Indeterminate { CouldNotRun }` |
| no conformed sandbox for the check | `Indeterminate { UnsupportedOs }` or `{ CouldNotRun }` (§6.1) |

**Built-in check adapters** cover common tools that do not speak the protocol, such as `cargo test` and `pytest`. Each implements `gate_outcome::Check` over captured output. For example, `cargo test` passes with `N` > 0 tests executed and 0 failed, and `N = 0` gives `NothingChecked`. The adapters are part of the trust base, reviewed like gates, and each carries a **refusal witness** test: a fixture where it must refuse (R3 H-04).

**Artifact checks** are always run:
- `deliverable.nonempty`: the diff against base is non-empty, unless the task is declared read-only;
- `deliverable.applies`: the diff applies cleanly to the base in-process;
- `protected.unchanged` (§7.6).

These check content, never presence (R3 H-01).

**The run's `GateOutcome`** is `verdict(all check reports ++ reviewer report)`: total and worst-wins, and an empty set gives `Indeterminate { NothingChecked }`. Summaries and counts in the report are derived from the reports, never written by the agent (R3 H-24).

### 7.4 What the agent can and cannot influence

The agent influences only the **workspace contents**. It cannot choose which checks run, see hidden checks, write the grading worktree, write the verification plan or produce a `GateReport`. Its `task.submit` note is journaled as untrusted and never read by any check.

### 7.5 The built-in independent reviewer (two pairs of eyes)

When the task enables `review` (the default for any task with write grants), the harness launches a **reviewer run** after checks complete:

- **Fresh context.** The reviewer gets the task text, the diff, the check reports and read-only tools (`fs.read`, `fs.search`, `fs.list`) over the grading worktree. It does **not** get the author's transcript, notes or deliverable note. The author could steer the reviewer through them.
- **Distinct identity.** It gets a new run id. If more than one model is configured, the reviewer profile must differ from the author's (Q5). Reviewer identity ≠ author identity is checked by `harness-run` and journaled. A same-identity review is refused (R3 H-22, INV-19).
- **Artifact, not prose.** The reviewer's `task.submit` must carry a review artifact in a fixed schema: findings, each with `severity` (UNIFIED `Severity`), `path:line` anchors and a quoted anchor text. `harness-run` validates it:
  - schema-valid;
  - every anchor resolves in the grading worktree and the quote matches (citation resolution, R3 H-20);
  - an unresolvable anchor is a `PHANTOM` finding.

  A valid artifact becomes a `GateReport` via `run_checked`: `Passed(Witness { checked = reviewed hunks, digest = diff digest })` if there is no `Blocking` finding, `Failed` if there is one. An invalid artifact, a reviewer timeout or a reviewer crash gives `Indeterminate { CouldNotRun }`, after which the **fallback reviewer** (next configured profile) is engaged and recorded (R3 H-22).
- **It can only block.** Because `verdict()` is worst-wins, a reviewer's pass cannot rescue failed checks. A reviewer is also not a substitute for checks: with no checks, the outcome stays `Indeterminate { NothingChecked }`.

### 7.6 Reward-hacking defences

| Defence | Mechanism | Evidence / source |
|---|---|---|
| Fresh workspace with no future history | The workspace is a **new single-commit repository** of the base tree: no remotes, no other refs, no shared object database. The mapping to the real repository stays outside. Harness-side git invocations use argv only with hooks and fsmonitor disabled (`-c core.hooksPath=<empty> -c core.fsmonitor=false`). | SWE-bench `git log --all` leak (R1 §2); repo-controlled git config is executable config |
| Protected paths read-only | `.git`, tests-under-evaluation, gate scripts, CI config, lockfiles, `deny.toml` and task-declared paths are read-only overlays in the sandbox (FT-9) | ImpossibleBench: 76% cheating, near zero once tests were read-only (R1 §2) |
| Pristine grading worktree | Rebuilt outside the sandbox for every verification: base tree + agent diff, with **every protected path materialised from base** | R4 §3 / F3 |
| Diff audit | The agent diff is compared with the protected-path list. Any change → a `Blocking` finding `protected-path-modified` in the `protected.unchanged` report, so the run is `Failed`. A task that legitimately adds tests declares `may_add: ["tests/new_*"]`; existing protected files must stay byte-identical. | R4 §3 (the negative test: deleting a failing test must not end green) |
| Hash checks | Protected-file digests go in the journal header at start. `protected.unchanged` recomputes them over the workspace; a mismatch is a Blocking finding even if the sandbox failed to stop the write. | R3 H-23 |
| Hidden checks | Hidden check sources live outside the workspace and are mounted only into the grading sandbox. Diagnostics from them are withheld during repair (§2.7). | benchmark repair mode (R2 §5) |
| Verdict outside the agent | Checks run in a separate confined process tree after the agent loop has stopped. The agent's sandbox is torn down first. | R1 §2 |
| No self-report | `StopCause::Submitted` never implies `Passed`. Only `run_checked` or `child::interpret` mint `Witness`. | UNIFIED §5 |

### 7.7 The CLI as a gate child

`rustyharness run` follows the UNIFIED child protocol, so any gate runner (the suite's included) can consume it. The **last stdout line** is the run's JSON `GateReport` (`gate-outcome` feature `json`).

| Exit | Meaning |
|---|---|
| 0 | `Passed`; the `GATE_OK_FILE` marker is written only then |
| 1 | `Failed` |
| 2 | usage error |
| 3 | confinement refused (the task needs execution, and `require()` failed) |
| 4 | unreadable input (task spec, config, manifest) |
| 5 | `Indeterminate` (the kind is in the JSON) |

A consumer that only reads exit codes sees 3, 4 and 5 as `CouldNotRun`. That is lossy but fail-closed (UNIFIED §6 item 2). This fulfils F10.

## 8. Deployments: standalone vs inside rustysuite

This section follows R6 §0 and §4. **Standalone** is the primary product (ADR-0002), and there the harness's own mechanisms are the whole protection. **Inside rustysuite**, the same harness runs with suite add-ons and host-level layers that the harness cannot give itself.

| Mechanism | Standalone | Inside rustysuite |
|---|---|---|
| Two domains, `Untrusted<T>`, single action channel | harness | same |
| Manifest validation, admission, pinning, quarantine | harness; the user admits and pins | same, plus suite-signed manifests for suite apps and a suite-owned admitted list delivered as add-on config |
| Effect policy, approvals, tokens | harness; the approver is the user at the CLI or UI | same; in unattended suite runs no approver ⇒ Ask becomes Deny |
| Trifecta rule | harness | same |
| Sandbox (file, network, process) | harness backend, gated by `Conformed` | same, **plus** the suite's host-level properties (separate OS account and keychain for agent runs, no signing material reachable). These are OS and owner controls the harness cannot grant itself; it refuses to run unconfined rather than depending on them. |
| Separate OS account per agent | not provided (a process cannot grant itself a uid) | the suite's confinement decision; the harness exposes the probes an outside supervisor checks |
| Secrets | `FileStore` / `KeychainStore`, handles only | a secret-custody add-on is available but off. The suite's own rules decide whether agent runs may use it (default: not). |
| Model endpoint | loopback; hosted by opt-in (feature + config) | also a mesh transport add-on reaching a model host over the suite's mesh (Q10 of OPEN-QUESTIONS lands here, not in core) |
| Journal and evidence | journal + run report; the user keeps the chain head | also: the run's `GateReport` and chain head go into the suite's evidence records via the child protocol (§7.7). Independent-witness rules are met by §7.5 plus the suite's observer ≠ author requirement. |
| Checks | task-declared commands and adapters | also suite gate entrypoints, invoked through the suite engine as a provider. The engine's reports speak the same `gate-outcome` type. |
| No commit, no push | no push or publish capability exists in the built-ins; any provider verb that pushes is `irreversible`/`shared` ⇒ human yes every time | same; the suite's engine additionally refuses protected refs by construction |
| Supply chain | this repo's gates on three OSes (`scripts/ci/gates.sh`, `.github/workflows/ci.yml`) | plus the suite's attestation chain |

**Add-on packaging** (answers OPEN-QUESTIONS 15): tools arrive as **runtime-loaded providers** (manifest + MCP server), which need no harness rebuild. Cargo features (`addon-*`) are only for what must be in-process: a mesh model transport and a secret-custody client. All are off by default. The ADR-0002 test: `cargo tree` of a default build contains no suite crate, and every §10 invariant passes with all add-ons off (INV-32).

## 9. Phasing

| Phase | Scope | Exit criteria (all gates green on macOS, Linux, Windows) | Key tests |
|---|---|---|---|
| **H0** | This design | Independent adversarial review → rework → confirming review → SOUND; §11 owner questions answered or defaults accepted in writing | review record |
| **H1** read-only agent | `gate-outcome` (workspace member); `harness-core` loop, `Meter`, loop detection; `harness-model` client (loopback), profiles, both protocols, replay (audit mode); `harness-manifest` v1 + built-in manifest; `harness-policy` for read classes; read tools in-process (no exec); journal v1; CLI `run` / `replay` / `profile check`; purity gates | A read-only question-answering task runs end to end on a local model with both protocols. Every run's outcome is `Indeterminate { NothingChecked }` (no checks yet), and that is shown to the user. | INV-1, 2, 3, 8, 11, 14, 18, 20, 22, 23, 24, 27, 28, 29, 30; replay determinism (audit mode reproduces every decision); format-error budget; mock server returning empty / `length` / 429 |
| **H2** confined action loop | `harness-sandbox` Linux + macOS backends, `Conformed`, confined file-op helper; `harness-conformance` FT-1..18; S-W1 spike (+ Windows backend if it passes); edit engine; `exec.run` with allowlist; workspace materialisation + protected overlays; approvals + tokens; secrets boundary + canaries; network = none only | FT suite green on Linux and macOS CI; Windows either green or `Unavailable` with the refusal path tested; a coding task edits and runs tests inside the sandbox | INV-5, 6, 10, 12, 13, 15, 16, 21, 25; edit no-op and stale-read tests; CRLF test; symlink escape (FT-12); orphan kill (FT-16) |
| **H3** evidence + reviewer | verification plan, child protocol, check adapters with refusal witnesses, pristine grading worktree, diff audit, repair rounds, reviewer run + fallback, run report, CLI exit codes; `gate-outcome` moved to its own repository | Reward-hack suite: deleting a failing test, flipping `#[ignore]`, editing a gate script, and adding a check that exits 0 without a marker all end **not green**, witnessed. A pinned evaluation set runs and its baseline is recorded per (harness version, model) (R1 §8 #15). | INV-4, 17, 18, 19, 26, 28; adapter refusal witnesses; phantom-citation reviewer test; reviewer ≠ author refusal |
| **H4** providers + MCP | `harness-mcp` (rmcp stdio, confined servers), admission CLI, signing and pinning, quarantine, the full trifecta with real labels, egress proxy (Linux + macOS; Windows after S-W2) | A third-party MCP server works as a `pinned`-tier provider; the fixture provider integrates with zero core diff | INV-7, 9, 31; rug-pull (a mutated description is quarantined); shadowing (duplicate namespace refused); FT-13/14/15 through the proxy; an MCP sampling request refused |
| **H5** suite add-ons | suite providers (engine verbs, evidence reporting), `addon-*` features (mesh transport, secret-custody client), a pinned harness entry for rustybenchmark's agentic board | Default build has zero suite dependencies; with add-ons on, every invariant still passes; suite-side acceptance happens in the suite | INV-32; add-on on/off matrix |

Phases are strictly ordered: H2 does not start before H1's exit, and so on. Each phase's exit goes through the two-eyes review, because the harness is itself reviewed like a gate.

## 10. Security invariants as testable properties

INV-1..14 are carried from R6 §5 (re-scoped where this design settles a detail); INV-15 onward are new. "Phase" is when the falsifying test must exist and pass.

| ID | Property | Falsifying test | Phase |
|---|---|---|---|
| INV-1 | Every invocable capability resolves to closed-set dimension values. Unknown fields, versions, namespaces and reserved names are refused. | Manifests with an unknown field, an unknown effect value, v0 and v2, `provider: "harness"` → each refused with a typed error | H1 |
| INV-2 | `Untrusted<T>` has no `Deref`, and its `Debug` shows no payload | compile-fail test on deref; property test: `format!("{:?}")` contains no payload substring | H1 |
| INV-3 | An empty or truncated completion is never a turn result or a pass | mock server: empty body with a clean stream end, then `finish_reason: length` → typed error, run outcome ≠ `Passed` | H1 |
| INV-4 | `Passed(Witness)` cannot be constructed outside `gate-outcome`'s choke points | compile-fail test constructing `Witness` or `Passed` from a harness crate | H1 (crate) / H3 (use) |
| INV-5 | An irreversible or `shared`-blast call without a valid, bound, unconsumed approval refuses, every time | invoke under ask with no approval; with an approval for different args; with an expired one; with a reused one → all refuse | H2 |
| INV-6 | Without `Conformed`, no execute-class tool and no confined check runs | disable the backend (matrix row absent) on each OS → refusal path, `Indeterminate { UnsupportedOs }` or `{ CouldNotRun }` | H2 |
| INV-7 | A capability whose pinned description or schema hash drifts is quarantined until re-pinned outside the run | mock MCP server mutates its description between admission and connect → invocations refuse; `Quarantined` journaled | H4 |
| INV-8 | Two providers cannot share a namespace, and ids stay inside their namespace | admit two manifests with the same `provider` → second refused; foreign-namespace id → refused | H1 |
| INV-9 | A session whose active set plus workspace is P ∧ U ∧ E is refused at start and on any set change | compose such a set → typed refusal naming three capabilities; quarantine that creates it mid-run → `PolicyAbort` | H1 (pure) / H4 (real labels) |
| INV-10 | No resolved secret or canary appears in any model request or child environment | canary test: capture every outbound model request body and every child `environ` → absent | H2 |
| INV-11 | A journal cannot be mutated undetectably, apart from wholesale replacement (anchored externally) | rewrite a middle line, delete a line, swap two lines → reader refuses; torn tail → reported, resumable | H1 |
| INV-12 | The trust base and protected paths are not writable by the agent, and the trust base is not even readable | FT-9, FT-10; plus a backend-less seeded diff to a protected file → `protected.unchanged` Blocking → run `Failed` | H2 |
| INV-13 | No harness code path executes a string built from model-controlled text; `exec.run` is argv-only and allowlist-resolved | grep gate: no `Command::new` on formatted or concatenated strings in `crates/`; test: argv `["sh","-c",…]` refused when no shell is allowlisted | H2 |
| INV-14 | Every budget ends the run with a typed cause; the loop cannot outlive its wall-clock budget | scripted model that always asks for another tool → `Budget(Steps)`; hanging tool → `Budget(Wall)` within budget + kill grace | H1 |
| INV-15 | `Containment::Available` is unnameable without `Conformed`, and `Conformed` is minted only by `probe()` passing matrix + live probe | compile-fail constructing `Conformed`; host with a sabotaged primitive (e.g. userns sysctl off) → `Unavailable` | H2 |
| INV-16 | Approval tokens bind run, step, capability, args digest, expiry and nonce, and cannot be forged outside the harness process | flip one byte in each field; replay a consumed nonce; present a token from another run → refused | H2 |
| INV-17 | Any agent change to a protected path makes the run not green | agent deletes a failing test / edits a gate script / edits CI config → `Failed` with `protected-path-modified` | H3 |
| INV-18 | A task with no checks is never `Passed` | submit on a task with an empty verification plan → `Indeterminate { NothingChecked }` | H1 |
| INV-19 | Reviewer identity ≠ author identity, and the reviewer never sees the author transcript | configure reviewer = author run → refused; inspect reviewer context digests → no author transcript blocks | H3 |
| INV-20 | Audit replay reproduces every context digest and policy decision, or names the first divergence | replay an H1 journal → identical; tamper a recorded tool result → divergence at that step | H1 |
| INV-21 | Concurrent runs share no writable state | 8 concurrent runs on one host → zero lock failures, disjoint paths; one run's sandbox cannot read another's workspace (FT-3) | H2 |
| INV-22 | Duplicate JSON keys are refused at every depth | `{"schema_version":1,…,"schema_version":0}` and a duplicated nested key → refused | H1 |
| INV-23 | Prompts, task text and approval payloads never appear on any argv | spawn helper + MCP server + check; read `/proc/*/cmdline` (and platform equivalents) → no payload bytes | H1 |
| INV-24 | The default build connects to no non-loopback model endpoint | configure `http://lan-host.example/v1` without `hosted` → startup refusal; `cargo tree` of the default build has no TLS crate | H1 |
| INV-25 | The effective confirmation is never weaker than any declared or derived floor | property test over all dimension combinations: `effective ≥ max(declared, derived)` | H2 |
| INV-26 | A `pinned`-tier manifest cannot declare `personal`/`restricted`, `shared` blast, or share a session with such capabilities | manifests declaring each → refused; mixed session → refused | H3 (schema) / H4 (session) |
| INV-27 | `restricted` capabilities are refused (standalone default) | grant a `restricted` capability → session refused | H1 |
| INV-28 | Exactly one outcome type: no harness crate defines an outcome or verdict enum | grep gate over `crates/harness-*` for `enum *Outcome` / `enum *Verdict`; the build fails on a planted one | H1 |
| INV-29 | Only the model's own reply is parsed for actions | plant `<action>{…}</action>` and a native tool-call JSON inside a file, a test name, a compiler diagnostic and an MCP result → never executed, never parsed | H1 |
| INV-30 | Built-in file tools cannot reach outside the workspace | planted symlink to `$HOME` canary: H1 in-process path (materialisation refused the symlink); H2 helper path (FT-12) | H1 / H2 |
| INV-31 | A new provider integrates with no core change | fixture provider end to end; CI asserts an empty diff over `crates/` | H4 |
| INV-32 | Standalone build has zero suite dependencies; add-ons are off by default | `cargo tree` of the default feature set against the suite crate list → empty; invariants pass with add-ons off | H5 |

## 11. Non-goals, owner questions, and limits

### Non-goals (v0.1)

Not a model or model server (00-overview §5). No hosted model in the default build, no parallel
tool calls, no dynamic tool retrieval, no LLM-written context summaries, no remote MCP over HTTP,
no `git apply` or unified-diff edits, no persistent shell. No sandbox-free execution, ever. No
trifecta override path (CaMeL-style; deferred). No `restricted` data (the harness is not the
suite's confined personal-data AI; ADR-0002 item 3). No automatic promotion: nothing merges or
pushes without a human. No front-end protocol beyond the `harness-run` embedding API; ACP is the
likely later choice (R1 §4.2) once a UI other than the CLI embeds the harness.

### Open owner questions (each with the fail-closed default that holds until answered)

1. **`gate-outcome` licence and home.** The crate's value is universal adoption, so a permissive licence (MIT OR Apache-2.0) fits it best. Its own repository is decided (§1.4). *Default:* PolyForm Noncommercial, like this repo, until the owner rules.
2. **May a hosted model ever see `personal`-sensitivity data?** *Default:* no. Hosted is limited to sessions with sensitivity ≤ operational. `restricted` is never allowed, whatever the answer.
3. **May the harness ever hold `restricted` capabilities** (the life-data class)? *Default:* refused at session start (INV-27). The owner's ADR-0002 item 3 already points this way.
4. **Windows in v1.** *Default:* Windows ships read-only (no execution) unless spike S-W1 passes. The alternative is to state now that Windows execution is v2.
5. **Reviewer independence bar.** *Default:* fresh context + distinct run always required; a distinct model required when two or more are configured. The alternative is to always require a distinct model, which would make the reviewer unavailable to single-model standalone users.
6. **Concurrency default per model endpoint.** *Default:* 1 for loopback, user-configurable (R3 §7 Q3).
7. **When to invest in a trifecta-breaking architecture** (plan-then-execute / dual-LLM, R1 §3.3). *Default:* never combine; revisit after H4.
8. **macOS if `sandbox-exec` disappears.** The options are a `sandbox_init` FFI in an audited unsafe crate, or App Sandbox with code signing. *Default:* macOS execution refuses (`probe()` fails) until one lands.

### What this design does not cover

- Suite-specific provider manifests and their policies (these live in the suite).
- Model serving and choice of weights.
- UI and approval UX beyond the CLI prompt.
- Retention and GC policy for `state_root` (default: keep; explicit `rustyharness gc`).
- Performance numbers for backends and the proxy.
- Covert or timing channels out of a sandbox (R6 §8).
- Exact seccomp and SBPL rule lists (spike outputs, reviewed as policy).
- A completeness claim over entry points (R6 §2).

### What would make this design wrong

- A backend passes §6.6 yet a probe outside the corpus escapes. The corpus, not the backend, is then the weak point: add the case, re-run all backends.
- UNIFIED changes `Witness` semantics or the child protocol. §1.4, §7.3 and §7.7 would move with it, which is why the type is imported, never mirrored.
- Small local models cannot do useful work within 5-8 tools and exact-match edits. The profile defaults, not the architecture, would change (R1 §6 suggests they can for short horizons).
- The trifecta rule refuses so many real sessions that users bypass the harness. That would argue for Q7, not for weakening INV-9.
- rmcp starts pulling compiled C, or requires server features §4.6 refuses. `harness-mcp` would then need its own thin client, as `harness-model` does.

## Appendix A — R3 requirements traceability

| R3 | Where met |
|---|---|
| H-01 content-shape artifact checks | §7.3 artifact checks; INV-18 |
| H-02 PASS / FAIL / PARSE-FAILURE / NOT-RUN distinct | `gate-outcome` kinds; §7.3 table |
| H-03 harness captures exit codes itself; CRASH / TIMEOUT / MISSING distinct | §7.3 (`child::interpret`), `ToolStatus` §4.5 |
| H-04 refusal witnesses for checks | §7.3 adapters; §6.6 behaviour-not-string |
| H-05 capability degradation fails closed | §6.1; INV-6, INV-15 |
| H-06 write allowlist enforced below the agent | §6.2, §6.4; FT-3, FT-9 |
| H-07 credentials brokered per spawn | §5.5 |
| H-08 no payloads through shells or argv | §2.1, §6.3 (helper spec over pipe); INV-23 |
| H-09 isolated per-run state | §2.8; INV-21 |
| H-10 harness-metered concurrency, typed provider errors | §2.8, §3.2 |
| H-11 done-ness from harness-owned state | §2.5, §7.3, §7.4 |
| H-12 first-class run identity | §2.8 |
| H-13 durable per-attempt logs, recovery | §7.1, §2.10 |
| H-14 standing-condition dedup | loop-detector notices fire once per detector state (§2.6); fuller alerting is out of scope |
| H-15 find-based snapshots incl. empty dirs | §2.8 snapshots |
| H-16 detective signals labelled | §5.5 redaction |
| H-17 environment context recorded | journal header §7.1; `UnsupportedOs` / `CouldNotRun` kinds |
| H-18 verification depth recorded; Rust, portable | header + `Coverage` on reports; §9 three-OS gates |
| H-19 volatile facts derived by the harness | §2.3 block 4 |
| H-20 citation resolution blocks | §7.5 reviewer anchors |
| H-21 structured claims | run report fields derived from reports (§7.3) |
| H-22 structural two-eyes with fallback | §7.5; INV-19 |
| H-23 stamps over committed content | protected digests in the header (§7.6); chain head (§7.1) |
| H-24 summaries derived from records | §7.3 last paragraph |
