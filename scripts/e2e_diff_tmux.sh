#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="$ROOT/target/e2e"
BIN="$ROOT/target/debug/lzgit"
SESSION="lzgit-e2e-diff-$$"
TMP="$(mktemp -d)"
REPO="$TMP/repo"
HOME_DIR="$TMP/home"
PANE=""

cleanup() {
  if [[ -n "$PANE" ]]; then
    tmux send-keys -t "$PANE" q >/dev/null 2>&1 || true
  fi
  tmux kill-session -t "$SESSION" >/dev/null 2>&1 || true
  rm -rf "$TMP"
}
trap cleanup EXIT

skip() {
  echo "SKIP: $*"
  exit 0
}

need() {
  command -v "$1" >/dev/null 2>&1 || skip "$1 is required"
}

need tmux
need git
need perl

mkdir -p "$OUT_DIR" "$HOME_DIR"

cargo build --bin lzgit >/dev/null

mkdir -p "$REPO/src"
cd "$REPO"
git init -q
git config user.email e2e@example.invalid
git config user.name "lzgit e2e"

cat > aaa_demo.rs <<'RS'
fn main() {
    let color = "red";
    let count = old_value + 1;
    println!("paint {color} {count}");
}
RS
cat > README.md <<'MD'
# Diff Fixture

This file will be renamed.
MD
cat > unicode.txt <<'TXT'
hello 中文 😀
tab	value
TXT
cat > deleted.md <<'MD'
This file is deleted in the fixture commit.
MD

git add .
git commit -q -m "initial fixture"

mkdir -p docs
git mv README.md docs/README.md
cat > docs/README.md <<'MD'
# Diff Fixture

This file was renamed and edited.
MD
cat > aaa_demo.rs <<'RS'
fn main() {
    let color = "green";
    let inserted = "history-only";
    let count = new_value + 1;
    println!("paint {color} {count}");
}
RS
cat > unicode.txt <<'TXT'
hello 中文世界 😀
tab	value changed
TXT
cat > notes.md <<'MD'
# Added file

This is a new file in the history fixture.
MD
rm deleted.md
git add -A
git commit -q -m "comview diff fixture"

cat > aaa_demo.rs <<'RS'
fn main() {
    let color = "blue";
    let inserted = "history-only";
    let count = latest_value + 1;
    println!("paint {color} {count}");
    println!("working tree change");
}
RS
cat > unicode.txt <<'TXT'
hello 中文世界 🚀
tab	value changed again
TXT
cat > untracked.txt <<'TXT'
Untracked file for status tree coverage.
TXT

RUNNER="$TMP/run-lzgit.sh"
cat > "$RUNNER" <<EOF_RUNNER
#!/usr/bin/env bash
cd "$REPO"
export HOME="$HOME_DIR"
export TERM=xterm-256color
exec "$BIN" "$REPO"
EOF_RUNNER
chmod +x "$RUNNER"

tmux new-session -d -s "$SESSION" -n e2e "$RUNNER"
PANE="$SESSION:0.0"
tmux resize-window -t "$SESSION:0" -x 120 -y 36

capture() {
  local name="$1"
  tmux capture-pane -ep -t "$PANE" > "$OUT_DIR/$name.ansi"
  tmux capture-pane -p -t "$PANE" > "$OUT_DIR/$name.txt"
}

wait_for_text() {
  local pattern="$1"
  local label="$2"
  local i
  for i in $(seq 1 80); do
    if tmux capture-pane -p -t "$PANE" | grep -Fq -- "$pattern"; then
      return 0
    fi
    sleep 0.25
  done
  echo "Timed out waiting for $label ($pattern)" >&2
  tmux capture-pane -p -t "$PANE" >&2 || true
  exit 1
}

assert_contains() {
  local file="$1"
  local pattern="$2"
  local label="$3"
  if ! grep -Fq -- "$pattern" "$file"; then
    echo "ASSERT FAIL: $label" >&2
    echo "missing pattern: $pattern" >&2
    echo "file: $file" >&2
    exit 1
  fi
}

assert_regex() {
  local file="$1"
  local pattern="$2"
  local label="$3"
  if ! grep -Eq -- "$pattern" "$file"; then
    echo "ASSERT FAIL: $label" >&2
    echo "missing regex: $pattern" >&2
    echo "file: $file" >&2
    exit 1
  fi
}

assert_not_contains() {
  local file="$1"
  local pattern="$2"
  local label="$3"
  if grep -Fq -- "$pattern" "$file"; then
    echo "ASSERT FAIL: $label" >&2
    echo "unexpected pattern: $pattern" >&2
    echo "file: $file" >&2
    exit 1
  fi
}

assert_ansi() {
  local file="$1"
  local label="$2"
  if ! perl -0ne 'BEGIN { $ok = 0 } $ok = 1 if /\e\[/; END { exit($ok ? 0 : 1) }' "$file"; then
    echo "ASSERT FAIL: $label has no ANSI SGR escapes" >&2
    exit 1
  fi
  if ! perl -0ne 'BEGIN { $ok = 0 } $ok = 1 if /\e\[[0-9;]*48;2;/; END { exit($ok ? 0 : 1) }' "$file"; then
    echo "ASSERT FAIL: $label has no RGB background style" >&2
    exit 1
  fi
}

wait_for_text "latest_value" "Git working diff"
capture git-side-by-side

tmux send-keys -t "$PANE" s
wait_for_text "Diff (Unified)" "Git unified mode"
capture git-unified

tmux send-keys -t "$PANE" 2
wait_for_text "comview diff fixture" "History tab"
tmux send-keys -t "$PANE" f
wait_for_text "README.md" "History files mode"
tmux send-keys -t "$PANE" Down
wait_for_text "green" "History unified diff"
capture history-unified

tmux send-keys -t "$PANE" s
wait_for_text "Old" "History side-by-side title"
capture history-side-by-side

PIPE_CHAR=$'\u2502'

assert_contains "$OUT_DIR/git-side-by-side.txt" "Old" "Git side-by-side old title"
assert_contains "$OUT_DIR/git-side-by-side.txt" "New" "Git side-by-side new title"
assert_contains "$OUT_DIR/git-side-by-side.txt" "Diff (SxS)" "Git side-by-side mode title"
assert_contains "$OUT_DIR/git-side-by-side.txt" "$PIPE_CHAR" "Git side-by-side separator"
assert_contains "$OUT_DIR/git-side-by-side.txt" "green" "Git side-by-side delete code"
assert_contains "$OUT_DIR/git-side-by-side.txt" "blue" "Git side-by-side add code"
assert_contains "$OUT_DIR/git-side-by-side.txt" "latest_value" "Git side-by-side inline changed token"
assert_regex "$OUT_DIR/git-side-by-side.txt" '[0-9]+-' "Git side-by-side delete gutter"
assert_regex "$OUT_DIR/git-side-by-side.txt" '[0-9]+\+' "Git side-by-side add gutter"

assert_contains "$OUT_DIR/git-unified.txt" "@@" "Git unified hunk header"
assert_contains "$OUT_DIR/git-unified.txt" "Diff (Unified)" "Git unified mode title"
assert_not_contains "$OUT_DIR/git-unified.txt" " Old " "Git unified should not show side-by-side title"
assert_contains "$OUT_DIR/git-unified.txt" "green" "Git unified delete code"
assert_contains "$OUT_DIR/git-unified.txt" "blue" "Git unified add code"
assert_regex "$OUT_DIR/git-unified.txt" '-[[:space:]]*let color' "Git unified delete marker"
assert_regex "$OUT_DIR/git-unified.txt" '\+[[:space:]]*let color' "Git unified add marker"

assert_contains "$OUT_DIR/history-unified.txt" "comview diff fixture" "History unified commit subject"
assert_contains "$OUT_DIR/history-unified.txt" "@@" "History unified hunk header"
assert_contains "$OUT_DIR/history-unified.txt" "red" "History unified old token"
assert_contains "$OUT_DIR/history-unified.txt" "green" "History unified new token"

assert_contains "$OUT_DIR/history-side-by-side.txt" "Old" "History side-by-side old title"
assert_contains "$OUT_DIR/history-side-by-side.txt" "New" "History side-by-side new title"
assert_contains "$OUT_DIR/history-side-by-side.txt" "$PIPE_CHAR" "History side-by-side separator"
assert_contains "$OUT_DIR/history-side-by-side.txt" "red" "History side-by-side old token"
assert_contains "$OUT_DIR/history-side-by-side.txt" "reen" "History side-by-side new token"

if cmp -s "$OUT_DIR/git-side-by-side.txt" "$OUT_DIR/git-unified.txt"; then
  echo "ASSERT FAIL: Git side-by-side and unified snapshots are identical" >&2
  exit 1
fi
if cmp -s "$OUT_DIR/history-side-by-side.txt" "$OUT_DIR/history-unified.txt"; then
  echo "ASSERT FAIL: History side-by-side and unified snapshots are identical" >&2
  exit 1
fi

for name in git-side-by-side git-unified history-unified history-side-by-side; do
  assert_ansi "$OUT_DIR/$name.ansi" "$name"
done

if command -v ansi2html >/dev/null 2>&1; then
  for f in "$OUT_DIR"/*.ansi; do
    ansi2html < "$f" > "${f%.ansi}.html"
  done
elif command -v aha >/dev/null 2>&1; then
  for f in "$OUT_DIR"/*.ansi; do
    aha < "$f" > "${f%.ansi}.html"
  done
fi

echo "E2E snapshots written:"
for name in git-side-by-side git-unified history-side-by-side history-unified; do
  echo "  $OUT_DIR/$name.ansi"
  echo "  $OUT_DIR/$name.txt"
  if [[ -f "$OUT_DIR/$name.html" ]]; then
    echo "  $OUT_DIR/$name.html"
  fi
done
