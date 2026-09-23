# Open questions

Owner decisions are marked **(owner)**; the rest are design questions the
research pipeline and v0.1 design must answer.

1. **Wire protocol for app capabilities:** suite-native, MCP, or native with an MCP bridge? MCP's security criticisms (tool poisoning, confused deputy) vs its ecosystem.
2. **Tool calling with small local models:** native tool calls vs a text protocol with grammar-constrained decoding. Which is more reliable, and on which models?
3. **Edit format** for code changes (unified diff, search/replace, whole file) — measured failure rates, and how the harness validates a patch before calling it delivered.
4. **Reuse vs write:** existing Rust agent/MCP crates (licences, maturity, C dependencies) vs our own.
5. **Shared crates with rustybenchmark:** the sandbox and the model client exist there. Shared crate, dependency, or copy?
6. **Hosted models (owner):** allowed at all? Opt-in per run? Never for some apps (the personal-data app)?
7. **Membership (owner):** a full suite member (mesh, gates, development line) or a dev tool like rustybenchmark?
8. **Licence (owner):** follows the suite ruling OI-05.
9. **Where the admin assistant ends:** which operations are ever automatic, and which always need a human yes?
10. **Remote model hosts over rustynet:** a GPU box on the mesh serving the model to a laptop — in scope for v1?
