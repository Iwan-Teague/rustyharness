# Open questions

Owner decisions are marked **(owner)**; the rest are design questions the
research pipeline and v0.1 design must answer.

1. **Wire protocol for app capabilities:** suite-native, MCP, or native with an MCP bridge? MCP's security criticisms (tool poisoning, confused deputy) vs its ecosystem.
2. **Tool calling with small local models:** native tool calls vs a text protocol with grammar-constrained decoding. Which is more reliable, and on which models?
3. **Edit format** for code changes (unified diff, search/replace, whole file) — measured failure rates, and how the harness validates a patch before calling it delivered.
4. **Reuse vs write:** existing Rust agent/MCP crates (licences, maturity, C dependencies) vs our own.
5. **Shared crates with rustybenchmark:** the sandbox and the model client exist there. Shared crate, dependency, or copy?
6. **Hosted models (owner):** allowed at all? Opt-in per run? Never for some apps (the personal-data app)?
7. ~~**Membership (owner)**~~ — **decided 2026-09-23:** present-non-member dev/ops tool until the design is SOUND (ADR-0002).
8. **Licence (owner):** follows the suite ruling OI-05.
9. **Where the admin assistant ends:** which operations are ever automatic, and which always need a human yes?
10. **Remote model hosts over rustynet:** a GPU box on the mesh serving the model to a laptop — in scope for v1?
11. ~~**rustyharness vs the charter's "rustyai" (owner)**~~ — **decided 2026-09-23: separate, standalone-first** (ADR-0002). Original question: the charter already names rustyai as the confined AI over personal data (the personal-data app's router, maximal-sensitivity floor). Is rustyharness the runtime rustyai is built on, or a separate dev/ops tool that never touches personal data? The answer decides which security floor applies to which parts.
12. **Confinement mode (owner, suite OI-24):** separate OS account, VM/container, or interim only. The harness's execution design depends on it; the suite's confinement design v0.2 (reviewed SOUND) lists the properties P1-P8 any agent runtime must have.
13. **Local-only vs cloud models:** the existing in-suite agent tooling (rustynet-mcp ai-agent, rustyfin ai-agent) is cloud-capable or cloud-first; the Component D draft says local only. The harness needs one stated egress posture.
14. **Reuse rustyfin's assistant tool contract?** It is the most mature in-suite pattern (per-tool access mode, risk tier, confirmation policy, six-layer permission check, confirmation tokens; MIT) — but the proposed membership ADR would archive rustyfin.
15. **How suite add-ons are packaged** without a hard suite dependency: cargo features, runtime-loaded adapters (manifest + MCP server), or both?
16. **The shared gate-outcome type as a standalone crate:** the suite has one outcome type; rustyharness must use it without depending on the suite. Publish `gate-outcome` standalone (licence permitting, suite OI-05), or a mirrored type with a conformance test?
