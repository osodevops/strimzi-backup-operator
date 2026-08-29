#!/usr/bin/env bash
# Swap the "fixed" e2e image (sbo-e2e/operator:0.2.23-b) for the published
# ghcr.io release, so the scenarios run against the real artifact.
# Usage: use-released.sh <version, e.g. 0.2.23>
# The published image is linux/amd64 only; on an arm64 host it is re-labelled
# (FROM --platform=linux/amd64 … built for linux/arm64) so the node runs it
# under binfmt emulation without pulling.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
VER=${1:?version}
SRC="ghcr.io/osodevops/strimzi-backup-operator:$VER"
docker pull -q --platform linux/amd64 "$SRC" >/dev/null
arch=$(docker image inspect "$SRC" --format '{{.Architecture}}')
if [ "$(uname -m)" = "arm64" ] && [ "$arch" != "arm64" ]; then
  work="$E2E_DIR/relabel-$VER"; mkdir -p "$work"
  printf 'FROM --platform=linux/amd64 %s\n' "$SRC" > "$work/Dockerfile"
  docker buildx build --platform linux/arm64 --load -t "$IMAGE_REPO:0.2.23-b" "$work" >/dev/null 2>&1
else
  docker tag "$SRC" "$IMAGE_REPO:0.2.23-b"
fi
got=$(docker run --rm --entrypoint sh "$IMAGE_REPO:0.2.23-b" -c 'grep -a -o -m1 "osodevops/kafka-backup:v0\.[0-9]*\.[0-9]*" /usr/local/bin/kafka-backup-operator | head -n1')
minikube -p "$PROFILE" image rm "$IMAGE_REPO:0.2.23-b" >/dev/null 2>&1 || true
minikube -p "$PROFILE" image load "$IMAGE_REPO:0.2.23-b"
log "sbo-e2e/operator:0.2.23-b now = $SRC (created $(docker image inspect "$SRC" --format '{{.Created}}'), default job image $got)"
