#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "Usage: ./scripts/delete_old_edgegap_image_tags.sh <image-name> <tags-to-keep>" >&2
  exit 1
fi

if [[ -z "${EDGEGAP_API_TOKEN:-}" ]]; then
  echo "Missing required env var: EDGEGAP_API_TOKEN" >&2
  exit 1
fi

image_name="$1"
tags_to_keep="$2"

tags_json="[]"
page=1
while :; do
  page_json="$(curl --silent --fail-with-body \
    --url "https://api.edgegap.com/v1/container-registry/images/${image_name}/tags?page=${page}" \
    --header "Authorization: token ${EDGEGAP_API_TOKEN}")"
  tags_json="$(jq --argjson page "${page_json}" '. + $page.data' <<<"${tags_json}")"
  [[ "$(jq '.pagination.has_next' <<<"${page_json}")" == "true" ]] || break
  page=$((page + 1))
done

old_tags="$(jq -r --argjson keep "${tags_to_keep}" 'sort_by(.last_push_at) | reverse | .[$keep:][].tag' <<<"${tags_json}")"

for tag in ${old_tags}; do
  echo "Deleting ${image_name}:${tag}"
  curl --silent --fail-with-body --request DELETE \
    --url "https://api.edgegap.com/v1/container-registry/images/${image_name}/tags/${tag}" \
    --header "Authorization: token ${EDGEGAP_API_TOKEN}"
  echo
done
