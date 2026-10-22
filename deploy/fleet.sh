#!/usr/bin/env bash
# Manage several trap nodes from your workstation over SSH.
#
#   deploy/fleet.sh deploy [--source] [--version TAG]   install or upgrade every node
#   deploy/fleet.sh status                              containers and persona of every node
#   deploy/fleet.sh stats [--top N] [--raw]             counters of every node, merged
#
# Nodes are listed in deploy/nodes (not committed), one per line:
#
#   ubuntu@203.0.113.10   trap-1.example.com
#   root@198.51.100.7     trap-2.example.com
#
# The SSH user needs passwordless sudo. `deploy` runs deploy/install.sh on each
# node; with --source it first copies this checkout to the node and builds the
# image there instead of pulling the released one. Admin tokens are read from
# each node over SSH when needed and never stored here.
set -euo pipefail

cd "$(dirname "$0")/.."
NODES_FILE="${NODES_FILE:-deploy/nodes}"
SSH_OPTS=(-o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=30 -o StrictHostKeyChecking=accept-new)
REMOTE_DIR=/opt/bot-cooker
REMOTE_SRC=/opt/bot-cooker-src

die() { printf 'error: %s\n' "$*" >&2; exit 1; }

nodes() {
  [ -f "$NODES_FILE" ] || die "no $NODES_FILE; list your nodes there as '<ssh-target> <domain>' lines"
  grep -Ev '^[[:space:]]*(#|$)' "$NODES_FILE"
}

cmd_deploy() {
  local source=0 version=latest
  while [ $# -gt 0 ]; do
    case "$1" in
      --source) source=1; shift ;;
      --version) version="${2:?--version needs a tag}"; shift 2 ;;
      *) die "deploy: unknown option $1" ;;
    esac
  done
  local target domain args script
  while read -r target domain _; do
    echo "=== $domain ($target)"
    args=(--yes --lang en --domain "$domain")
    if [ "$source" = 1 ]; then
      ssh "${SSH_OPTS[@]}" -n "$target" "sudo mkdir -p $REMOTE_SRC && sudo chown \"\$(id -u):\$(id -g)\" $REMOTE_SRC"
      rsync -az --delete -e "ssh ${SSH_OPTS[*]}" \
        --exclude target/ --exclude .git/ --exclude .idea/ --exclude docs/book/ \
        --exclude deploy/nodes --exclude packs/ \
        ./ "$target:$REMOTE_SRC/"
      args+=(--source "$REMOTE_SRC")
      script="$REMOTE_SRC/deploy/install.sh"
    else
      [ "$version" = latest ] || args+=(--version "$version")
      # Run it as a file, so it can keep a copy of itself as bot-cooker-ctl.
      script=/tmp/bot-cooker-install.sh
      scp -q "${SSH_OPTS[@]}" deploy/install.sh "$target:$script"
    fi
    # Arguments are quoted for the remote shell.
    # shellcheck disable=SC2029
    ssh "${SSH_OPTS[@]}" -n "$target" "sudo bash $script $(printf '%q ' "${args[@]}")"
  done < <(nodes)
}

cmd_status() {
  local target domain
  while read -r target domain _; do
    echo "=== $domain ($target)"
    ssh "${SSH_OPTS[@]}" -n "$target" "
      sudo docker ps --filter name=bot-cooker- --format '  {{.Names}}: {{.Status}}'
      sudo docker logs bot-cooker-bot-cooker-1 2>&1 | grep 'INFO persona:' | tail -n 1 | sed -e 's/.*persona: /  persona: /' -e 's/ | stats.*//'
    " || echo "  unreachable"
  done < <(nodes)
}

# One node's stats document plus its name, or an error stub, into $1.
fetch_stats() {
  local out="$1" target="$2" domain="$3" token stats
  if ! token="$(ssh "${SSH_OPTS[@]}" -n "$target" "sudo sed -n 's/^COOKER_ADMIN_TOKEN=//p' $REMOTE_DIR/.env" 2>/dev/null)" || [ -z "$token" ]; then
    jq -n --arg node "$domain" '{node: $node, error: "no admin token (ssh failed or not installed)"}' > "$out"
    return
  fi
  # The token goes in on stdin so it never shows up in the process list.
  if ! stats="$(curl -fsS -m 20 -H @- "https://$domain/_cooker/stats" <<<"X-Admin-Token: $token" 2>&1)"; then
    jq -n --arg node "$domain" --arg e "$stats" '{node: $node, error: $e}' > "$out"
    return
  fi
  jq --arg node "$domain" '. + {node: $node}' <<<"$stats" > "$out"
}

cmd_stats() {
  local top=10 raw=0
  while [ $# -gt 0 ]; do
    case "$1" in
      --top) top="${2:?--top needs a number}"; shift 2 ;;
      --raw) raw=1; shift ;;
      *) die "stats: unknown option $1" ;;
    esac
  done
  command -v jq >/dev/null || die "jq is required"
  local tmp i=0 target domain
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' RETURN
  while read -r target domain _; do
    fetch_stats "$tmp/$i.json" "$target" "$domain" &
    i=$((i + 1))
  done < <(nodes)
  wait
  if [ "$raw" = 1 ]; then
    jq -s '.' "$tmp"/*.json
    return
  fi
  jq -rs --argjson top "$top" --argjson now "$(date +%s)" '
    def bytes: if . >= 1073741824 then "\(. / 1073741824 * 10 | floor / 10) GB"
               elif . >= 1048576 then "\(. / 1048576 * 10 | floor / 10) MB"
               elif . >= 1024 then "\(. / 1024 * 10 | floor / 10) KB"
               else "\(.) B" end;
    def dur: if . >= 86400 then "\(. / 86400 | floor)d \(. % 86400 / 3600 | floor)h"
             elif . >= 3600 then "\(. / 3600 | floor)h \(. % 3600 / 60 | floor)m"
             elif . >= 60 then "\(. / 60 | floor)m \(. % 60)s"
             else "\(.)s" end;
    def pad($n): tostring | . + (" " * ([$n - length, 0] | max));
    def lpad($n): tostring | (" " * ([$n - length, 0] | max)) + .;
    def line: "up \(.uptime_secs | dur) | active \(.active) | conns \(.connections_total) | requests \(.requests_total) | IPs \(.unique_ips) | fed \(.bytes_fed | bytes) | bot-time \(.wasted_secs | dur)";

    (map(select(.error)) | .[] | "\(.node)  ERROR: \(.error)"),
    (map(select(.stats)) | .[] | "\(.node)\n  \(.stats | line)\n"),
    (map(select(.stats) | .stats) | if length > 1 then
      "TOTAL  active \(map(.active) | add) | conns \(map(.connections_total) | add) | requests \(map(.requests_total) | add) | IPs \(map(.unique_ips) | add) | fed \(map(.bytes_fed) | add | bytes) | bot-time \(map(.wasted_secs) | add | dur)\n"
    else empty end),
    "Top \($top) client IPs by bytes fed:",
    ("  " + ("IP" | pad(40)) + ("node" | pad(24)) + ("requests" | lpad(9)) + ("fed" | lpad(11)) + "  last seen"),
    ([.[] | select(.stats) | .node as $n | .top_ips[] | . + {node: ($n | split(".")[0])}]
      | sort_by(-.bytes, -.requests) | .[:$top][]
      | "  " + (.ip | pad(40)) + (.node | pad(24)) + (.requests | lpad(9)) + (.bytes | bytes | lpad(11)) + "  \(($now - .last_seen) | dur) ago")
  ' "$tmp"/*.json
}

case "${1:-}" in
  deploy) shift; cmd_deploy "$@" ;;
  status) shift; cmd_status "$@" ;;
  stats) shift; cmd_stats "$@" ;;
  *) sed -n '2,6p' "$0" | sed 's/^# \{0,1\}//'; exit 2 ;;
esac
