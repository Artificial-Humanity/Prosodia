# Workflow — Prosodia

Follow [AGENTS.md](AGENTS.md) for repository rules and [PERSONA.md](PERSONA.md)
for the developer role and commit identity.

## Branch, review, merge

1. Branch off local `main`. All work happens on a branch named `<type>/<short-slug>`,
   matching the commit type: `fix/`, `feat/`, `docs/` or `chore/`.
2. When code work is complete, use `superpowers:requesting-code-review` to dispatch
   a review. Documentation-only commits need no review.
3. Use `superpowers:receiving-code-review` to evaluate findings. Address them,
   then commit the fixes.
4. Merge to `main` after the applicable review cycle. A direct push to `main` is
   allowed; a pull request is not required.

## Review scope and conduct

* Review source, build configuration and dependency manifests: `crates/`, `bindings/`,
  `platforms/`, `apps/`, `Cargo.toml`, `Cargo.lock` and build scripts.
* A reviewer reports findings. The reviewing agent makes fixes only when the owner
  explicitly requests them.
* Keep findings in the review itself. File findings that the cycle cannot resolve
  as issues; do not create separate review documents.

## Synchronization

* Run `git pull --rebase` as the first step of a commit-and-push sequence on your
  branch. Rebase on `main` again immediately before merging or pushing directly.
* If the tree contains the owner's uncommitted edits, fetch and check ahead/behind
  instead of forcing a rebase.
