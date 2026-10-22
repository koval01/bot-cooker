# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
uses [Semantic Versioning](https://semver.org/) (pre-1.0: minor versions may
change behaviour and configuration).

## [Unreleased]

## [0.1.0] - 2026-10-23

First public release.

### Trap

- One persona per instance, derived from a secret master key: organisation,
  staff, one of ten application stacks, a front server that fits the stack and
  the host OS, real released versions, file-formatting habits and timing.
- Decoy artifacts rendered from MiniJinja templates and YAML data embedded in
  the binary: leaked configuration in each stack's format, `.git` metadata,
  SQL dumps per dialect, chronological access/error/application logs,
  stack-shaped JSON APIs, login pages, `phpinfo`, `server-status`, sitemaps,
  feeds and a browsable site with directory listings.
- Content is a deterministic function of key, path and epoch, drifting
  between configuration and log epochs; nothing is stored on disk.
- HTTP/1.1 spoken in each front server's dialect: error pages, header order
  and spelling, keep-alive rules, answers to `HEAD`, `OPTIONS`, unknown
  methods and malformed requests.
- Pacing that looks like a real application on a weak server and a poor
  link: headers after a few hundred milliseconds, then a body whose chunk
  sizes and gaps change between regimes within a response, with
  retransmission-like hiccups and bursts, and no silence longer than
  `max_idle_ms`; aborts at different stages, occasional fast answers; static
  files and `robots.txt` at once. All distributions are per instance.
- Stylesheets, scripts and `favicon.ico` linked from pages are served under
  names in each toolchain's hash format, consistent with the access logs.
- Some misconfigured PHP personas print PHP errors into their pages,
  consistent with their configuration, phpinfo and logs.
- Token-protected JSON stats endpoint and periodic log reports; forwarding
  headers trusted only from a local proxy.
- Fingerprint audit in the test suite: no line of the project's own making
  may appear on many instances.

### Deployment

- One-command interactive installer in English and Russian
  (`deploy/install.sh`): checks the server, the hostname's DNS and the
  firewall, installs and configures Docker on Ubuntu, Debian, Fedora, RHEL
  clones, Amazon Linux, openSUSE, Arch and Alpine, adds swap and network
  tuning where useful, runs the trap behind Caddy with automatic HTTPS,
  reports every change it made, and installs `bot-cooker-ctl`.
- Opt-in automatic updates from GitHub releases, with checksum verification
  and rollback; cloud-init user data and a multi-node tool
  (`deploy/fleet.sh`).
- Static release binaries for Linux (x86-64, ARM64) and Windows, a Windows
  service installer, and a multi-arch container image on GHCR.
- Offline content packs (`cooker-pack`) and `templates_dir` /
  `COOKER_TEMPLATES_DIR` overlays.

### Project

- Apache-2.0 licence with a NOTICE file, documentation site, code of conduct,
  contributing guide and security policy.

[Unreleased]: https://github.com/koval01/bot-cooker/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/koval01/bot-cooker/releases/tag/v0.1.0
