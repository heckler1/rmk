#!/usr/bin/env bash

# Build firmware in Docker, with no Rust toolchain on the host.
#
# Usage:
#   scripts/docker.sh build <example-dir>        # build firmware into <example-dir>
#   scripts/docker.sh run <dir> [command...]     # run a command in <dir> (default: a shell)
#   scripts/docker.sh image [--no-cache]         # rebuild the image; --no-cache updates Rust
#   scripts/docker.sh clean                      # delete cached build output and downloads

set -euo pipefail

repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
volumes=(rmk-docker-target rmk-docker-cargo-registry rmk-docker-cargo-git)

usage() {
    sed -n '5,9s/^# \{0,1\}//p' "${BASH_SOURCE[0]}" >&2
    exit 2
}

build_image() {
    local stage=$1
    shift
    docker build "$@" --target "$stage" --tag "rmk-builder:$stage" \
        --file "$repo/docker/Dockerfile" "$repo"
}

# Sets `rel` to the path of $1 inside the repo.
resolve_dir() {
    local abs
    abs=$(cd "$1" && pwd -P)
    case "$abs" in
        "$repo") rel=. ;;
        "$repo"/*) rel=${abs#"$repo"/} ;;
        *) echo "error: $1 is outside $repo" >&2; exit 2 ;;
    esac
}

run_in_container() {
    local dir=$1
    shift
    local stage=base
    if grep -qs '^channel = "esp"' "$repo/$dir/rust-toolchain.toml"; then
        stage=esp
    fi
    build_image "$stage"

    local tty=()
    if [[ -t 0 ]]; then
        tty=(-it)
    fi
    docker run --rm "${tty[@]+"${tty[@]}"}" \
        --volume "$repo:/workspace" \
        --volume rmk-docker-target:/rmk-target \
        --volume rmk-docker-cargo-registry:/usr/local/cargo/registry \
        --volume rmk-docker-cargo-git:/usr/local/cargo/git \
        --env "CARGO_TARGET_DIR=/rmk-target/$dir" \
        --workdir "/workspace/$dir" \
        "rmk-builder:$stage" \
        bash /workspace/docker/entrypoint.sh "$@"
}

command=${1:-}
[[ $# -gt 0 ]] && shift

case "$command" in
    build)
        [[ $# -eq 1 ]] || usage
        resolve_dir "$1"
        run_in_container "$rel" bash /workspace/docker/build.sh
        ;;
    run)
        [[ $# -ge 1 ]] || usage
        resolve_dir "$1"
        shift
        if [[ $# -eq 0 ]]; then
            set -- bash
        fi
        run_in_container "$rel" "$@"
        ;;
    image)
        build_image base "$@"
        ;;
    clean)
        docker volume rm "${volumes[@]}"
        ;;
    *)
        usage
        ;;
esac
