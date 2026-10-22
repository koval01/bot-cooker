# Ethics and responsible use

bot-cooker exists to protect infrastructure by making automated attacks more
expensive. That purpose sets clear limits, and the project keeps to them in
code, in documentation and in what it accepts from contributors.

## Principles

1. **Purely reactive.** The trap only answers connections that clients open
   to it. It never scans, never connects out, never follows up on a client.
2. **Time is the only cost.** A client that talks to a trap loses time and
   gets decoy data. It receives no exploits, malware, decompression bombs or
   anything else aimed at its machine, and the trap makes no attempt to
   identify or track the people behind it.
3. **Decoys are decoys.** Credentials, keys, names and records in responses
   are generated. They are not anyone's real data, and they do not lead to
   any real system.
4. **Your infrastructure only.** Run traps on hostnames and servers you
   control, never on someone else's address space or in front of a service
   other people rely on.

## Practical guidance

- **Use dedicated hostnames.** Nothing legitimate should live on a trap
  hostname. If you must run bot-cooker next to a real site, give it only
  junk paths and allowlist the IP ranges of your users and partners by CIDR.
- **Do not trap people you have a duty to.** If customers, colleagues or
  partners could plausibly reach the hostname, use a different one.
- **Keep it separate.** A trap should not share secrets, networks or
  credentials with real systems. The decoy data must never be derived from
  real data.
- **Logs and privacy.** bot-cooker keeps per-IP counters in memory only and
  writes nothing to disk except its own log (if you configure one). Treat
  what you collect from scanners as personal data where your jurisdiction
  says so, and keep only what you need.
- **Law and policy.** Tarpits are a long-established defensive technique, but
  rules differ between countries and between hosting providers. Check your
  provider's acceptable-use policy. This documentation is not legal advice.

## Reporting and contributing

If you find that the software can be used to harm anyone, or that it does
anything outside these principles, report it privately as described in the
[security policy](https://github.com/koval01/bot-cooker/blob/main/SECURITY.md).
Contributions that add retaliation, exploitation or tracking are not accepted;
see [Contributing](development/contributing.md).
