# Security policy

rustyharness runs language models that act through tools. Its security posture:
confinement fails closed, tool and model output is untrusted data, irreversible
actions need a human yes, and results are decided by evidence. A way around any of
these is a security bug.

**Status:** H1, the read-only agent (design `docs/01-design-v0.1.md` §9). The
`rustyharness` binary drives a model on loopback through read-only workspace tools
and writes a hash-chained journal. Nothing the agent asks for executes: no sandbox
backend has passed conformance, so no execute capability can be granted, and no
provider outside the built-in one can be admitted. In scope for reports today: a
way for model or tool output to act as an instruction, to read outside the
workspace, to reach a non-loopback endpoint, to alter or forge a journal
undetectably beyond the residuals the design names (§7.1, §11), to make a run
report a pass, or to put a payload on an argv.

**Reporting:** do not open a public issue. Contact the maintainer privately
(Iwan Teague, via the GitHub profile that owns this repository) with a description
and, if possible, a reproduction. **Service levels (T2-A8):** acknowledgement
within 72 hours of a report; fixes targeted within 48 hours for critical,
7 days for high, 30 days for medium/low. Fixes land with a regression test.
