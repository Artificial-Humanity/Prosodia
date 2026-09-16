# Workflow — Prosodia

**This file holds how work gets done in this repo: branching, review, and landing on `main`.**
[AGENTS.md](AGENTS.md) §3 points here. Your identity and remit are [PERSONA.md](PERSONA.md).

---

## Branch, review, merge

**Standing across every project repo in this workspace** (owner, 2026-09-16):

1. **Branch off `main`.** All work happens on a branch.
2. **When the work is complete, call for a review.** Use the `superpowers:requesting-code-review`
   skill to dispatch it.
3. **Receive the review with the `superpowers:receiving-code-review` skill.** Address what it
   finds, then commit the fixes.
4. **Merge to `main`.** Once this review cycle has been followed, a direct push to `main` is
   allowed — a pull request is not required.

⚠ **This supersedes the old PR-only rule and the `claude-review.yml` / `claude-fix.yml` lane**
(owner, 2026-09-16). That lane was already stood down in this repo (#6, 2026-08-10); the
review cycle above replaces it rather than sitting alongside it.

## Branch naming

`<type>/<short-slug>` matching the commit type — `fix/`, `feat/`, `docs/`, `chore/`.

## Pull before push, every time

The Mac and `ai-lab-0` (and their agent sessions) work the same repo concurrently: run
`git pull --rebase` as the first step of any commit-and-push sequence on your branch, and
rebase on `main` again immediately before merging or pushing directly. If the tree holds the
owner's uncommitted local edits, fetch and check ahead/behind instead of forcing a rebase.
