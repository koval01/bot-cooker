# bot-cooker

bot-cooker is a defensive HTTP **tarpit**: a small server you run on a
hostname of your own, which answers unsolicited scanners slowly and with
convincing decoy content. Automated tools that sweep the internet for leaked
`.env` files, exposed `.git` directories, database dumps and admin panels
spend their time, connections and proxy budget on it, and leave with nothing
of value.

## Why a tarpit

Most of the hostile traffic a public server sees is automated: scripts that
try the same few hundred paths on every address they can find, at a cost
close to zero per target. A firewall rule or a `403` tells such a script to
move on immediately; its cost stays near zero.

A tarpit changes the economics instead. Each request that reaches it is held
open and answered at a trickle with material that looks worth reading, so the
scanner keeps a slot busy for minutes instead of milliseconds, and whatever it
collects is worthless. Spread over many traps, that raises the cost of
indiscriminate scanning for everyone who runs one, and gives the scanner's
operator less time for real targets.

## What it is not

- **Not a weapon.** bot-cooker never connects anywhere, never scans, never
  "hacks back". It only answers clients that chose to connect to it.
- **Not harmful to clients.** Responses waste time; they carry no exploits,
  no malware, nothing aimed at the client's machine.
- **Not a replacement for securing real systems.** Run it on hostnames that
  serve nothing else. It complements patching, firewalls and monitoring of
  your real services; it does not protect them by itself.

## What makes it convincing

A trap is only useful while scanners cannot tell it from a real, careless
server. bot-cooker therefore derives a whole imaginary organisation from a
secret per-instance key — company, domain, staff, application stack, web
server, versions, formatting habits, timing — and renders every response from
it: configuration files in the stack's own format, git metadata, SQL dumps in
the right dialect, chronological logs, login pages, JSON APIs, a browsable
website. Two instances with different keys look like two unrelated systems.
See [How it works](how-it-works.md).

## Where to start

- To run a trap: [Quick start](deploy/quick-start.md) — one command on a
  fresh Linux server.
- To understand the design: [How it works](how-it-works.md) and
  [Ethics and responsible use](ethics.md).
- To contribute: [Architecture](development/architecture.md) and
  [Contributing](development/contributing.md).

bot-cooker is free software under the Apache License 2.0. Anyone may use it,
anywhere; forks keep the NOTICE file that names the original project.
