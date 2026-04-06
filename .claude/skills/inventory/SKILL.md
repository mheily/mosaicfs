---
name: inventory
description: Display a structural overview of the codebase — crates, binaries, deployment, API surface, frontend, dependencies. Use before implementation work to orient yourself, or when the user asks about project structure.
user-invocable: true
disable-model-invocation: true
---

# Codebase Inventory

Here is the current structural overview of the project, generated from the code:

```!
bash .claude/skills/architect/inventory.sh
```

Use this inventory to orient yourself before reading or modifying code. It tells you
which crate owns what, where the API routes live, how the system is deployed, and
what code is duplicated across crates.

When the inventory is not sufficient, use Glob, Grep, and Read to drill into
specific files. The inventory shows you *where* to look; the tools show you *what*
is there.

$ARGUMENTS
