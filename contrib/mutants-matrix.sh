#!/bin/sh

# SPDX-License-Identifier: MIT OR Apache-2.0

# Run cargo-mutants over the crate's feature matrix, so that mutants are
# tested under every meaningful configuration. Runs are chained with
# --iterate: a mutant that is caught (or unviable) in one configuration is
# skipped in the next ones. The final mutants.out/missed.txt therefore lists
# only the mutants missed in EVERY configuration, i.e. the real coverage gaps.
# Mutants that make the tests time out are likewise retried in every
# configuration and reported from the final mutants.out/timeout.txt.
#
# Two modes (exactly one is required):
#
#   package mode (-p <package>):
#     Full matrix over a single package, with the all-features configuration
#     first so that the maximum number of mutants is caught before the more
#     restricted configurations run.
#
#   diff mode (--diff [<base-ref>]):
#     Test only mutants that overlap the diff between <base-ref> and the
#     current tree. If <base-ref> is omitted, the project's main branch
#     ("master", falling back to "origin/master" or to the branch recorded in
#     refs/remotes/origin/HEAD) is used. This is meant for fast feedback in CI
#     on pull requests and for keeping modules that already have a 100%
#     mutation score protected against regressions.
#     An existing mutants.out is archived before the first run, because
#     --iterate reads caught.txt/unviable.txt from a previous run and stale
#     results would skip mutants that are no longer covered.
#     In CI, the base ref and the merge base require a full clone: with
#     actions/checkout set `fetch-depth: 0`, otherwise these git commands fail
#     on a shallow checkout.
#
# Caveats (from the cargo-mutants manual):
#   - --in-diff only matches the code under test, not test code itself, so a
#     diff that only changes tests runs no mutants.
#   - git diff does not include untracked files, so a new Rust file that was
#     never `git add`ed is not tested in diff mode.
#   - Incremental testing is helpful for faster feedback, but it is not a
#     substitute for a full run: run package mode periodically.
#
# Environment:
#   MUTANTS_JOBS      Jobs passed to cargo-mutants (default: 3).
#   MUTANTS_KEEP_OUT  When set to 1, keep an existing mutants.out instead of
#                     archiving it (default: archive with a timestamp). Since
#                     --iterate reads caught/unviable lists from that directory,
#                     stale results can skip mutants that are no longer
#                     covered; prefer the default.
#
# Exit codes:
#   0  no mutant survived every configuration
#   1  missed mutants and/or timeouts survived every configuration
#   2  usage error, missing prerequisite, or a configuration failed to run
#
# Usage:
#   contrib/mutants-matrix.sh -p <package> [extra cargo-mutants args...]
#   contrib/mutants-matrix.sh --diff [<base-ref>] [extra cargo-mutants args...]
#
# The extra arguments are forwarded to every cargo-mutants invocation. In diff
# mode they can for example narrow the scope with "-p <package>"; by default
# the workspace default-members are examined (all crates and binaries; `fuzz`
# is excluded by `default-members` in the root Cargo.toml). Pass "--workspace"
# to examine every workspace member.

set -u

usage() {
    cat >&2 <<'EOF'
Usage:
  contrib/mutants-matrix.sh -p <package> [extra cargo-mutants args...]
  contrib/mutants-matrix.sh --diff [<base-ref>] [extra cargo-mutants args...]

Modes (exactly one is required):
  -p <package>   Test the full feature matrix of a single package.
  --diff [ref]   Test only mutants overlapping the diff between <ref> and the
                 current tree. <ref> defaults to the project's main branch,
                 "master" (or "origin/master"/origin/HEAD when there is no
                 local master).

Environment:
  MUTANTS_JOBS      Jobs passed to cargo-mutants (default: 3).
  MUTANTS_KEEP_OUT  Set to 1 to keep an existing mutants.out instead of
                    archiving it.

Exit codes:
  0  no mutant survived every configuration
  1  missed mutants and/or timeouts survived every configuration
  2  usage error, missing prerequisite, or a configuration failed to run
EOF
}

# Archives any existing cargo-mutants output, so that --iterate starts from a
# clean slate for the current matrix.
archive_previous_output() {
    if [ ! -d mutants.out ]; then
        return 0
    fi

    if [ "${MUTANTS_KEEP_OUT:-0}" = "1" ]; then
        echo "Keeping existing mutants.out (MUTANTS_KEEP_OUT=1)"
        return 0
    fi

    stamp="$(date +%Y%m%d%H%M%S)"
    archive="mutants.out.$stamp.$$"
    echo "Archiving existing mutants.out to $archive"
    mv mutants.out "$archive" || exit 2
}

# Runs one configuration of the feature matrix.
#
# cargo-mutants uses distinct exit codes (see src/exit_code.rs):
#   0   all mutants caught
#   2   missed mutants found
#   3   one or more mutants timed out
#   4   the tests already fail on the unmutated tree
#   5/6 the --in-diff file is invalid or does not match the tree
#   70  internal error
#
# 0, 2 and 3 are results of the mutation run. Any other code, or a run that
# did not even create mutants.out, is an infrastructure failure: continuing
# would silently turn a broken suite or a bad invocation into a green run.
run_config() {
    label="$1"
    shift

    printf '\n==> %s\n' "$label"

    # Remove the previous missed list: a run that reaches the testing phase
    # always writes a new one (possibly empty), so its absence means the
    # invocation failed before producing results.
    rm -f mutants.out/missed.txt

    cargo mutants --jobs "$jobs" --gitignore true "$@"
    status=$?

    case "$status" in
        0)
            return 0
            ;;
        2|3)
            if [ -f mutants.out/missed.txt ]; then
                echo "note: configuration '$label' finished with exit $status (mutants missed or timed out)"
                return 0
            fi
            ;;
    esac

    echo "error: cargo-mutants failed for configuration '$label' (exit $status)" >&2
    hard_failure=1
    return 1
}

if [ "$#" -eq 0 ]; then
    usage
    exit 2
fi

mode=""
package=""
base=""

case "$1" in
    -p)
        if [ "$#" -lt 2 ] || [ -z "$2" ]; then
            echo "error: -p requires a package name" >&2
            usage
            exit 2
        fi
        mode="package"
        package="$2"
        shift 2
        ;;
    --diff)
        mode="diff"
        shift
        # The base ref is optional. Only consume the next argument when it does
        # not look like an option, so extra cargo-mutants flags still work.
        if [ "$#" -gt 0 ] && [ -n "$1" ] && [ "${1#-}" = "$1" ]; then
            base="$1"
            shift
        else
            base="master"
        fi
        ;;
    *)
        echo "error: expected -p <package> or --diff [<base-ref>]" >&2
        usage
        exit 2
        ;;
esac

# Validate the forwarded arguments: the mode flags are mutually exclusive and
# feature flags would fight with the matrix assembled below.
for arg in "$@"; do
    case "$arg" in
        --features|--features=*|--all-features|--no-default-features)
            echo "warning: '$arg' is forwarded to every configuration and overrides the feature matrix" >&2
            ;;
    esac
done

if [ "$mode" = "package" ]; then
    for arg in "$@"; do
        case "$arg" in
            --diff|--diff=*)
                echo "error: -p and --diff are mutually exclusive" >&2
                usage
                exit 2
                ;;
            --in-diff|--in-diff=*)
                echo "error: use --diff <base-ref> rather than --in-diff in package mode" >&2
                usage
                exit 2
                ;;
        esac
    done
fi

command -v cargo >/dev/null 2>&1 || {
    echo "error: cargo not found in PATH" >&2
    exit 2
}
cargo mutants --version >/dev/null 2>&1 || {
    echo "error: cargo-mutants is not available; install it with 'cargo install cargo-mutants'" >&2
    exit 2
}

jobs="${MUTANTS_JOBS:-3}"
hard_failure=0

# Work from the repository root: mutants.out is created in the current
# directory and the git commands below need the repository context.
repo_root="$(git rev-parse --show-toplevel 2>/dev/null)" || {
    echo "error: not inside a git repository" >&2
    exit 2
}
cd "$repo_root" || exit 2

diff_file=""
if [ "$mode" = "diff" ]; then
    if ! git rev-parse --verify --quiet "$base" >/dev/null 2>&1; then
        fallback=""
        if [ "$base" = "master" ]; then
            if git rev-parse --verify --quiet origin/master >/dev/null 2>&1; then
                fallback="origin/master"
            else
                fallback="$(git symbolic-ref --quiet --short refs/remotes/origin/HEAD 2>/dev/null || true)"
            fi
        fi
        if [ -z "$fallback" ]; then
            echo "error: base ref '$base' not found" >&2
            exit 2
        fi
        base="$fallback"
    fi

    base_commit="$(git merge-base "$base" HEAD)" || {
        echo "error: cannot find a merge base between '$base' and HEAD" >&2
        exit 2
    }

    diff_file="$(mktemp "${TMPDIR:-/tmp}/mutants-diff.XXXXXX")" || exit 2
    trap 'rm -f "$diff_file"' EXIT
    trap 'rm -f "$diff_file"; exit 130' INT TERM

    git diff "$base_commit" >"$diff_file" || exit 2

    if [ ! -s "$diff_file" ]; then
        echo "Nothing to test: the diff between '$base' and HEAD is empty."
        exit 0
    fi

    echo "Testing mutants in the diff between '$base' and HEAD"
fi

archive_previous_output

if [ "$mode" = "package" ]; then
    run_config "all features" -p "$package" --all-features "$@"
    run_config "default features" -p "$package" --iterate "$@"
    run_config "no default features" -p "$package" --no-default-features --iterate "$@"
else
    run_config "all features" --in-diff "$diff_file" --all-features "$@"
    run_config "default features" --in-diff "$diff_file" --iterate "$@"
    run_config "no default features" --in-diff "$diff_file" --no-default-features --iterate "$@"
fi

if [ "$hard_failure" -ne 0 ]; then
    echo "error: one or more configurations failed to run; see the log above" >&2
    exit 2
fi

echo
problems=0
if [ -s mutants.out/missed.txt ]; then
    echo "Mutants missed in every configuration (real coverage gaps):"
    cat mutants.out/missed.txt
    problems=1
fi

if [ -s mutants.out/timeout.txt ]; then
    if [ "$problems" -ne 0 ]; then
        echo
    fi
    echo "Mutants that timed out in every configuration (tests never fail for them):"
    cat mutants.out/timeout.txt
    problems=1
fi

if [ "$problems" -ne 0 ]; then
    exit 1
fi

echo "No mutants were missed or timed out in any configuration."
exit 0

