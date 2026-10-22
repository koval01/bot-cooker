# Content packs

The built-in prose and vocabularies are shared by every instance; the
procedural layer recombines them so no two instances read alike, but a
**content pack** makes the text itself unique to one instance.

A pack is a directory of templates and data laid over the built-in catalog
(`deception.templates_dir`). It contains no executable code: only MiniJinja
templates, YAML and text, checked when the trap starts.

## Generating one

`cooker-pack` runs **on your own machine**, never on a trap. It asks Claude
to write the prose, comment and commit vocabularies and the site theme for one
instance's persona, checks that the result loads and renders, and writes the
overlay directory:

```bash
export ANTHROPIC_API_KEY=...
COOKER_MASTER_KEY=<the instance key> \
  cargo run --release --features pack --bin cooker-pack -- --out packs/node-1
```

Only the fictional persona (company, industry, stack, staff first names) goes
into the prompts; the master key does not. `--dry-run` prints the prompts
without calling the API.

The trap binary is built without the `pack` feature and contains no HTTP
client.

## Installing one

Copy the directory to the server and point the configuration at it:

```yaml
deception:
  templates_dir: /path/to/pack
```

With the installer, put the directory on the server and pass it with
`--pack` (re-running the installer keeps the key):

```bash
sudo bash install.sh --domain trap.example.com --pack /opt/bot-cooker-pack
```

The pack is mounted read-only into the container and selected with
`COOKER_TEMPLATES_DIR`. Files in the pack replace the built-in files with the
same path; a broken pack fails at startup instead of serving half-rendered
pages.

## Writing one by hand

Any file under `templates/` can be overridden: copy it into the pack at the
same relative path and edit it. See [Templates and data](../development/templates.md)
for what lives where.
