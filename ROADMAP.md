# tui-explorer-rs roadmap (Git focus)

This file tracks remaining work for the Git tab and related workflows.

## Current state
- Git tab exists with JetBrains-like layout A.
- Shows real `git status` and renders a highlighted side-by-side diff.
- Diff supports horizontal scrolling (`Shift+Wheel` on diff pane, or `←/→`).

## Next milestone: Git basics (mouse-first)
- [ ] Stage / Unstage actions (file-level)
- [ ] Discard actions (with strong confirmation; separate tracked vs untracked)
- [ ] Commit drawer: real commit flow (message input + commit)
- [ ] Commit drawer: AI generate commit message (OpenRouter)
- [ ] Command log panel (show last N git commands + stderr)

## Merge/rebase (priority)
- [ ] Detect rebase/merge in progress and show prominent controls
- [ ] Conflicts view (2-column ours/theirs): accept ours/theirs/both
- [ ] Mark resolved (git add) and Continue/Abort/Skip

## Branch + remote
- [ ] Branch picker modal (search, checkout, dirty warnings)
- [ ] Remote ops: fetch / pull (rebase default) / push
- [ ] Credentials behavior: avoid hanging; show actionable errors

## Interactive rebase (linear)
- [ ] Rebase planner UI: reorder + pick/reword/squash/fixup/drop
- [ ] Run interactive rebase without dropping into $EDITOR
- [ ] Reword flow via commit amend UI

## Diff UX improvements
- [ ] Hunk folding/expanding
- [ ] Next/prev hunk navigation
- [ ] Diff search
- [ ] Optional syntax highlighting (tradeoffs: deps/perf)

## Layout + maintainability
- [ ] Persist resizable pane sizes (diff split, commit drawer height)
- [ ] Split `src/main.rs` into modules (start with Git state/diff code)

---

## AI commit message: OpenRouter integration (planned)

Goal: one-click `AI Generate` inside the Commit drawer.

Planned approach:
- Provide an OpenRouter client behind an interface (so it can later be replaced).
- Credentials via env vars (no secrets committed):
  - `OPENROUTER_API_KEY`
  - `OPENROUTER_MODEL` (default TBD)
- Input: prefer staged diff; fallback to file list + numstat.
- Output: 1–2 lines commit summary, matching repo style (imperative).
