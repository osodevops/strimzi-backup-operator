#!/usr/bin/env bash
# Scenario 5 — leader crashes without releasing.
#  a) container SIGKILL: the restarted container has the same identity (pod
#     name) and resumes the lease immediately without a transition.
#  b) pod force-deleted (no grace, no release): another replica must wait for
#     leaseDuration to expire before taking over; transitions +1.
export SCEN=05-crash; source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
operator_uninstall
operator_install "$CHART_FIX" 0.2.23-b --set replicaCount=2; apply_cr
wait_for 60 leader_count_is 1 >/dev/null || fail "no single leader"
leader=$(leader_pods); tr0=$(lease_transitions)
# (a) SIGKILL the operator child of tini inside the leader container.
k -n "$NS_OP" exec "$leader" -- sh -c 'for p in /proc/[0-9]*; do pid=${p#/proc/}; [ "$pid" = 1 ] && continue; if [ "$(tr "\0" "\n" < "$p/cmdline" 2>/dev/null | head -n1)" = kafka-backup-operator ]; then kill -9 "$pid"; fi; done' 2>/dev/null || true
t_kill=$(date +%s)
wait_for 60 restarts_ge "$leader" 1 >/dev/null || fail "killed container did not restart"
renew_moves() { local a b; a=$(lease_state | cut -f4); sleep 2.5; b=$(lease_state | cut -f4); [ "$a" != "$b" ]; }
wait_for 20 renew_moves >/dev/null || fail "lease not renewed after container restart"
[ "$(lease_holder)" = "$leader" ] || fail "expected the restarted container (same identity) to resume, holder=$(lease_holder)"
[ "$(lease_transitions)" = "$tr0" ] || fail "a resumed lease must not count as a transition"
evidence "(a) container SIGKILL: same identity resumed within $(( $(date +%s) - t_kill ))s, transitions unchanged"
pass "(a) restart resumes the lease"
# (b) Freeze the leader (SIGSTOP): it can neither renew nor release, so the
#     other replica must wait for the lease to expire. Resuming it (SIGCONT)
#     makes it discover the new holder and exit — the split-brain guard.
leader=$(lease_holder); other=$(op_pod_names | grep -v "^$leader$" | head -1); tr1=$(lease_transitions); r1=$(pod_times | grep "^$leader" | grep -o 'restarts=[0-9]*' | cut -d= -f2)
LEASELOG=$(watch_lease_bg "$EVID/$SCEN/lease.jsonl"); sleep 1
signal_operator() { k -n "$NS_OP" exec "$1" -- sh -c 'for p in /proc/[0-9]*; do pid=${p#/proc/}; [ "$pid" = 1 ] && continue; if [ "$(tr "\0" "\n" < "$p/cmdline" 2>/dev/null | head -n1)" = kafka-backup-operator ]; then kill -'"$2"' "$pid"; fi; done' 2>/dev/null; }
last_renew=$(lease_state | cut -f4)
signal_operator "$leader" STOP; t_stop=$(date +%s)
sleep 4; [ "$(lease_state | cut -f4)" = "$last_renew" ] || fail "renewTime kept moving after SIGSTOP"
holder_changed() { local h; h=$(lease_holder); [ -n "$h" ] && [ "$h" != "$leader" ]; }
wait_for 40 holder_changed >/dev/null || fail "no takeover within 40s"
took=$(( $(date +%s) - t_stop )); sleep 2; kill "$LEASELOG" 2>/dev/null || true
summary=$(python3 "$ROOT/scripts/e2e/analyze.py" lease "$EVID/$SCEN/lease.jsonl"); log "expiry takeover lease: $summary"
[ "$took" -ge 13 ] && [ "$took" -le 22 ] || fail "takeover after ${took}s, expected leaseDuration (15s) + up to one retry"
[ "$(lease_holder)" = "$other" ] || fail "expected $other to take over, holder=$(lease_holder)"
[ "$(lease_transitions)" = "$((tr1+1))" ] || fail "leaseTransitions $tr1 -> $(lease_transitions)"
evidence "(b) SIGSTOP: takeover by $other after ${took}s, transitions $tr1 -> $(lease_transitions)"
pass "(b) expiry takeover after ${took}s"
# (c) Resume the frozen ex-leader: its next step sees another holder and it must exit.
signal_operator "$leader" CONT; t_cont=$(date +%s)
wait_for 30 restarts_ge "$leader" "$((r1+1))" >/dev/null || fail "resumed ex-leader did not exit after losing the lease"
k -n "$NS_OP" logs "$leader" --previous 2>/dev/null > "$EVID/$SCEN/ex-leader-previous.log" || true
grep -q "leadership lost" "$EVID/$SCEN/ex-leader-previous.log" || fail "previous log lacks the 'leadership lost' reason"
[ "$(lease_holder)" = "$other" ] || fail "holder changed unexpectedly after the ex-leader resumed: $(lease_holder)"
wait_for 20 readyz_is "$leader" standby >/dev/null || fail "restarted ex-leader readyz: $(readyz_of "$leader")"
wait_for 30 leader_count_is 1 >/dev/null || fail "leader count after resume"
evidence "(c) SIGCONT: ex-leader exited $(( $(date +%s) - t_cont ))s after resuming, now standby; holder still $other"
pass "(c) lost leadership -> exit -> standby"
