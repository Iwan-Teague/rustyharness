# ADR-0001 — Name and placement

- **Status:** Accepted (Iwan, 2026-09-23)
- **Decider:** the owner, answering two direct questions in session.

## Decision

1. The project is named **rustyharness** (binary `rustyharness`, crates `harness-*`).
2. It lives in its **own git repository from day one**, checked out at
   `projects/rustyharness` in the suite and added as a submodule (the ADR-019 model),
   rather than starting in-tree and splitting later.

## Options considered

| Option | For | Against |
|---|---|---|
| **rustyharness** (chosen) | Says what it is: the runtime around a swappable model. Matches the suite's lowercase what-it-does names and rustybenchmark's "pinned harness" vocabulary | Jargon to non-developers — a user-facing assistant built on it can take a friendlier name |
| rustyai | Friendly; broad enough for model serving and an assistant UI | Vague; implies the project is a model; "AI" dates |
| rustyagent | Names what it runs | Less precise about the runtime, checks and confinement |
| In-tree, split later | Nothing to publish yet; the personal-data app precedent | History has to be carried over at the split |
| **Own repo now** (chosen) | Clean history from the first commit | The suite's gitlink cannot be fetched by anyone until the owner publishes the repo (the same "not our ref" condition rustybenchmark has today) |

## Consequences

- Publishing the repo (creating the GitHub remote and pushing) is the owner's action; until then the submodule URL names the intended remote and fresh clones cannot fetch it.
- The suite's member lists and membership gates must learn about the new project (tracked in the suite action queue).
