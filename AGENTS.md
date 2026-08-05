# Project instructions

## Git completion policy

- For implementation tasks, do not report "done" or "hotovo" until the completed change has been verified and committed locally.
- Before staging, inspect `git status` and the relevant diffs. This workspace may be shared by multiple Codex tasks.
- Stage only files or hunks that belong to the current task. Do not use `git add -A` unless the entire working tree has been verified to belong to that task.
- A small completed part requires at least a local commit. Push larger coherent features or milestones to `origin/main` after successful verification; also push whenever the user explicitly requests it.
- Do not create time-based Git autosave, auto-commit, or auto-push automations.
- If unrelated or overlapping changes make a safe commit impossible, do not claim completion; explain the blocker instead.
- In the final response, include the commit hash and whether it was pushed.

## Agent skills

### Issue tracker

Issues and PRDs are tracked in GitHub Issues for `plachow/photosite`. See `docs/agents/issue-tracker.md`.

### Triage labels

The standard triage labels are used without renaming. See `docs/agents/triage-labels.md`.

### Domain docs

This is a single-context repository. See `docs/agents/domain.md`.
