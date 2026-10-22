# Upgrades and keys

## The master key is the instance

Everything a scanner sees is derived from the master key. Keep it:

- **secret** — whoever has it can predict every decoy the instance serves;
- **stable** — a new key turns the trap into a different organisation
  overnight, which is not how real servers behave;
- **unique** — two hosts with the same key serve the same organisation, the
  clearest possible sign of a trap.

The installer generates the key once and keeps it in
`/opt/bot-cooker/.env` across upgrades. Back that file up if you want to be
able to rebuild the same instance on another server.

## Upgrading

```bash
bot-cooker-ctl update            # one server, now
bot-cooker-ctl auto-update on    # one server, daily, by itself
deploy/fleet.sh deploy           # a fleet
```

Automatic updates are off unless enabled at installation or with
`auto-update on`; see [Quick start](../deploy/quick-start.md#automatic-updates).

Read the [changelog](https://github.com/koval01/bot-cooker/blob/main/CHANGELOG.md)
first. Releases that add stacks, servers or new realism can change what an
existing key's persona looks like; the changelog says so when it happens.
Where scanners already know an instance, a fresh key on a fresh hostname is
often better than a visible change of identity.

Pin a release with `--version X.Y.Z` to upgrade on your own schedule.

## Rotating identities on purpose

`deception.persona_era_days: N` makes an instance become a new organisation
every N days, as if the server had been reinstalled for a new owner. Leave it
at `0` unless you want that.

## Moving to another server

Copy `/opt/bot-cooker/.env` to the new server, point DNS at it and run the
installer there: the persona, the admin token and (after re-issue) the
certificate are the same as before.
