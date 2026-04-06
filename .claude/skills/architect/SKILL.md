---
name: architect
description: Use when producing or revising architecture documents (architecture.md, design-notes.md). Enforces codebase-grounded analysis before any design work. TRIGGER when the user asks to design, architect, plan, or propose changes to the system.
user-invocable: true
disable-model-invocation: false
model: opus
effort: max
argument-hint: [description of the change]
---

# Architect Skill

You are producing or revising architecture documents as part of the project's
change management process (see Project Decisions for the full 5-step workflow).
This skill covers steps 1-2: clarifying the architecture and developing the
detailed plan. Your output will be committed to a numbered directory under
`docs/changes/` and will guide future implementation work. Errors in these
documents waste significant human and machine effort downstream. Accuracy
matters more than speed.

## Phase 0: Load Project Context

Read the project decisions and constraints before doing anything else. These
represent settled choices and principles that constrain your proposals:

[Project Decisions](../decisions/SKILL.md)

## Phase 1: Inventory the Current State

Review the auto-generated inventory below. This was produced by scanning the
actual codebase at invocation time.

```!
bash .claude/skills/architect/inventory.sh
```

After reviewing, supplement the inventory with anything the script missed that is
relevant to the proposed change. Use the Glob, Grep, and Read tools to verify
details. Do not guess. If you cannot confirm something from the code, say so.

The inventory (with your supplements) MUST appear in your output so the human can
verify your understanding before reading the proposed changes.

## Phase 2: Delta Framing

Every proposed change must be stated as a delta from the current state:

- **Today**: [how it works now, citing specific files/crates]
- **Proposed**: [what changes and why]
- **Justification**: [why the current state is insufficient]

If you cannot articulate why the current state is insufficient, the change is
probably unnecessary. Remove it.

### Rules

- **Never introduce a new crate, binary, framework, or abstraction without
  justifying why the existing structure cannot serve the need.** If the justification
  is "cleaner organization," that is not sufficient — the cost of restructuring must
  be weighed against the benefit.
- **Never describe an existing capability as new.** If the system already has a REST
  API, do not propose "building a REST API" — propose specific new endpoints or
  modifications to the existing one.
- **Never merge or split crates without justifying the change.** State what concrete
  problem the merge/split solves that cannot be solved by a smaller change (e.g.,
  extracting shared code into the existing common crate).
- **Acknowledge scope when replacing components.** If proposing to replace a UI
  framework, quantify what exists today (pages, components, features) and state
  whether the replacement targets feature parity, a subset, or a superset.

## Phase 3: Internal Consistency

Before finalizing, verify:

- **Phasing dependencies**: If Phase N depends on work from Phase M, confirm that
  Phase M actually delivers what Phase N needs. Read your own phase descriptions
  critically.
- **Endpoint references**: If you reference an API endpoint, confirm whether it
  already exists or is being newly created. State which.
- **Performance claims**: If you claim a latency or performance characteristic,
  state the full access path (e.g., "HTTP request -> handler -> redb read ->
  serialize -> HTTP response") and note whether the claim is validated or assumed.
- **Deployment impact**: If the change affects the deployment model (number of
  binaries, container topology, new build artifacts), state this explicitly.
- **CI/CD impact**: If the change adds build dependencies, static assets, new
  compilation targets, or new test requirements, state how CI is affected.

## Output Format

Structure your architecture document as follows:

1. **Current State Summary** — The inventory from Phase 1, condensed. This is the
   ground truth the reader uses to evaluate your proposals.
2. **Goal** — What the change achieves, in one or two sentences.
3. **Changes** — Each change as a delta (today / proposed / justification).
4. **Implementation Phases** — Ordered phases with explicit deliverables and
   dependencies between phases.
5. **What Does Not Change** — Explicitly list components, crates, and interfaces
   that are unaffected. This prevents the reader from wondering whether you forgot
   about them.
6. **Deferred** — Ideas, enhancements, or features that came up during design but
   are not needed for the current goal. Each item gets a one-line rationale for
   why it can wait. This is where YAGNI lives — it captures the ideas without
   letting them bloat the current scope.

$ARGUMENTS
