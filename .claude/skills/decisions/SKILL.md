---
name: decisions
description: Project decisions, constraints, and context for MosaicFS. Load this before architecture work, design discussions, or when you need to understand why the project is shaped the way it is.
user-invocable: true
disable-model-invocation: false
---

# MosaicFS Project Decisions & Constraints

This document captures settled decisions, project context, and constraints that
should inform all architecture and design work. These are not derivable from
the code alone.

_Last updated: 2026-04-05. If a decision here conflicts with current code, flag
the discrepancy and ask the developer — the document may reflect intent not yet
implemented, or it may be stale._

---

## Project Identity

MosaicFS is a personal project by a single developer, built primarily for
educational purposes: exploring how humans and AI can build software together.
As a side benefit, it produces a working tool that solves a real problem.

**The problem:** Modern power users accumulate data across laptops, desktops,
NAS devices, virtual machines, and multiple cloud services. No single tool
provides a unified view of all that data or a consistent way to access it.

**The solution:** A peer-to-peer mesh of agents that index every file in every
location, federated metadata via CouchDB, and a virtual filesystem layer that
presents everything as a single coherent tree — accessible from any device, to
any application that can open a file.

**Audience:** Primarily the developer themselves. Others may adopt it, and their
needs matter but are not the primary design driver.

**Platforms (active today):** Linux and macOS.

---

## Settled Technology Choices

These are not up for debate in architecture discussions. They reflect the
developer's expertise and ability to maintain the project long-term.

- **Rust** for all core logic — agent, server, VFS, shared libraries.
- **CouchDB** as the federation and metadata store between peers.
- **MVC web framework (Loco + HTMX)** replacing React/Tauri for the UI. This
  is a settled decision. The developer knows traditional MVC patterns and can
  support them; the React/Tauri stack added complexity without proportional
  benefit.
- **macOS-specific technologies** (Swift, FileProvider, Keychain) are used only
  where required for the best native user experience. They are added carefully
  and kept as thin as possible over the Rust core.

---

## Architectural Direction

### Unified binary (in progress)

The project is moving toward eliminating the distinction between agent and
server. The target is a single binary where components are enabled/disabled
via configuration:

- Agent crawler: can be enabled or disabled per node.
- REST API: can be enabled or disabled per node.
- Web UI: can be enabled or disabled per node.

Each node is self-sufficient and does not rely on a designated "server" or
"client" program. This is a peer-to-peer design where all nodes are capable
of running any component necessary.

**Note:** The current codebase still has separate `mosaicfs-server` and
`mosaicfs-agent` binaries. This consolidation is a planned change, not yet
implemented.

### CouchDB stays

CouchDB remains the federation mechanism between peers. Each node talks to
CouchDB for metadata synchronization and replication coordination. This is
not changing.

---

## Decision-Making Principles

### Front-load risk

This is a hobbyist effort with no time pressure. The developer prefers to
tackle the hardest and riskiest parts first so that showstoppers surface
early, before the rest of the code is built around assumptions that turn out
to be wrong.

### Exploration is welcome

It is OK to try new frameworks or approaches if they are interesting, even if
the existing stack could work. The educational value of the project is as
important as the end product.

### YAGNI — aggressively defer what isn't needed now

The developer tends to get excited about potential features and future
enhancements. This is a strength for brainstorming but a risk for scoping.
When proposing architecture changes, actively challenge whether each piece
of work is necessary *for the current goal* or is speculative future work.

- If a feature is not required by the current phase, move it to a "Deferred"
  section with a one-line rationale for why it can wait.
- Prefer a working system with fewer features over a comprehensive design
  that takes longer to deliver.
- When the developer proposes something ambitious, it is OK (and encouraged)
  to say: "This could work, but it's not needed yet. I'd suggest deferring
  it to a later phase so we can ship X first."

The goal is not to kill ideas but to sequence them honestly.

### No unnecessary abstractions

Do not add layers of indirection, trait hierarchies, plugin systems, or
speculative generalization unless the use case concretely demands it. Three
similar lines of code are better than a premature abstraction.

### No greenfield rewrites

Prefer incremental migration over rewriting working components from scratch.
When proposing changes, build on what exists rather than starting over.

### One moving part at a time

Do not simultaneously change the framework, the data model, and the deployment
topology in a single scope of work. Each change should be deployable and
testable independently. When a larger effort requires multiple moving parts,
break it into separate numbered directories under `docs/changes/`, each with
its own narrow scope. A single part may touch multiple layers (UI, database,
API) — that's fine as long as it represents one coherent change. Parts of a
larger effort should cross-reference each other (e.g., "this is part 1 of a
3-part change covered by docs/changes/{001,002,003}").

### Boring technology bonus

When two tools can solve a problem and one is well-understood by the developer
while the other is novel, the well-understood one gets a significant bonus.
Novel technology is fine when it solves a problem boring tech cannot — but the
burden of proof is on the new thing. This is why the project uses CouchDB,
Rust, and traditional MVC: the developer knows them and can maintain them
long-term.

### Explicit boundaries over implicit conventions

In a codebase where AI assists with development, clear module boundaries
(crate boundaries, API contracts, configuration schemas) matter more than
unwritten code style conventions. An LLM can respect a crate boundary; it is
less reliable at following an implicit convention about which module "owns" a
concept. When designing, prefer making boundaries visible in the code structure
rather than relying on documentation to explain them.

---

## Change Management Process

All architecture changes follow a structured process managed through numbered
directories under `docs/changes/`. Each directory represents one narrowly
scoped "part" that includes the plan, tests, code, and documentation updates.

### The 5-step process for each part

1. **Discuss goal and clarify architecture.** Define what the change achieves,
   produce `architecture.md` with the current-state inventory and proposed
   deltas. The `/architect` skill guides this step.
2. **Develop detailed plan with technical details.** Produce `design-notes.md`
   with implementation specifics, review with the developer, and iterate until
   aligned. This includes review and feedback rounds.
3. **Implement.** Write the code, following the plan. Flag deviations from the
   plan as they arise rather than silently diverging.
4. **Review and adjust.** Verify the implementation against the plan. Fix
   issues, update tests, adjust the plan if reality differed from design.
5. **Document and commit.** Ensure documentation reflects what was actually
   built, commit the final state.

### Multi-part changes

When a change is too large for a single part, split it into separate numbered
directories under `docs/changes/`. Each part's `architecture.md` should
reference the others (e.g., "this is part 1 of a 3-part change covered by
docs/changes/{001,002,003}"). Each part goes through the full 5-step process
independently.

---

## When Code and Documents Disagree

Flag the discrepancy and ask the developer. Sometimes the document reflects
a decision not yet implemented. Sometimes the document is stale or wrong.
Do not silently assume either is correct.

$ARGUMENTS
