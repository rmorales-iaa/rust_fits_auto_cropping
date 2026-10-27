---
name: scientific-local-git
description: Register verified scientific project changes as scoped local Git commits while preserving unrelated work and excluding scientific outputs.
---

# Scientific Local Git

Respond in English. Inspect repository root, status and diff before editing. Initialize a local repository when absent and authorized by the project contract. At each verified coherent milestone, stage explicit task-owned paths, inspect the staged diff, run relevant checks and automatically commit using an imperative scoped message. Include tests, documentation, skills, runtime configuration and application lockfiles. Ignore outputs, logs, raw local observations, build products and secrets. Do not absorb unrelated edits, change global Git settings, invent author identity, amend, reset, rebase or publish remotely without explicit authorization. If validation or commit fails, preserve work and report the exact blocker. Report the commit identifier and remaining worktree changes. Automatic registration means milestone commits, not a watcher committing half-written files or an application invoking Git during image processing.

When asked to merge branches, inspect local and remote-tracking refs and merge only existing relevant branches; do not create speculative branches or merge unrelated work. When asked to push, require an explicit configured remote and destination branch. If no remote exists, finish local work and request the repository URL rather than guessing. User authorization persists for the requested push, but does not supply a missing remote identity.
