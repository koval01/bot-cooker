# Quick start: one command

You need a Linux server you control (x86-64 or ARM64; 1 vCPU and 512 MiB of
RAM are enough) with ports **80** and **443** reachable from the internet,
and a hostname for the trap, e.g. `backend-node-1.example.org`. Then, on the
server:

```bash
curl -fsSL https://raw.githubusercontent.com/koval01/bot-cooker/main/deploy/install.sh | sudo bash
```

## What the installer does

It asks its questions in English or Russian and checks every answer:

1. **The server**: architecture, memory, disk, whether ports 80 and 443 are
   free (and which program holds them if not), and whether bot-cooker is
   already installed (then it offers to update or remove it).
2. **The mode**: HTTPS on the trap's own hostname (recommended), or plain HTTP
   on port 80 behind a proxy you already have.
3. **The hostname and its DNS**: it finds the server's public address, asks
   whether the hostname's A record points to it, and checks through a public
   resolver. A missing record, a record pointing elsewhere, Cloudflare
   proxying left on, or a stray AAAA record that would break the certificate
   are each explained with exactly what to set; you can fix it and re-check,
   install anyway, or switch to HTTP only.
4. **The provider's firewall**: it reminds you to allow inbound TCP 80 and
   443 in the cloud console, which cannot be seen from the server.
5. **Automatic updates**: offered, off unless you say yes (see below).
6. **A summary** of the settings and of every change it is about to make to
   the system. Nothing changes before you confirm.

Then it works the same way on every supported distribution:

| | |
|---|---|
| Docker | installed from the distribution's packages (Ubuntu, Debian, Fedora, Amazon Linux, openSUSE, Arch, Alpine), from Docker's own repository where the distribution has none (RHEL, Rocky, Alma, CentOS Stream), with the Compose plugin from its official releases (SHA-256 checked) where only that is missing; the service is enabled at boot |
| Firewall | ufw, firewalld, nftables or iptables, whichever is active, gets TCP 80/443 with rules marked `bot-cooker`, saved to survive a reboot |
| Swap | a swap file when the server has less than 2 GiB of memory and none |
| Network | `/etc/sysctl.d/90-bot-cooker.conf` raises connection queues and file limits for many long connections; values already higher are kept |
| The trap | `/opt/bot-cooker` with a fresh master key and admin token, the trap and Caddy containers with log rotation, the `bot-cooker-ctl` command |

Finally it waits for the certificate and the trap's first answer. If the
certificate fails it reads Caddy's log and tells you the cause: closed
firewall, DNS not found, DNS pointing elsewhere, or Let's Encrypt rate limits.
It ends with a guide (also in `/opt/bot-cooker/GUIDE.txt`) and a report of
everything it changed on the server.

## Afterwards

```bash
bot-cooker-ctl status            # running? persona, counters
bot-cooker-ctl stats             # statistics as JSON
bot-cooker-ctl logs              # the log (Ctrl+C to leave)
bot-cooker-ctl stop | start | restart
bot-cooker-ctl update            # install a newer release now
bot-cooker-ctl auto-update on    # or off
bot-cooker-ctl uninstall         # remove; --purge also deletes key and certificate
bot-cooker-ctl guide             # the guide again
```

`uninstall` also undoes the system changes (firewall rules, swap file,
network settings, update timer). Docker stays, since other programs may use
it.

## Automatic updates

With your consent, a systemd timer (or cron where there is no systemd) checks
this repository's releases once a day at a random time. A newer release is
installed by its own installer, whose SHA-256 is checked against the
release's `SHA256SUMS` first; the key, token, certificate and persona stay. If
the new version does not come up, the previous one is restored. Every check
is logged in `/opt/bot-cooker/update.log`. Pre-releases are never installed.

## Unattended installation

Every question has an option, and `--yes` takes the given options plus the
answers of a previous installation:

```bash
curl -fsSL https://raw.githubusercontent.com/koval01/bot-cooker/main/deploy/install.sh \
  | sudo bash -s -- --yes --domain backend-node-1.example.org --auto-update
```

| Option | Effect |
|---|---|
| `--domain NAME` | HTTPS for this hostname |
| `--http-only` | plain HTTP on port 80, no certificate |
| `--lang en\|ru` | language of messages |
| `--auto-update` / `--no-auto-update` | automatic updates on or off |
| `--version X.Y.Z` | a specific release instead of the latest |
| `--repo OWNER/NAME` | releases and image of a fork |
| `--source DIR` | build the image from a source checkout on the server |
| `--pack DIR` | serve a [content pack](../operate/content-packs.md) |
| `--skip-dns-check` | do not verify the hostname's DNS |

Alpine needs `bash` first (`apk add bash`). Prefer to read the script before
running it? Download it, read it, then run `sudo bash install.sh`.
