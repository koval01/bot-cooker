# Monitoring and stats

On a server set up by the installer:

```bash
bot-cooker-ctl status    # running? persona, active connections, requests, clients
bot-cooker-ctl stats     # the full statistics document
bot-cooker-ctl logs      # follow the log
```

## The stats endpoint

```bash
curl -H "X-Admin-Token: <token>" https://trap.example.com/_cooker/stats
```

returns JSON:

```json
{
  "stats": {
    "uptime_secs": 599,
    "started_unix": 1792681423,
    "requests_total": 72,
    "connections_total": 72,
    "active": 3,
    "bytes_fed": 61345,
    "wasted_secs": 1897,
    "unique_ips": 58
  },
  "top_ips": [
    { "ip": "203.0.113.5", "requests": 8, "bytes": 8308, "first_seen": 1792681435, "last_seen": 1792681666 }
  ]
}
```

| Field | Meaning |
|---|---|
| `active` | connections held right now |
| `connections_total`, `requests_total` | since start |
| `bytes_fed` | decoy bytes sent |
| `wasted_secs` | total time clients spent connected — the time the trap took from them |
| `unique_ips` | distinct clients (capped by `stats.max_tracked_ips`) |
| `top_ips` | the 20 clients that received the most bytes |

The token comes from `COOKER_ADMIN_TOKEN` (the installer stores it in
`/opt/bot-cooker/.env`). A request to the stats path without the right token
is answered like any other request: slowly, with decoy content. Send the
token in the header rather than in the URL, so it does not end up in proxy
logs.

Counters live in memory and reset on restart; nothing is written to disk.

## Logs

Every `stats.report_interval_secs` the process logs a summary line:

```text
INFO report: active 1 | conns 3 | requests 3 | unique IPs 3 | fed 111 B | bot-time 14s | up 2m 0s
```

```bash
sudo docker compose -p bot-cooker logs -f bot-cooker     # installer deployment
```

The startup line names the persona (`persona: Driftlight Clinic
(driftlight-clinic.eu) — Laravel on apache, mysql | routes: 28`). Set
`RUST_LOG=debug` for more detail.

## Several nodes

`deploy/fleet.sh stats` collects the counters of every node in your
inventory, adds them up and lists the busiest clients across all of them. See
[Several nodes](../deploy/fleet.md).

## What to watch

- Memory stays proportional to `active` (about 30 KiB per held connection)
  and bounded by `limits.max_concurrent`.
- `active` close to `max_concurrent` for long periods means the cap is
  protecting the host; raise it only if the server has memory and open-file
  headroom.
- Restarts change nothing for scanners as long as the master key is the same.
