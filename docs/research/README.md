# Research pipeline

Research feeds the v0.1 design. Every input is kept here with its sources. The
design is reviewed independently before it is called SOUND.

| Stream | Question | Status |
|---|---|---|
| R1 prior art | What makes a good harness — SWE-agent, mini-SWE-agent, OpenHands, Aider, Claude Code, Codex CLI, opencode, Goose; failure modes; confinement; MCP and plugin models; Rust crates; local-model tool calling | **done** — [R1-prior-art-2026-09-23.md](R1-prior-art-2026-09-23.md) (17 ranked principles, 12 open questions) |
| R2 suite inventory | What already exists in the suite that the harness reuses, must comply with, or replaces | **done** — [R2-suite-inventory-2026-09-23.md](R2-suite-inventory-2026-09-23.md) (12 hard requirements, reusable parts, placement mechanics, contradictions) |
| R3 fleet lessons | Every failure the suite's own agent fleet has hit, turned into a requirement the harness meets by construction | running (lane ah02) |
| R4 Component D review | First independent review of the 2026-09-14 agent-harness draft: what carries forward | running (lane ah01) |
| R5 integration map | Per app: what an agent would do with it, effect classes, the capabilities it would declare | queued (lane ah04) |
| R6 threat model | The harness's own threat model, aligned with the suite's agent-fleet threat model and confinement design | after R1-R4 |

Then: v0.1 design → adversarial review → rework → confirming review → SOUND.
