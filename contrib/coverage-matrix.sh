#!/bin/sh

# SPDX-License-Identifier: MIT OR Apache-2.0

# Union coverage over the feature matrix (all/default/no-default features): a
# line counts as covered when any configuration covers it. Use it to catch
# lines only tested under some feature set, or to gate a change on its new lines.
# #[cfg(test)] lines count toward the totals; IO `?` paths make 100% regions
# unreachable without fault injection; crates not building with
# --no-default-features make workspace mode best-effort, prefer -p.
# Example: contrib/coverage-matrix.sh -p floresta-watch-only --fail-under 95
# Run with --help for the modes, outputs and exit codes.

set -u

# Defaults: change here to retune the script.
out_dir=coverage.out         # published results (the previous run rotates to $out_dir.old)
default_base_ref=master      # --diff base when none is given (origin fallbacks apply)
default_fail_under_diff=100  # minimum coverage of the added lines in --diff mode

usage() {
    cat <<EOF
Usage:
  contrib/coverage-matrix.sh -p <package> [--file <path>]... [--fail-under N] [extra cargo-llvm-cov args...]
  contrib/coverage-matrix.sh --diff [<base-ref>] [--file <path>]... [--fail-under N] [extra cargo-llvm-cov args...]
Union coverage over all/default/no-default features. Exactly one mode:
  -p <package>  whole package
  --diff [ref]  only lines added vs merge base with <ref> (default "$default_base_ref"/origin fallbacks); no added Rust lines exits 0 without running
--file <path> (repeatable) restricts the report and --fail-under to files (or directories ending in '/'); test suites still run in full.
Minimum coverage: --fail-under N (integer 0-100) exits 1 below N percent, 0 at N or more. In -p mode it measures the package's lines; in --diff mode the
added lines (default $default_fail_under_diff there; -p has no default). Example: -p floresta-watch-only --fail-under 95 fails under 95%.
Extra args forward to cargo-llvm-cov (runs and reports); feature and report-format flags are managed by this script and rejected. Output goes to
$out_dir/{summary.txt,lcov.info,missed.txt} (previous run rotates to $out_dir.old). Exit codes: 0 ok, 1 below threshold, 2 usage/run error.
EOF
}

usage_error() {
    echo "error: $1" >&2
    usage >&2
    exit 2
}

# Validates and records the --fail-under threshold.
set_fail_under() {
    case "$1" in
        [0-9]|[0-9][0-9]|100) fail_under="$1" ;;
        *) usage_error "'--fail-under' requires an integer between 0 and 100, got '$1'" ;;
    esac
}

# Runs one matrix configuration ($1 labels the log); a failure publishes the partials and stops the matrix.
run_config() {
    label="$1"; shift
    printf '\n==> %s\n' "$label"
    cargo llvm-cov --no-report "$@" && return 0
    status=$?
    [ -n "$(ls "$work/results" 2>/dev/null)" ] && publish_results
    echo "error: cargo llvm-cov failed for configuration '$label' (exit $status); partial results are in $out_dir/" >&2
    exit 2
}

# Moves $work/results to $out_dir, keeping the previous results as $out_dir.old.
publish_results() {
    rm -rf "$out_dir.old" || exit 2
    if [ -e "$out_dir" ]; then
        mv "$out_dir" "$out_dir.old" || exit 2
    fi
    mv "$work/results" "$out_dir" || exit 2
}

# Publishes the partial results and stops: a union report failed to generate.
abort_report() {
    publish_results
    echo "error: cargo llvm-cov failed to generate the $1 report; partial results are in $out_dir/" >&2
    exit 2
}

# Publishes the finished reports when interrupted, if there are any.
# shellcheck disable=SC2329 # invoked via the INT trap below
on_int() {
    [ -n "$(ls "$work/results" 2>/dev/null)" ] && publish_results
    exit 130
}

# Writes to $3 the path:line entries of $2 under scope list $1 (equal match, or '/' entries prefix-match).
filter_by_files() {
    awk '
    NR == FNR { entries[++n] = $0; next }
    { path = $0; sub(/:[0-9]+$/, "", path)
        for (i = 1; i <= n; i++) { entry = entries[i]
            if (path == entry || (substr(entry, length(entry)) == "/" && index(path, entry) == 1)) { print; next } } }' "$1" "$2" >"$3" || exit 2
}

# Notes on stderr the scope entries ($work/files.txt) matching no line of the set $1.
note_zero_entries() {
    while IFS= read -r entry; do
        [ -n "$entry" ] || continue
        printf '%s\n' "$entry" >"$work/one-entry.txt" || exit 2
        filter_by_files "$work/one-entry.txt" "$1" "$work/one-match.txt"
        [ -s "$work/one-match.txt" ] || echo "note: no executable lines found for $entry" >&2
    done <"$work/files.txt" || exit 2
}

[ "$#" -gt 0 ] || { usage >&2; exit 2; }
mode=""; package=""; base=""
case "$1" in
    -h|--help) usage; exit 0 ;;
    -p)
        if [ "$#" -lt 2 ] || [ -z "$2" ]; then usage_error "-p requires a package name"; fi
        mode="package"; package="$2"; shift 2 ;;
    --diff)
        mode="diff"; shift
        # Base ref is optional: consume $1 only when it is not an option.
        if [ "$#" -gt 0 ] && [ -n "$1" ] && [ "${1#-}" = "$1" ]; then base="$1"; shift; else base="$default_base_ref"; fi ;;
    *) usage_error "expected -p <package> or --diff [<base-ref>]" ;;
esac

# Scratch space for the diff, the line sets and the reports until published.
work="$(mktemp -d "${TMPDIR:-/tmp}/coverage-matrix.XXXXXX")" || exit 2
trap 'rm -rf "$work"' EXIT
trap 'exit 143' TERM
trap 'on_int' INT
mkdir "$work/results" || exit 2
# Strip this script's --fail-under/--file options before forwarding the rest to cargo-llvm-cov.
fail_under=""
: >"$work/files-raw.txt" || exit 2
total=$#; i=0
while [ "$i" -lt "$total" ]; do
    case "$1" in
        --fail-under)
            shift; i=$((i + 1)); [ "$i" -lt "$total" ] || usage_error "'--fail-under' requires an integer between 0 and 100"
            set_fail_under "$1"; shift; i=$((i + 1)) ;;
        --fail-under=*)
            set_fail_under "${1#*=}"; shift; i=$((i + 1)) ;;
        --file)
            shift; i=$((i + 1)); [ "$i" -lt "$total" ] || usage_error "'--file' requires a path"
            [ -n "$1" ] || usage_error "'--file' requires a path"
            printf '%s\n' "$1" >>"$work/files-raw.txt" || exit 2; shift; i=$((i + 1)) ;;
        --file=*)
            [ -n "${1#*=}" ] || usage_error "'--file' requires a path"
            printf '%s\n' "${1#*=}" >>"$work/files-raw.txt" || exit 2; shift; i=$((i + 1)) ;;
        *) set -- "$@" "$1"; shift; i=$((i + 1)) ;;
    esac
done
# The script manages some cargo-llvm-cov options itself; feature flags would fight with the matrix below.
for arg in "$@"; do
    case "$arg" in
        --diff|--diff=*) usage_error "'$arg' can only be given once, as the first argument" ;;
        --) usage_error "'--' is not supported: no test-harness passthrough in v1" ;;
        --fail-under-lines*|--fail-under-regions*|--fail-under-diff*) usage_error "'$arg' was replaced by '--fail-under N'" ;;
        --features*|--all-features|--no-default-features|--no-report|--no-clean|--clean|--target-dir*|--json|--lcov|--cobertura|--codecov|--text|--html|--output-path*|--output-dir*|--summary-only|--show-missing-lines|--fail-under-functions*) usage_error "'$arg' is managed by this script" ;;
    esac
done
command -v cargo >/dev/null 2>&1 || { echo "error: cargo not found in PATH" >&2; exit 2; }
cargo llvm-cov --version >/dev/null 2>&1 || { echo "error: cargo-llvm-cov 0.8.x is required; install it with 'cargo install cargo-llvm-cov'" >&2; exit 2; }
sysroot="$(rustc --print sysroot 2>/dev/null || true)"
llvm_cov=""
for candidate in "$sysroot"/lib/rustlib/*/bin/llvm-cov; do [ -x "$candidate" ] && { llvm_cov="$candidate"; break; }; done
[ -n "$llvm_cov" ] || { echo "error: llvm-cov not found in the rustc sysroot; install it with 'rustup component add llvm-tools-preview'" >&2; exit 2; }
# Work from the repo root: results publish there and git needs the repository context.
repo_root="$(git rev-parse --show-toplevel 2>/dev/null)" || { echo "error: not inside a git repository" >&2; exit 2; }
cd "$repo_root" || exit 2
# Normalize scope entries against the worktree now that the repo root is the working directory.
has_files=0
if [ -s "$work/files-raw.txt" ]; then
    has_files=1
    : >"$work/files.txt" || exit 2
    while IFS= read -r entry; do
        [ -n "$entry" ] || continue
        while [ "${entry#./}" != "$entry" ]; do entry="${entry#./}"; done
        case "$entry" in
            "$PWD"/*) entry="${entry#"$PWD"/}" ;;
        esac
        [ -e "$entry" ] || usage_error "--file '$entry' does not exist"
        printf '%s\n' "$entry" >>"$work/files.txt" || exit 2
    done <"$work/files-raw.txt" || exit 2
fi

if [ "$mode" = "diff" ]; then
    if ! git rev-parse --verify --quiet "$base" >/dev/null 2>&1; then
        fallback=""
        if [ "$base" = "$default_base_ref" ]; then
            fallback="$(git symbolic-ref --quiet --short refs/remotes/origin/HEAD 2>/dev/null || true)"
            git rev-parse --verify --quiet "origin/$default_base_ref" >/dev/null 2>&1 && fallback="origin/$default_base_ref"
        fi
        [ -n "$fallback" ] || { echo "error: base ref '$base' not found" >&2; exit 2; }
        base="$fallback"
    fi
    merge_base="$(git merge-base "$base" HEAD)" || { echo "error: cannot find a merge base between '$base' and HEAD (CI checkouts need fetch-depth: 0)" >&2; exit 2; }
    # The set of added lines, as sorted path:line entries.
    git diff -U0 --no-color "$merge_base" -- '*.rs' >"$work/diff.patch" || exit 2
    awk '
/^\+\+\+ / { file = substr($0, 5); if (file == "/dev/null") file = ""; else if (index(file, "b/") == 1) file = substr(file, 3) }
/^@@ / { if (file == "") next; rest = substr($0, index($0, "+") + 1)
    comma = index(rest, ","); space = index(rest, " "); start = rest + 0; count = 1
    if (comma > 0 && comma < space) { start = substr(rest, 1, comma - 1) + 0; count = substr(rest, comma + 1, space - comma - 1) + 0 }
    for (i = 0; i < count; i++) print file ":" (start + i) }' "$work/diff.patch" | LC_ALL=C sort -u >"$work/added.txt" || exit 2
    if [ "$has_files" -eq 1 ]; then
        filter_by_files "$work/files.txt" "$work/added.txt" "$work/added-scoped.txt"
        mv "$work/added-scoped.txt" "$work/added.txt" || exit 2
    fi
    if [ ! -s "$work/added.txt" ]; then echo "Nothing to cover: no Rust lines added in the diff between '$base' and HEAD."; exit 0; fi
    [ -n "$fail_under" ] || fail_under=$default_fail_under_diff
else
    set -- -p "$package" "$@"
fi

# Start from a clean slate: stale profiles would silently inflate the union.
cargo llvm-cov clean || { echo "error: cargo llvm-cov clean failed" >&2; exit 2; }
run_config "all features" --all-features "$@"
run_config "default features" "$@"
run_config "no default features" --no-default-features "$@"

# A single union report over the accumulated profiles of all configurations.
results="$work/results"
cargo llvm-cov report --summary-only "$@" >"$results/summary.txt" || abort_report summary
cargo llvm-cov report --lcov --output-path "$results/lcov.info" "$@" || abort_report lcov
# Line sets from the union lcov (every DA record, and zero-hit ones) as sorted repo-relative path:line entries.
: >"$work/all-lines.unsorted" || exit 2
: >"$work/uncovered.unsorted" || exit 2
awk -v root="$PWD/" -v all_file="$work/all-lines.unsorted" -v unc_file="$work/uncovered.unsorted" '
/^SF:/ { file = substr($0, 4); if (index(file, root) == 1) file = substr(file, length(root) + 1) }
/^DA:/ { rest = substr($0, 4); comma = index(rest, ","); key = file ":" substr(rest, 1, comma - 1)
    all_lines[key] = 1; if (substr(rest, comma + 1) + 0 > 0) covered[key] = 1 }
END { for (key in all_lines) { print key > all_file; if (!(key in covered)) print key > unc_file } }' "$results/lcov.info" || exit 2
LC_ALL=C sort -u "$work/all-lines.unsorted" >"$work/all-lines.txt" || exit 2
LC_ALL=C sort -u "$work/uncovered.unsorted" >"$work/uncovered.txt" || exit 2

if [ "$mode" = "package" ]; then
    if [ "$has_files" -eq 1 ]; then
        filter_by_files "$work/files.txt" "$work/uncovered.txt" "$results/missed.txt"
        filter_by_files "$work/files.txt" "$work/all-lines.txt" "$work/scoped-all.txt"
        note_zero_entries "$work/scoped-all.txt"
        scoped_total="$(grep -c . "$work/scoped-all.txt" || true)"
        scoped_miss="$(grep -c . "$results/missed.txt" || true)"
        publish_results
        if [ "$scoped_total" -eq 0 ]; then echo "error: no executable lines match the requested --file paths; see $out_dir/" >&2; exit 2; fi
        scoped_covered=$((scoped_total - scoped_miss))
        awk '
        NR == FNR { path = $0; sub(/:[0-9]+$/, "", path); total[path]++; next }
        { path = $0; sub(/:[0-9]+$/, "", path); uncovered[path]++ }
        END { for (path in total) { c = total[path] - uncovered[path]; printf "%s: %d/%d lines (%.1f%%)\n", path, c, total[path], 100 * c / total[path] } }' \
            "$work/scoped-all.txt" "$out_dir/missed.txt" | LC_ALL=C sort || exit 2
        pct="$(awk -v c="$scoped_covered" -v d="$scoped_total" 'BEGIN { printf "%.1f", 100 * c / d }')" || exit 2
        echo "File coverage: $scoped_covered/$scoped_total lines ($pct%)"
        if [ -n "$fail_under" ] && [ "$((scoped_covered * 100))" -lt "$((fail_under * scoped_total))" ]; then
            echo "error: file coverage $pct% is below the required $fail_under%; see $out_dir/" >&2
            exit 1
        fi
        exit 0
    fi
    cp "$work/uncovered.txt" "$results/missed.txt" || exit 2
    threshold_failed=0
    if [ -n "$fail_under" ]; then
        cargo llvm-cov report --summary-only --fail-under-lines "$fail_under" "$@" >/dev/null || threshold_failed=1
    fi
    publish_results
    cat "$out_dir/summary.txt" || exit 2
    if [ "$threshold_failed" -ne 0 ]; then
        echo "error: coverage is below the requested threshold; see $out_dir/" >&2
        exit 1
    fi
    exit 0
fi

# Newly uncovered lines: added lines that are executable but have zero hits.
LC_ALL=C comm -12 "$work/added.txt" "$work/all-lines.txt" >"$work/executable-added.txt" || exit 2
LC_ALL=C comm -12 "$work/executable-added.txt" "$work/uncovered.txt" >"$results/missed.txt" || exit 2
denom="$(grep -c . "$work/executable-added.txt" || true)"
miss="$(grep -c . "$results/missed.txt" || true)"
if [ "$has_files" -eq 1 ]; then note_zero_entries "$work/executable-added.txt"; fi
publish_results
if [ "$denom" -eq 0 ]; then echo "No executable lines in the diff between '$base' and HEAD."; exit 0; fi
covered=$((denom - miss))
pct="$(awk -v c="$covered" -v d="$denom" 'BEGIN { printf "%.1f", 100 * c / d }')" || exit 2
echo "Diff coverage: $covered/$denom lines ($pct%)"
[ "$miss" -eq 0 ] || { echo "Newly uncovered lines:"; cat "$out_dir/missed.txt" || exit 2; }
if [ "$((covered * 100))" -lt "$((fail_under * denom))" ]; then
    echo "error: diff coverage $pct% is below the required $fail_under%; see $out_dir/" >&2
    exit 1
fi
exit 0
