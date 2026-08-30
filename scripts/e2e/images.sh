#!/usr/bin/env bash
# Build the four operator images the scenarios need (arm64/native, from source)
# and load them into the profile. Skips tags already present unless FORCE=1.
#   0.2.22-a  v0.2.22 tag,  DEFAULT_BACKUP_IMAGE -> $ENGINE_OLD
#   0.2.22-b  v0.2.22 tag,  DEFAULT_BACKUP_IMAGE -> $ENGINE_NEW
#   0.2.23-a  working tree, DEFAULT_BACKUP_IMAGE -> $ENGINE_OLD
#   0.2.23-b  working tree, DEFAULT_BACKUP_IMAGE -> $ENGINE_NEW (as compiled)
# ENGINE_OLD / ENGINE_NEW come from lib.sh (ENGINE_NEW defaults to this
# checkout's scripts/default-engine-image.sh). The "0.2.23" tag names are the
# working-tree builds whatever version the tree is at.
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
mkdir -p "$E2E_DIR"
have() { minikube -p "$PROFILE" image ls 2>/dev/null | grep -q "docker.io/${IMAGE_REPO}:$1\$"; }
build() { # <src-dir> <tag> <engine-image>
  local src=$1 tag=$2 engine=$3
  if [ -n "${ONLY:-}" ] && [[ "$tag" != ${ONLY}* ]]; then return; fi
  if [ -z "${FORCE:-}" ] && have "$tag"; then log "image $tag already loaded"; return; fi
  local work="$E2E_DIR/build-$tag"; rm -rf "$work"; mkdir -p "$work"
  rsync -a --exclude target --exclude .e2e --exclude .git "$src/" "$work/"
  # The constant lives in src/engine.rs since 0.2.25 and in
  # src/reconcilers/mod.rs before that; patch whichever this source has.
  local const_file="" f
  for f in src/engine.rs src/reconcilers/mod.rs; do
    [ -f "$work/$f" ] && grep -q '^pub const DEFAULT_BACKUP_IMAGE' "$work/$f" && { const_file="$work/$f"; break; }
  done
  [ -n "$const_file" ] || fail "no DEFAULT_BACKUP_IMAGE constant in $work"
  sed -i '' "s|^\(pub const DEFAULT_BACKUP_IMAGE: &str = \)\"osodevops/kafka-backup:v[0-9]*\.[0-9]*\.[0-9]*\";|\1\"$engine\";|" "$const_file"
  grep -q "\"$engine\"" "$const_file" || fail "could not set default image in $const_file"
  docker build -q -t "$IMAGE_REPO:$tag" "$work" >/dev/null
  local got; got=$(docker run --rm --entrypoint sh "$IMAGE_REPO:$tag" -c 'grep -a -o -m1 "osodevops/kafka-backup:v[0-9]*\.[0-9]*\.[0-9]*" /usr/local/bin/kafka-backup-operator | head -n1')
  [ "$got" = "$engine" ] || fail "image $tag carries $got, expected $engine"
  minikube -p "$PROFILE" image load "$IMAGE_REPO:$tag"
  log "built+loaded $IMAGE_REPO:$tag (default job image $got)"
}
if [ ! -d "$E2E_DIR/src-v0.2.22" ]; then git -C "$ROOT" worktree add "$E2E_DIR/src-v0.2.22" v0.2.22 >/dev/null; fi
build "$E2E_DIR/src-v0.2.22" 0.2.22-a "$ENGINE_OLD"
build "$E2E_DIR/src-v0.2.22" 0.2.22-b "$ENGINE_NEW"
build "$ROOT" 0.2.23-a "$ENGINE_OLD"
build "$ROOT" 0.2.23-b "$ENGINE_NEW"

# Job images. osodevops/kafka-backup is published for linux/amd64 only; on an
# arm64 host build re-labelled copies first (see the kind notes in the repo
# issue history: FROM --platform=linux/amd64 <image> built with
# --platform linux/arm64) so the node can run them without pulling.
for img in "$ENGINE_OLD" "$ENGINE_NEW"; do
  if docker image inspect "$img" >/dev/null 2>&1; then
    minikube -p "$PROFILE" image ls 2>/dev/null | grep -q "$img\$" && { log "job image $img already loaded"; continue; }
    minikube -p "$PROFILE" image load "$img" && log "loaded job image $img ($(docker image inspect "$img" --format '{{.Architecture}}'))"
  else
    log "WARN: $img not present locally; job pods will try to pull it"
  fi
done
