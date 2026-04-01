#!/usr/bin/env bash
set -e

ARTIFACTS_DIR="${ARTIFACTS_DIR:-/app/prover/artifacts}"
mkdir -p "$ARTIFACTS_DIR"

BASE_URL="https://pub-6830de9795e8488c9fc2d2577d9e3596.r2.dev"

fetch() {
  local name=$1
  local dest="$ARTIFACTS_DIR/$name"

  if [ -f "$dest" ]; then
    echo "artifact already present: $name"
    return
  fi

  echo "fetching $name from R2..."
  curl -fSL "$BASE_URL/$name" -o "$dest"
  echo "done: $name"
}

fetch "pk.key"
fetch "vk.key"
fetch "model.compiled"
fetch "settings.json"

echo "all artifacts ready"