# Windows Server

Download `bot-cooker-<version>-x86_64-pc-windows-msvc.zip` from the
[releases](https://github.com/koval01/bot-cooker/releases) (a static build:
no Visual C++ runtime needed), unpack it and, in an elevated PowerShell in
that folder:

```powershell
.\install-service.ps1 -Port 80 -OpenFirewall
```

The script:

- installs `bot-cooker.exe` into `C:\Program Files\bot-cooker`;
- writes `C:\ProgramData\bot-cooker\bot-cooker.yaml` with a freshly generated
  master key, readable only by Administrators, SYSTEM and the service;
- registers the `bot-cooker` service under `LOCAL SERVICE` with delayed
  automatic start and restart on failure;
- optionally opens the port in Windows Firewall, and starts the service.

Logs go to `C:\ProgramData\bot-cooker\bot-cooker.log`. Re-running the script
upgrades the binary and keeps the configuration; `uninstall-service.ps1`
removes the service, and `-Purge` also deletes the configuration and log.

## Why the persona is always IIS here

Scanners can tell the operating system from the TCP/IP stack (TTL, window
size, options). A Windows server claiming to be nginx on Ubuntu would give
itself away, so on Windows the persona is always IIS with ASP.NET Core, and
Linux traps never claim IIS. If connections terminate on a different OS than
the one bot-cooker runs on (Docker Desktop on Windows publishes Linux
containers through the Windows stack), set `deception.host_os`.

## HTTPS

bot-cooker speaks plain HTTP. For HTTPS, put a TLS-terminating proxy in front
that streams responses without buffering, e.g. Caddy for Windows or IIS with
Application Request Routing (with response buffering disabled). If IIS already
uses port 80 (`netstat -ano | findstr :80`), choose another port or server.
