#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "Usage: ./scripts/configure_edgegap_version.sh <version-name> <docker-tag>" >&2
  exit 1
fi

require_env_var() {
  local env_var_name="$1"
  if [[ -z "${!env_var_name:-}" ]]; then
    echo "Missing required env var: ${env_var_name}" >&2
    exit 1
  fi
}

require_env_var "EDGEGAP_API_TOKEN"
require_env_var "EDGEGAP_APPLICATION_NAME"
require_env_var "EDGEGAP_REGISTRY_URL"
require_env_var "EDGEGAP_REGISTRY_PROJECT"
require_env_var "EDGEGAP_IMAGE_NAME"
require_env_var "EDGEGAP_REGISTRY_USERNAME"
require_env_var "EDGEGAP_REGISTRY_TOKEN"

version_name="$1"
docker_tag="$2"

# Keep the static Edgegap version settings here.
edgegap_game_port_name="gameport"
edgegap_internal_port="7777"
edgegap_server_cpu_units="1024"
edgegap_server_memory_mb="2048"
edgegap_time_to_deploy_seconds="120"
# Edgegap ends a game server after 14 hours, even with players in it. An empty server stops itself after 5 seconds.
edgegap_max_duration_minutes="840"
# Edgegap saves each deployment's container logs to the kube-new-edgegap-logs S3 bucket in our AWS account, kept 30 days.
edgegap_endpoint_storage_name="kube-new-logs"

get_edgegap_version_status_code() {
  local version_name="$1"

  curl --silent --output /dev/null --write-out '%{http_code}' \
    --request GET \
    --url "https://api.edgegap.com/v1/app/${EDGEGAP_APPLICATION_NAME}/version/${version_name}" \
    --header "Authorization: token ${EDGEGAP_API_TOKEN}" \
    --header "Content-Type: application/json"
}

version_status_code="$(get_edgegap_version_status_code "${version_name}")"
if [[ "${version_status_code}" == "200" ]]; then
  echo "Edgegap version ${version_name} already exists. This script is only for first-time setup." >&2
  exit 1
fi

if [[ "${version_status_code}" != "404" ]]; then
  echo "Unexpected Edgegap status while checking ${version_name}: ${version_status_code}" >&2
  exit 1
fi

curl --fail-with-body --request POST \
  --url "https://api.edgegap.com/v1/app/${EDGEGAP_APPLICATION_NAME}/version" \
  --header "Authorization: token ${EDGEGAP_API_TOKEN}" \
  --header "Content-Type: application/json" \
  --data @- <<EOF
{
  "name": "${version_name}",
  "is_active": true,
  "docker_repository": "${EDGEGAP_REGISTRY_URL}",
  "docker_image": "${EDGEGAP_REGISTRY_PROJECT}/${EDGEGAP_IMAGE_NAME}",
  "docker_tag": "${docker_tag}",
  "private_username": "${EDGEGAP_REGISTRY_USERNAME}",
  "private_token": "${EDGEGAP_REGISTRY_TOKEN}",
  "req_cpu": ${edgegap_server_cpu_units},
  "req_memory": ${edgegap_server_memory_mb},
  "time_to_deploy": ${edgegap_time_to_deploy_seconds},
  "max_duration": ${edgegap_max_duration_minutes},
  "endpoint_storage": "${edgegap_endpoint_storage_name}",
  "verify_image": true,
  "ports": [
    {
      "port": ${edgegap_internal_port},
      "protocol": "UDP",
      "to_check": true,
      "tls_upgrade": false,
      "name": "${edgegap_game_port_name}"
    }
  ]
}
EOF
