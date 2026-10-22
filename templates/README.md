# Template catalog

Everything bot-cooker serves is rendered from this tree by one MiniJinja
environment (`src/catalog`). The tree is embedded into the binary at build
time; set `deception.templates_dir` to overlay files from disk.

- `*.j2` — templates. Names ending in `.html.j2` / `.xml.j2` are HTML-escaped,
  everything else is written verbatim. Whitespace: `trim_blocks`,
  `lstrip_blocks`, trailing newline kept. Undefined variables are errors.
- `*.yaml` — data the persona is derived from (word lists, stack and server
  facts).

Filters available on top of MiniJinja's built-ins: `quote(mode)`, `php`,
`sql(dialect)`, `strftime(fmt, offset_min)`, `ljust(n)`, `rjust(n)`,
`human_size(style)`, `hexnum`, `base36`, `chunks(n)`.

## Layout

- `data/` — what personas and records are drawn from: `stacks.yaml`,
  `servers.yaml`, `infra.yaml`, `names.yaml` (by locale), `agents.yaml`
  (browser release cadences), `probes.yaml` (scanner request parts),
  `schemas.yaml`, `env/*.yaml` (`.env` layouts), `php_errors.yaml` (what PHP
  prints with `display_errors` on), `words.yaml`, `notes.yaml`,
  `synonyms.yaml` and `corpus/*.txt` (training text for the prose model).
- `servers/<frontend>/`, `stacks/<stack>/` — per-layer pages (errors, login,
  autoindex, `OPTIONS` answers…); `stacks/default/` is the fallback.
- `site/`, `pages/`, `sql/`, `logs/`, `api/`, `vcs/`, `secrets/`, `common/` —
  the artifacts themselves.
- `php/` — PHP's own error output, put in front of (or into) the pages of
  personas that display errors.
- `assets/` — the stylesheets and scripts pages link to. Which files a stack
  has, and how their hashed names look, is `data/assets.yaml`; the persona
  fixes the names once, so pages, access logs and the route table agree.
- `pack/` — prompts of the offline `cooker-pack` tool (not served).

An overlay file replaces the embedded file with the same path; data files are
validated at startup, so a broken overlay fails loudly instead of serving
half-rendered pages. `cargo run -- --check` loads the catalog and prints the
persona.
