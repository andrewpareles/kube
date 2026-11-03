#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

hash_paths() {
    for input_path in "$@"; do
        if [[ -d "$input_path" ]]; then
            find "$input_path" -type f
            continue
        fi

        if [[ -f "$input_path" ]]; then
            printf '%s\n' "$input_path"
            continue
        fi

        echo "Missing bundle input: $input_path" >&2
        exit 1
    done | LC_ALL=C sort | while IFS= read -r file_path; do
        shasum -a 256 "$file_path"
    done | shasum -a 256 | awk '{print $1}'
}

are_outputs_present() {
    for output_path in "$@"; do
        if [[ ! -f "$output_path" ]]; then
            return 1
        fi
    done
}

bundle_kube_ts_api_if_needed() {
    local stamp_path="packages/kube-ts-api/dist/.bundle-input.sha256"
    local input_hash
    input_hash="$(
        hash_paths \
            packages/kube-ts-api/package.json \
            packages/kube-ts-api/package-lock.json \
            packages/kube-ts-api/tsconfig.types.json \
            packages/kube-ts-api/api-extractor.json \
            packages/kube-ts-api/src
    )"

    if [[ -f "$stamp_path" ]] \
        && [[ "$(cat "$stamp_path")" == "$input_hash" ]] \
        && are_outputs_present \
            packages/kube-ts-api/dist/kube-ts-api.bundle.js \
            packages/kube-ts-api/dist/jsx-runtime.js \
            packages/kube-ts-api/dist/kube-ts-api.api.d.ts; then
        echo "kube-ts-api bundle is current."
        return
    fi

    echo "Bundling kube-ts-api..."
    (cd "$REPO_ROOT/packages/kube-ts-api" && npm run bundle)
    printf '%s\n' "$input_hash" > "$stamp_path"
}

bundle_kube_ts_api_if_needed
