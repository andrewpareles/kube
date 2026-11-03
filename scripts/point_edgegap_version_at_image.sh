#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 4 ]]; then
  echo "Usage: ./scripts/point_edgegap_version_at_image.sh <application-name> <version-name> <docker-image> <docker-tag>" >&2
  exit 1
fi

if [[ -z "${EDGEGAP_API_TOKEN:-}" ]]; then
  echo "Missing required env var: EDGEGAP_API_TOKEN" >&2
  exit 1
fi

application_name="$1"
version_name="$2"
docker_image="$3"
docker_tag="$4"

# The release workflow repoints an existing version at each release's image. Creating a version is first-time setup,
# done by scripts/configure_edgegap_version.sh or scripts/configure_editor_edgegap_version.sh.
version_status_code="$(curl --silent --output /dev/null --write-out '%{http_code}' \
  --request GET \
  --url "https://api.edgegap.com/v1/app/${application_name}/version/${version_name}" \
  --header "Authorization: token ${EDGEGAP_API_TOKEN}" \
  --header "Content-Type: application/json")"
if [[ "${version_status_code}" == "404" ]]; then
  echo "Edgegap version ${version_name} of ${application_name} is missing. Create it with its configure script and ${docker_tag}, then rerun this workflow." >&2
  exit 1
fi

if [[ "${version_status_code}" != "200" ]]; then
  echo "Unexpected Edgegap status while checking ${version_name} of ${application_name}: ${version_status_code}" >&2
  exit 1
fi

curl --fail-with-body --request PATCH \
  --url "https://api.edgegap.com/v1/app/${application_name}/version/${version_name}" \
  --header "Authorization: token ${EDGEGAP_API_TOKEN}" \
  --header "Content-Type: application/json" \
  --data @- <<EOF
{
  "docker_image": "${docker_image}",
  "docker_tag": "${docker_tag}"
}
EOF
