#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
# Dev uses a Linux container so generated Python gets the same sandbox as on Edgegap.
docker build -f crates/editor_server/linux-sandbox/Dockerfile -t kube-editor-linux-sandbox .
