# Several nodes

`deploy/fleet.sh` drives any number of trap servers from your workstation
over SSH. It needs `ssh`, `rsync` (for `--source`), `curl` and `jq` locally,
and an SSH user with passwordless `sudo` on each node.

List the nodes in `deploy/nodes` (ignored by git, so your addresses never end
up in a commit), one per line, SSH target then trap hostname:

```text
# deploy/nodes
ubuntu@203.0.113.10   trap-1.example.com
root@198.51.100.7     trap-2.example.com
```

Each node should have its own hostname, and gets its own master key: never
reuse a key, the key *is* the persona.

## Commands

```bash
deploy/fleet.sh deploy                 # install or upgrade every node (released image)
deploy/fleet.sh deploy --version 0.2.0 # a specific release
deploy/fleet.sh deploy --source        # build this checkout on each node instead
deploy/fleet.sh status                 # containers and persona per node
deploy/fleet.sh stats                  # counters per node, totals, busiest clients
deploy/fleet.sh stats --top 20 --raw   # more clients / raw JSON
```

`deploy` copies `deploy/install.sh` to each node over SSH and runs it with
the node's hostname, one node after another. `stats` reads each node's admin
token over SSH at the moment it needs it, passes it to `curl` on standard
input, and never stores it.

Use another inventory with `NODES_FILE=path/to/nodes deploy/fleet.sh …`.

## Rolling out changes

Upgrade one node, watch it for a while, then the rest: see the
[rollout checklist](../operate/rollout.md).
