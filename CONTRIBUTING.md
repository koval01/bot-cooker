# Contributing to bot-cooker

Thank you for helping. bot-cooker raises the cost of automated attacks on
infrastructure its operators own, and it only works if it stays trustworthy:
for the people who deploy it, and for everyone whose traffic it touches.
Please read the principles first; a pull request that breaks one of them will
not be merged however good the code is.

## Principles

1. **Defence only.** The trap answers requests that clients chose to send. It
   never connects out, never scans, never "strikes back". No feature may make
   an outbound connection from the trap binary (the offline `cooker-pack`
   tool, which never runs on a trap, is the one place with an HTTP client).
2. **Harmless to clients.** Responses waste a scanner's time and nothing else:
   no exploit payloads, no malware, no decompression or parser bombs, no
   content aimed at the client's machine, no attempts to identify or track the
   people behind a scanner. Decoy credentials must never be real credentials
   of anyone.
3. **Realism over invention.** Every surface copies real software: stock error
   pages and directory listings, real header order and spelling, versions that
   were actually released, file formats as the real tools write them. When in
   doubt, capture the real thing (see `tools/capture/grab.py`) rather than
   writing it from memory.
4. **No shared fingerprint.** Two instances (two master keys) must not share
   anything a scanner could match on. Content is a deterministic function of
   the master key, the request path and the current epoch; anything random
   per request is limited to timing and per-request tokens.
5. **Calm tone.** Code, comments, docs and commit messages describe what the
   software does in plain, professional language. The project is about making
   attacks expensive, not about mocking or punishing anyone.

## Getting started

You need Rust (the exact toolchain is pinned in `rust-toolchain.toml`; rustup
installs it on the first build). Docker is only needed for the deployment
files.

```bash
cargo run                      # serves the trap on http://0.0.0.0:8080
cargo run -- --check           # validate the config and print the persona
cargo test                     # unit, engine, fingerprint-audit, HTTP and process tests
cargo clippy --all-targets     # must be warning-free
cargo fmt
```

`COOKER_MASTER_KEY=some-key cargo run -- --check` shows the persona another
key would get; a handful of keys is the quickest way to see the variety.

The documentation site lives in `docs/` (mdBook). Preview it with:

```bash
cargo install mdbook
mdbook serve docs
```

## How the code is laid out

| Path | What lives there |
|---|---|
| `src/main.rs`, `src/server.rs`, `src/service.rs` | CLI, process wiring, Windows service |
| `src/http/`, `src/wire/` | connection loop and the HTTP/1.1 wire layer, in each front server's dialect |
| `src/tarpit/` | how a response is paced: delays, chunking, aborts |
| `src/engine/` | the current persona and router; the fingerprint audit and engine tests |
| `src/persona/` | the persona model and its derivation from the master key, one RNG stream per facet |
| `src/world/` | things that "happened" on the imaginary system: people, records, posts, commits, traffic |
| `src/render/` | turning a routed request into a document: profiles, headers, method handling |
| `src/catalog/`, `src/data/` | the template engine and the typed view of `templates/data/*.yaml` |
| `templates/` | every artifact as a MiniJinja template, plus the data personas are drawn from |

The [architecture chapter](https://koval01.github.io/bot-cooker/development/architecture.html)
of the documentation walks through a request end to end.

## Making changes

### Content (most contributions)

Most of what a scanner sees is data and templates, not Rust:

- word lists, names, versions, stacks and servers: `templates/data/*.yaml`;
- files, pages and logs: `templates/**/*.j2`;
- which static files a stack links to: `templates/data/assets.yaml`.

Rules for content:

- Use only versions that were really released; the version tests reject
  impossible series, but they cannot know whether `1.24.1` ever shipped.
- Templates that reproduce real software output verbatim belong in the
  directories listed in `VERBATIM_DIRS` in `src/engine/audit.rs`. Everything
  else must vary between instances; the audit test fails when a line of our
  own making appears on more than 15% of 40 rendered instances.
- Randomness comes from the persona and the world (`Persona::rng`,
  `World::fixed_rng`, `cfg_rng`, `log_rng`, the template `choice` filter keyed
  with `persona_seed`), never from a fresh RNG or the clock.

### Adding a stack, a front server or a new kind of artifact

Today these need a few coordinated edits in Rust as well as data. Until the
planned restructuring lands (see "Roadmap for extensibility" below), follow
the existing examples:

- **Stack**: a variant in `Stack` (`src/persona/model.rs`), an entry in
  `templates/data/stacks.yaml`, `templates/data/env/<stack>.yaml`, a schema in
  `schemas.yaml` if it differs, an entry in `assets.yaml`, templates under
  `templates/stacks/<stack>/` and `templates/logs/app/<stack>.log.j2`. Then
  search `src/` for `stack.id()` to find the few places with stack-specific
  behaviour.
- **Front server**: a variant in `Frontend`, an entry in
  `templates/data/servers.yaml` including its `wire` facts, and
  `templates/servers/<server>/{error,autoindex,redirect}.html.j2`, captured
  from a real installation you run yourself. Add it to the `frontends` of the
  stacks it can serve.
- **Artifact**: a variant in `Profile` (`src/config.rs`), a module in
  `src/render/profiles/`, its branch in `render::render`, its class in
  `render/methods.rs`, and a place in `Persona::routes`.

Adding a stack, server or weight changes which persona existing master keys
get. Say so in the pull request; operators will see their instances change on
upgrade.

### Roadmap for extensibility

The direction agreed for the codebase, in small reviewable steps:

1. guard rails: `clippy.toml` disallowed methods (no clocks or fresh RNGs in
   content code, no outbound sockets), `cargo-deny`, golden snapshots of
   personas for fixed keys, a test that scans all output for harmful content;
2. stack and server behaviour expressed as data instead of `match` on ids;
3. a generic `file` profile with a template per route, replacing single-file
   profiles;
4. a profile registry, so metadata (static or not, boilerplate, streaming)
   lives in one place;
5. one directory per stack and per server, each with its own manifest;
6. string ids with rendezvous hashing, so adding an option moves as few
   existing personas as possible;
7. **packs** — directories of templates and data with a manifest — as the
   only form of third-party plugin. Packs carry no executable code by design.

If you want to work on one of these, open an issue first so the steps stay
coordinated.

## Pull requests

- One topic per pull request; keep refactors separate from behaviour changes.
- `cargo fmt --check`, `cargo clippy --all-targets` (no warnings) and
  `cargo test` pass. CI runs them on Linux and Windows.
- New behaviour comes with tests: stable for one key, different across keys,
  and realistic where a real reference exists.
- Commit messages: an imperative subject line that says what changes for the
  user of the trap, then a body explaining why.
- Update the documentation in `docs/` and `CHANGELOG.md` when behaviour or
  configuration changes.
- Found a way to tell a trap from a real server? Report it privately (see
  [SECURITY.md](https://github.com/koval01/bot-cooker/blob/main/SECURITY.md)) before opening a pull request that fixes it.

## License of contributions

bot-cooker is licensed under Apache-2.0. Unless you state otherwise, any
contribution you submit is licensed under the same terms (section 5 of the
license), without additional conditions.

Everyone taking part is expected to follow the
[Code of Conduct](https://github.com/koval01/bot-cooker/blob/main/CODE_OF_CONDUCT.md).
