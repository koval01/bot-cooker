# bot-cooker

**A defensive HTTP tarpit that makes automated scanning expensive.**

bot-cooker runs on a hostname of your own and answers the scripts that sweep
the internet for leaked `.env` files, open `.git` directories, database dumps
and admin panels. It answers them slowly, with decoy content convincing enough
to keep them reading, so each attempt costs minutes of their time and returns
nothing of value.

[Documentation](https://koval01.github.io/bot-cooker/) ·
[Quick start](https://koval01.github.io/bot-cooker/deploy/quick-start.html) ·
[How it works](https://koval01.github.io/bot-cooker/how-it-works.html) ·
[Contributing](CONTRIBUTING.md)

## Philosophy

Automated attacks are cheap. A scanner tries the same few hundred paths on
every address it finds, and a blocked request costs it nothing: it simply moves
on to the next target. Defence that only says "no" leaves that economy intact.

bot-cooker takes the opposite approach. It **protects infrastructure by raising
the cost of attacking it**: every request a scanner sends to a trap holds one
of its connections for minutes and yields only decoys. Spread across many
traps, that is less time and money for indiscriminate attacks against everyone.

It does this within strict limits, the same ones defensive security work has
always kept:

- **We defend, we do not attack.** The trap never scans, never connects out,
  never "hacks back". It only answers connections that clients chose to open.
- **Time is the only cost.** Responses waste a scanner's time and nothing
  else: no exploits, no malware, nothing aimed at anyone's machine, no attempt
  to track the people behind a scanner.
- **Decoys are decoys.** Every credential, record and name is generated; none
  is anyone's real data or leads to a real system.
- **Your own infrastructure only.** Traps belong on servers and hostnames you
  control and that nobody depends on.

Read more in [Ethics and responsible use](https://koval01.github.io/bot-cooker/ethics.html).

## What a scanner meets

Each instance derives a whole imaginary organisation from a secret key: a
company, its staff, an application stack (Laravel, Symfony, Django, Rails,
Express, Spring Boot, ASP.NET Core, WordPress, Joomla or Drupal), a web server
(nginx, Apache, OpenResty, Caddy, IIS or LiteSpeed), real released versions,
habits and timing. Everything it serves is rendered from that persona, so the
pieces agree with each other, and two instances look like two unrelated
systems:

- leaked configuration in the stack's own format, `.git` metadata, SQL dumps
  in the right dialect, chronological logs, stack-shaped JSON APIs;
- login pages, `phpinfo`, version files, sitemaps, feeds, and a website whose
  pages and directory listings lead ever deeper;
- the persona's server behaviour: its 404 and 403 pages, header order and
  spelling, answers to `HEAD`, `OPTIONS` and malformed requests;
- a slow, irregular trickle whose pace, chunk sizes and cut-offs differ per
  instance and change during a response.

Content is a deterministic function of the key, the path and the current
epoch: the same file reads the same within an epoch and drifts between epochs
like a maintained system. Nothing is stored on disk, nothing leaves the host.

## Quick start

On a fresh Linux server you own:

```bash
curl -fsSL https://raw.githubusercontent.com/koval01/bot-cooker/main/deploy/install.sh | sudo bash
```

The installer talks you through it, in English or Russian: it checks the
server, asks for the trap's hostname and verifies that its DNS really points
here (and explains what to fix if not), installs and configures Docker on
Ubuntu, Debian, Fedora, RHEL-likes, Amazon Linux, openSUSE, Arch or Alpine,
opens the host firewall, adds swap and network tuning where useful, starts
the trap behind Caddy with an automatic Let's Encrypt certificate, and ends
with a plain-language guide and a report of everything it changed. Automatic
updates from this repository's releases are offered, and off unless you say
yes. Afterwards, `bot-cooker-ctl` shows status and statistics, stops, starts,
updates and removes it.

Other ways to deploy: [cloud-init one-click](https://koval01.github.io/bot-cooker/deploy/cloud-init.html),
[several nodes from your workstation](https://koval01.github.io/bot-cooker/deploy/fleet.html),
[Docker Compose](https://koval01.github.io/bot-cooker/deploy/docker.html),
[behind an existing proxy](https://koval01.github.io/bot-cooker/deploy/behind-proxy.html),
[Windows Server](https://koval01.github.io/bot-cooker/deploy/windows.html).

To try it locally:

```bash
cargo run                                  # http://localhost:8080
curl http://localhost:8080/robots.txt      # what this instance "hides"
```

## Building

```bash
cargo build --release      # target/release/bot-cooker
cargo test                 # unit, fingerprint-audit, HTTP and process tests
cargo clippy --all-targets
```

The Rust toolchain is pinned in `rust-toolchain.toml`. Linux, macOS and
Windows are supported; releases ship static binaries for Linux (x86-64,
ARM64) and Windows, and a multi-arch image at `ghcr.io/koval01/bot-cooker`.

## Contributing

Contributions are welcome — realism fixes, new stacks and servers, deployment
improvements, documentation. Start with [CONTRIBUTING.md](CONTRIBUTING.md); it
explains the principles every change must keep and how the code is laid out.
Found a way to tell a trap from a real server, or a security problem? Report
it privately: [SECURITY.md](SECURITY.md).

## License and attribution

bot-cooker is free software under the [Apache License 2.0](LICENSE). Anyone
may use it, anywhere, for any purpose — individuals, companies, public
institutions.

Forks and derivative works are welcome. Please keep it decent: preserve the
[NOTICE](NOTICE) file as the license requires, and keep a visible link back
to the original project, <https://github.com/koval01/bot-cooker>.

Created by Yaroslav Koval.
