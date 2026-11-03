#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "${script_dir}/.." && pwd)"
cd "${repo_root}"

paths_file="$(mktemp)"
match_count=0

cleanup() {
  rm -f "${paths_file}"
}

trap cleanup EXIT

# This keeps Git path handling safe for spaces, quotes, and newlines in file names.
while IFS= read -r -d '' path; do
  if [[ "${path}" == *tests* || "${path}" == *.kube ]]; then
    printf '%s\0' "${path}" >> "${paths_file}"
    match_count=$((match_count + 1))
  fi
done < <(git ls-files -z --modified --deleted --others --exclude-standard --deduplicate)

if [[ "${match_count}" -eq 0 ]]; then
  echo "No matching files to stage."
  exit 0
fi

git add -A --pathspec-from-file="${paths_file}" --pathspec-file-nul
echo "Staged ${match_count} matching file(s)."
