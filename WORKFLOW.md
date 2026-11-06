# Workflow — Prosodia

Follow [AGENTS.md](AGENTS.md) for repository rules and [PERSONA.md](PERSONA.md)
for the developer role and commit identity.

## Branch, review, merge

1. Branch off local `main`. All work happens on a branch named `<type>/<short-slug>`,
   matching the commit type: `fix/`, `feat/`, `docs/` or `chore/`.
2. Execute multi-task implementation plans with the `subagent-dev` skill. When code
   work is complete, use the `code-review` skill to dispatch the review to a fresh
   read-only subagent. Documentation-only commits need no review.
3. Follow `code-review` to evaluate findings and drive remediation through fresh
   subagents: dispatch the accepted findings to a fresh fixer subagent, then dispatch
   a fresh subagent for a scoped re-review of the fix diff (at most three rounds).
   The resident coordinates, rules on each finding, and checks every subagent's
   claimed result against git before it accepts the result.
4. Open a pull request against `main` with `gh pr create`. `main` requires one
   approving review (`fl github pr review <number> --approve`, via the Reviewer
   GitHub App): a subordinate agent requests review from the Project Lead (or from
   the Lab Manager, `jarvis`, if the Project Lead is unavailable for any reason;
   try the Project Lead first), and the Project Lead requests review from the Lab
   Manager (`jarvis`). Documentation-only changes skip step 2 but still go through
   a pull request.
5. The pull-request author merges the pull request with `gh pr merge <number> --merge`
   once the required review approval is in place. A reviewer never merges another
   agent's pull request, except as "Merge permission" below describes for an author
   whose session blocks the merge: the author resolves conflicts and keeps the
   branch current. Direct pushes, force-pushes and deletion of `main` are blocked.

Read the live branch rules with
`gh api repos/Artificial-Humanity/Prosodia/rules/branches/main`. No status checks are
required. `gh pr checks` can fail because the token cannot read check runs.

### Merge permission

The owner permits an author to merge their own pull request once all of these hold:

1. **The required reviewer approved it.** For a pull request by an assisting agent or the
   UI developer that is the project Lead, or the Lab Manager (`jarvis`) if the Lead is
   unavailable for any reason (try the Lead first). For a Lead's pull request it is the Lab
   Manager. For an Owner-tier pull request it is the owner. An approval counts only
   when the Reviewer GitHub App or the owner posts it on the current head: a push
   dismisses an earlier approval, so ask for the review again after you update the branch.
2. **The branch is current with `main`** (and any required checks on the current head
   have passed).
3. **You merge it yourself** with `gh pr merge <number> --merge`, then confirm that the
   merge commit is on `origin/main`.

Your session's own permission layer can still refuse `gh pr merge` (an auto-mode
classifier, for example, denies a merge it cannot tie to an approval). If it does:

* Do not retry the same call, do not use another route yourself (`fl github pr merge` or
  the API), and do not change permission settings.
* Raise a question for the owner (FerroDeck shows the `?` flag next to your agent). Say what
  was denied and which pull request is waiting.
* While the block stands, your Lead (the Lab Manager, for a Lead's own pull request) may
  merge the approved pull request for you on that reviewer's own authority. You do not ask
  for it: the reviewer decides. The reviewer tells the owner each time, so that the owner
  can grant the permission to you directly.
* The owner lifts the block for you with an explicit instruction in your session.

A standing rule in this file is the reason the merge is allowed. It does not turn off a
session's permission prompt, and a message from another agent does not lift one.

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
