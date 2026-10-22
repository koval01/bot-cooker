# Architecture

bot-cooker is a single Rust binary (Tokio, no web framework) with its
content in an embedded template catalog. This chapter follows one request
through it.

## A request, end to end

1. **Accept** — `http::serve` accepts the connection; `http::conn` reads
   requests with `wire::read_head`, applying the persona's front-server rules
   (header size limits, keep-alive, timeouts). Malformed requests are refused
   the way that server refuses them (`400`, `414`, `505`, …).
2. **Persona** — `Engine::current()` returns the *incarnation* for the current
   persona era: the `Persona` derived from the master key
   (`persona::derive`, one RNG stream per facet) and its `Router`, built from
   `Persona::routes()`.
3. **Route** — the router matches the path against glob patterns and picks a
   `Profile` (`env`, `git`, `sql_dump`, `labyrinth`, `asset`, …). Unknown
   paths get `not_found`.
4. **Render** — `render::render` builds a `Scope` (persona, `World` at the
   current epochs, request) and lets `render::methods::gate` decide what the
   method gets. The profile module in `render/profiles/` chooses templates and
   context; text comes only from `templates/`. The result is a `Doc`, to which
   `render::headers` adds the front server's and framework's headers.
5. **Pace** — `tarpit::Plan::sample` decides the wait before the head, the
   wait before the body, the chunking regimes and whether the response will
   be cut; `tarpit::respond` writes it through `wire::response` in the front
   server's dialect. Long artifacts (logs, dumps, sitemaps, API pages) are
   streamed block by block, each block rendered on demand.
6. **Count** — `stats` records the connection, bytes and time per client IP.

## Modules

| Module | Responsibility |
|---|---|
| `config` | the YAML configuration and its defaults |
| `catalog` | MiniJinja environment, embedded templates, overlays, custom filters |
| `data` | typed, validated view of `templates/data/*.yaml` |
| `persona` | the persona model, its derivation, route table and versions |
| `world` | what happened on the imaginary system: people, records, posts, commits, traffic, icons |
| `render` | profiles, headers, methods, caching of expensive documents |
| `engine` | persona eras, epochs, incarnations; fingerprint tests and audit |
| `tarpit` | pacing plans and the drip |
| `wire` | HTTP/1.1 parsing and serialisation per front server |
| `http` | accept loop, connection handling, admin endpoint |
| `stats` | counters, per-IP table, periodic report |
| `routing` | glob router and allowlist |
| `text` | per-persona prose model and voice |
| `pack` | the offline content-pack generator (feature `pack`) |

## Determinism and time

- Content randomness always comes from keyed streams: `Persona::rng(label)`,
  `World::fixed_rng`, `cfg_rng`, `log_rng`, or the template `choice` filter
  keyed with `persona_seed`. The same key, path and epoch give the same bytes.
- Only pacing (`tarpit`) and per-request tokens (session cookies, request
  ids) use fresh randomness.
- Epochs come from the wall clock through `engine`; tests pin the clock.

## Tests that guard the design

- `engine::tests` — every route renders for many personas; stability within
  an instance; differences across instances; realism checks (autoindex
  layouts, logs in order, dumps well formed, linked assets served).
- `engine::audit` — renders 40 instances and fails if a line of our own
  making appears on more than 15% of them.
- `render::tests`, `persona::tests`, `world::*` — formats and invariants.
- `tests/http.rs`, `tests/server.rs` — the real binary over real sockets.

## Where it is heading

The [contributing guide](contributing.md) lists the planned steps that make
new stacks, servers and artifacts mostly a matter of data, and packs the
supported form of third-party extension.
