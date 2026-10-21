#!/bin/sh

# SPDX-License-Identifier: MIT OR Apache-2.0

# Mutation coverage over the feature matrix (all/default/no-default features;
# tested with cargo-mutants 27.x): a mutant counts as covered as soon as one
# configuration catches it, so survivors are real coverage gaps. Mutants unviable
# under fewer features are still retested in the next configuration. In diff mode,
# untracked files are absent from git diff and untested.
# Example: contrib/mutants-matrix.sh -p floresta-watch-only
# Run with --help for the modes, outputs and exit codes.

set -u

# Defaults: change here to retune the script.
out_dir=mutants.out        # published results (the previous run rotates to $out_dir.old)
default_base_ref=master    # --diff base when none is given (origin fallbacks apply)

usage() {
    cat <<EOF
Usage:
  contrib/mutants-matrix.sh -p <package> [extra cargo-mutants args...]
  contrib/mutants-matrix.sh --diff [<base-ref>] [extra cargo-mutants args...]
Mutation coverage over all/default/no-default features: a mutant counts as covered when any configuration catches it, and is reported only when missed or timed out with all features and never caught later.
Unviable with all features skips retests; unviable later is still retested in the next configuration.
Exactly one mode: -p <package> runs the whole package; --diff [ref] tests only mutants overlapping the diff vs the merge base with <ref> (default "$default_base_ref", falling back to "origin/$default_base_ref" or origin/HEAD); CI checkouts need fetch-depth: 0.
Extra args forward to every cargo-mutants invocation; --jobs/--output/--iterate/--gitignore/--in-diff and the feature flags are managed by this script and rejected (--list is unsupported: run 'cargo mutants --list' directly).
Results go to $out_dir/{missed.txt,timeout.txt,<n>-<config>/} (previous run rotates to $out_dir.old).
Exit codes: 0 clean or nothing to test, 1 missed/timeout survivors, 2 usage error, missing prerequisite, no mutants found, or a configuration failed to run.
EOF
}

usage_error() {
    echo "error: $1" >&2
    usage >&2
    exit 2
}

# Prints the contents of the given files, skipping the ones that do not exist.
lines() {
    for file in "$@"; do
        [ -f "$file" ] && cat "$file"
    done
}

# Runs one matrix configuration ($1 output dir, $2 log label, rest cargo-mutants args). Later configs skip
# $work/exclude.txt via a seeded previously_caught.txt, so mutants unviable under fewer features are still
# retested later. Accept 0/2/3 only with missed.txt present; anything else is infrastructure failure.
run_config() {
    name="$1"; label="$2"; shift 2
    printf '\n==> %s\n' "$label"
    rm -rf "${work:?}/$out_dir" "${work:?}/$out_dir.old" || exit 2
    if [ -f "$work/exclude.txt" ]; then
        mkdir "$work/$out_dir" || exit 2
        cp "$work/exclude.txt" "$work/$out_dir/previously_caught.txt" || exit 2
    fi
    cargo mutants --gitignore true --output "$work" "$@"
    status=$?
    dir="$work/results/$name"
    if [ -d "$work/$out_dir" ]; then
        mv "$work/$out_dir" "$dir" || exit 2
    fi
    case "$status" in
        0|2|3)
            if [ -f "$dir/missed.txt" ]; then
                [ "$status" -eq 0 ] || echo "note: configuration '$label' finished with exit $status (mutants missed or timed out)"
                if [ -f "$work/exclude.txt" ]; then
                    lines "$work/exclude.txt" "$dir/caught.txt"
                else
                    lines "$dir/caught.txt" "$dir/unviable.txt"
                fi | LC_ALL=C sort -u >"$work/exclude.next" || exit 2
                mv "$work/exclude.next" "$work/exclude.txt" || exit 2
                return 0
            fi
            ;;
    esac
    echo "error: cargo-mutants failed for configuration '$label' (exit $status)" >&2
    return 1
}

# Moves $work/results to $out_dir, keeping the previous results as $out_dir.old.
publish_results() {
    rm -rf "$out_dir.old" || exit 2
    if [ -e "$out_dir" ]; then
        mv "$out_dir" "$out_dir.old" || exit 2
    fi
    mv "$work/results" "$out_dir" || exit 2
}

# Publishes the outputs of the configurations that ran and stops the matrix.
abort_matrix() {
    publish_results
    echo "error: the matrix stopped because a configuration failed to run; see the log above and $out_dir/" >&2
    exit 2
}

# True when some listed mutant is not in the exclusion list yet.
remaining() {
    LC_ALL=C comm -23 "$work/list-sorted.txt" "$work/exclude.txt" | grep -q .
}

# Reports the survivors of one class ($1 label, $2 file), flagging problems.
report() {
    [ -s "$2" ] || return 0; echo "$1 mutants:"; cat "$2"; problems=1
}

# Publishes the finished configurations when interrupted, if there are any.
# shellcheck disable=SC2329 # invoked via the INT trap below
on_int() {
    [ -n "$(ls "$work/results" 2>/dev/null)" ] && publish_results
    exit 130
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
# The script manages some cargo-mutants options itself; feature flags would fight with the matrix below.
for arg in "$@"; do
    case "$arg" in
        --diff|--diff=*) usage_error "'$arg' can only be given once, as the first argument" ;;
        --jobs*|-j*|--output*|-o*|--iterate|--gitignore*|--in-diff*|-D*|--list*|--in-place|--features*|--all-features|--no-default-features)
            usage_error "'$arg' is managed by this script" ;;
    esac
done
command -v cargo >/dev/null 2>&1 || { echo "error: cargo not found in PATH" >&2; exit 2; }
cargo mutants --version >/dev/null 2>&1 || { echo "error: cargo-mutants 27.x is required; install it with 'cargo install cargo-mutants'" >&2; exit 2; }
# Work from the repo root: results publish there and git needs the repository context.
repo_root="$(git rev-parse --show-toplevel 2>/dev/null)" || { echo "error: not inside a git repository" >&2; exit 2; }
cd "$repo_root" || exit 2

# Scratch space for the diff, the exclusion list and the outputs until published.
work="$(mktemp -d "${TMPDIR:-/tmp}/mutants-matrix.XXXXXX")" || exit 2
trap 'rm -rf "$work"' EXIT
trap 'exit 143' TERM
trap 'on_int' INT
mkdir "$work/results" || exit 2

if [ "$mode" = "diff" ]; then
    if ! git rev-parse --verify --quiet "$base" >/dev/null 2>&1; then
        fallback=""
        if [ "$base" = "$default_base_ref" ]; then
            if git rev-parse --verify --quiet "origin/$default_base_ref" >/dev/null 2>&1; then
                fallback="origin/$default_base_ref"
            else
                fallback="$(git symbolic-ref --quiet --short refs/remotes/origin/HEAD 2>/dev/null || true)"
            fi
        fi
        [ -n "$fallback" ] || { echo "error: base ref '$base' not found" >&2; exit 2; }
        base="$fallback"
    fi
    base_commit="$(git merge-base "$base" HEAD)" || { echo "error: cannot find a merge base between '$base' and HEAD" >&2; exit 2; }
    diff_file="$work/diff.patch"
    git diff "$base_commit" >"$diff_file" || exit 2
    [ -s "$diff_file" ] || { echo "Nothing to test: the diff between '$base' and HEAD is empty."; exit 0; }
    set -- --in-diff "$diff_file" "$@"
else
    set -- -p "$package" "$@"
fi

# cargo-mutants only warns about an unknown package, so an empty list signals a mistyped name (diff mode: only tests or non-Rust files changed).
cargo mutants --list --all-features "$@" >"$work/list.txt" || { echo "error: cargo-mutants failed to list the mutants" >&2; exit 2; }
LC_ALL=C sort -u "$work/list.txt" >"$work/list-sorted.txt" || exit 2
count="$(grep -c . "$work/list-sorted.txt" || true)"
if [ "$count" -eq 0 ]; then
    if [ "$mode" = "package" ]; then
        echo "error: no mutants found in package '$package'; check the package name and any filters" >&2
        exit 2
    fi
    echo "Nothing to test: no mutants in the diff between '$base' and HEAD (only tests or non-Rust files changed?)."
    exit 0
fi
if [ "$mode" = "package" ]; then
    echo "Testing $count mutants of package '$package'"
else
    echo "Testing $count mutants in the diff between '$base' and HEAD"
fi

run_config 1-all-features "all features" --all-features "$@" || abort_matrix
if remaining; then
    run_config 2-default-features "default features" --iterate "$@" || abort_matrix
else
    echo "Skipping the default-features configuration: every mutant was already caught or is unviable"
fi
if remaining; then
    run_config 3-no-default-features "no default features" --no-default-features --iterate "$@" || abort_matrix
else
    echo "Skipping the no-default-features configuration: every mutant was already caught or is unviable"
fi

# A mutant survives when missed/timed out with all features and never caught later; missed-any survivors are coverage gaps.
results="$work/results"
lines "$results/1-all-features/missed.txt" "$results/1-all-features/timeout.txt" |
    LC_ALL=C sort -u >"$work/candidates.txt" || exit 2
lines "$results/2-default-features/caught.txt" "$results/3-no-default-features/caught.txt" |
    LC_ALL=C sort -u >"$work/caught-later.txt" || exit 2
LC_ALL=C comm -23 "$work/candidates.txt" "$work/caught-later.txt" >"$work/survivors.txt" || exit 2
lines "$results"/*/missed.txt | LC_ALL=C sort -u >"$work/missed-any.txt" || exit 2
LC_ALL=C comm -12 "$work/survivors.txt" "$work/missed-any.txt" >"$results/missed.txt" || exit 2
LC_ALL=C comm -23 "$work/survivors.txt" "$work/missed-any.txt" >"$results/timeout.txt" || exit 2
publish_results

echo
problems=0
report Missed "$out_dir/missed.txt"
report Timeout "$out_dir/timeout.txt"
[ "$problems" -ne 0 ] && exit 1
echo "No mutants were missed or timed out in any configuration."
exit 0
