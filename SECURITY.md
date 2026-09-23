# Security policy

rustyharness runs language models that act through tools. Its security posture:
confinement fails closed, tool and model output is untrusted data, irreversible
actions need a human yes, and results are decided by evidence. A way around any of
these is a security bug.

**Status:** scaffold, design phase — nothing executes yet.

**Reporting:** do not open a public issue. Contact the maintainer privately
(Iwan Teague, via the GitHub profile that owns this repository) with a description
and, if possible, a reproduction. You will get an acknowledgement; fixes land with
a regression test.
