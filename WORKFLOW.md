# Workflow — Prosodia

Follow [AGENTS.md](AGENTS.md) for repository rules and [PERSONA.md](PERSONA.md)
for the developer role and commit identity.

## Branch, review, merge

1. Branch off local `main`. All work happens on a branch named `<type>/<short-slug>`,
   matching the commit type: `fix/`, `feat/`, `docs/` or `chore/`.
2. When code work is complete, use `superpowers:requesting-code-review` to dispatch
   the review to a fresh subagent. Documentation-only commits need no review.
3. Use `superpowers:receiving-code-review` to evaluate findings. Remediation is
   subagent-driven too: dispatch the accepted findings to a fresh implementer
   subagent, then dispatch a fresh subagent for a scoped re-review of the fix diff.
   Repeat until no accepted finding remains open. The resident coordinates, rules on
   each finding, and checks every subagent's claimed result against git before it
   accepts the result.
4. Open a pull request against `main` with `gh pr create`. `main` requires one
   approving review, and the machine account cannot approve its own pull request, so
   the owner approves every one. `.github/workflows/request-admin-review.yml` requests
   the owner's review and assigns the owner when a pull request opens as ready for
   review. Documentation-only changes skip step 2 but still go through a pull request.
5. Merge the pull request after the owner approves it. Direct pushes, force-pushes and
   deletion of `main` are blocked.

Read the live branch rules with
`gh api repos/Artificial-Humanity/Prosodia/rules/branches/main`. No status checks are
required. `gh pr checks` can fail because the token cannot read check runs.

## Review scope and conduct

* Review source, build configuration and dependency manifests: `crates/`, `bindings/`,
  `platforms/`, `apps/`, `Cargo.toml`, `Cargo.lock` and build scripts.
* A reviewer subagent reports findings and does not fix them. Fixes come from a
  separate implementer subagent (step 3).
* Keep findings in the review itself. File findings that the cycle cannot resolve
  as issues; do not create separate review documents.

## Synchronization

* Run `git pull --rebase` as the first step of a commit-and-push sequence on your
  branch. Rebase on `main` again immediately before you open or merge a pull request.
* If the tree contains the owner's uncommitted edits, fetch and check ahead/behind
  instead of forcing a rebase.
