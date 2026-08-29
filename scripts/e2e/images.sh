#!/usr/bin/env bash
# Build the four operator images the scenarios need (arm64/native, from source)
# and load them into the profile. Skips tags already present unless FORCE=1.
#   0.2.22-a  v0.2.22 tag, DEFAULT_BACKUP_IMAGE -> v0.19.0
#   0.2.22-b  v0.2.22 tag as released (v0.19.1)
#   0.2.23-a  working tree,  DEFAULT_BACKUP_IMAGE -> v0.19.0
#   0.2.23-b  working tree as is (v0.19.1)
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
mkdir -p "$E2E_DIR"
have() { minikube -p "$PROFILE" image ls 2>/dev/null | grep -q "docker.io/${IMAGE_REPO}:$1\$"; }
build() { # <src-dir> <tag> <default-image-version>
  local src=$1 tag=$2 ver=$3
  if [ -n "${ONLY:-}" ] && [[ "$tag" != ${ONLY}* ]]; then return; fi
  if [ -z "${FORCE:-}" ] && have "$tag"; then log "image $tag already loaded"; return; fi
  local work="$E2E_DIR/build-$tag"; rm -rf "$work"; mkdir -p "$work"
  rsync -a --exclude target --exclude .e2e --exclude .git "$src/" "$work/"
  sed -i '' "s|osodevops/kafka-backup:v0\.19\.[0-9]*|osodevops/kafka-backup:$ver|" "$work/src/reconcilers/mod.rs"
  grep -q "kafka-backup:$ver" "$work/src/reconcilers/mod.rs" || fail "could not set default image in $work"
  docker build -q -t "$IMAGE_REPO:$tag" "$work" >/dev/null
  local got; got=$(docker run --rm --entrypoint sh "$IMAGE_REPO:$tag" -c 'grep -a -o -m1 "osodevops/kafka-backup:v0\.[0-9]*\.[0-9]*" /usr/local/bin/kafka-backup-operator | head -n1')
  [ "$got" = "osodevops/kafka-backup:$ver" ] || fail "image $tag carries $got, expected $ver"
  minikube -p "$PROFILE" image load "$IMAGE_REPO:$tag"
  log "built+loaded $IMAGE_REPO:$tag (default job image $got)"
}
if [ ! -d "$E2E_DIR/src-v0.2.22" ]; then git -C "$ROOT" worktree add "$E2E_DIR/src-v0.2.22" v0.2.22 >/dev/null; fi
build "$E2E_DIR/src-v0.2.22" 0.2.22-a v0.19.0
build "$E2E_DIR/src-v0.2.22" 0.2.22-b v0.19.1
build "$ROOT" 0.2.23-a v0.19.0
build "$ROOT" 0.2.23-b v0.19.1

# Job images. osodevops/kafka-backup is published for linux/amd64 only; on an
# arm64 host build re-labelled copies first (see the kind notes in the repo
# issue history: FROM --platform=linux/amd64 <image> built with
# --platform linux/arm64) so the node can run them without pulling.
for v in v0.19.0 v0.19.1; do
  if docker image inspect "osodevops/kafka-backup:$v" >/dev/null 2>&1; then
    minikube -p "$PROFILE" image ls 2>/dev/null | grep -q "osodevops/kafka-backup:$v\$" && { log "job image $v already loaded"; continue; }
    minikube -p "$PROFILE" image load "osodevops/kafka-backup:$v" && log "loaded job image osodevops/kafka-backup:$v ($(docker image inspect "osodevops/kafka-backup:$v" --format '{{.Architecture}}'))"
  else
    log "WARN: osodevops/kafka-backup:$v not present locally; job pods will try to pull it"
  fi
done
