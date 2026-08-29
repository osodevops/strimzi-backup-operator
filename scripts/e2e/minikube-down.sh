#!/usr/bin/env bash
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
minikube delete -p "$PROFILE" || true
git -C "$ROOT" worktree remove --force "$E2E_DIR/src-v0.2.22" 2>/dev/null || true
[ -n "${KEEP_IMAGES:-}" ] || docker rmi "$IMAGE_REPO:0.2.22-a" "$IMAGE_REPO:0.2.22-b" "$IMAGE_REPO:0.2.23-a" "$IMAGE_REPO:0.2.23-b" 2>/dev/null || true
prev=$(cat "$E2E_DIR/prev-context" 2>/dev/null || true)
[ -n "$prev" ] && kubectl config use-context "$prev" >/dev/null 2>&1 || true
[ -n "${KEEP_EVIDENCE:-}" ] || rm -rf "$E2E_DIR"
