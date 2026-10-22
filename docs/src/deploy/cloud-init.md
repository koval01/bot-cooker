# One-click with cloud-init

Most providers (Hetzner, DigitalOcean, AWS, Google Cloud, Oracle Cloud,
Vultr, Linode…) accept **user data** when you create a server. With it, a new
server installs the trap on its first boot and needs no login at all.

1. Copy [`deploy/cloud-init.yaml`](https://github.com/koval01/bot-cooker/blob/main/deploy/cloud-init.yaml)
   and replace `trap.example.com` with your trap hostname:

   ```yaml
   #cloud-config
   runcmd:
     - curl -fsSL https://raw.githubusercontent.com/koval01/bot-cooker/main/deploy/install.sh | bash -s -- --yes --domain trap.example.com
   ```

2. Create the server with a supported Linux image and paste the file into the
   *user data* / *cloud-init* field.
3. Allow inbound TCP 80 and 443 in the provider's firewall or security group.
4. Point the hostname's DNS A/AAAA record at the new server's public address
   (DNS only, not proxied).

Add `--auto-update` to the options if the server should install new releases
by itself. The installer keeps retrying the HTTPS check for a few minutes; if DNS takes
longer, Caddy obtains the certificate as soon as the record resolves. On the
server, `/var/log/cloud-init-output.log` shows what happened.

To manage the server later, log in and re-run the installer as in the
[quick start](quick-start.md), or add it to your [fleet](fleet.md).
