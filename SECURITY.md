# Security policy

bot-cooker faces the internet by design, so its own security matters as much
as how convincing it is.

## Reporting

Report privately through GitHub's private vulnerability reporting:
**Security → Report a vulnerability**, or directly
<https://github.com/koval01/bot-cooker/security/advisories/new>.

Please do not open public issues, discussions or pull requests for anything
on the list below. You will get an acknowledgement within a week; fixes are
released as soon as they are ready, and you are credited in the advisory
unless you prefer otherwise.

## What to report privately

- **Vulnerabilities in the binary**: memory or CPU exhaustion that a single
  client can trigger beyond the configured limits, crashes or panics that
  take the process down, request smuggling or parsing bugs in `src/wire`,
  template injection, path traversal into the host's file system, leaks of
  the master key or admin token.
- **Behaviour the project promises never to have**: any outbound connection
  made by the trap, any response that could harm a client beyond wasting its
  time (exploit payloads, malware, decompression bombs, content aimed at the
  scanner's own machine).
- **Ways to tell a trap from a real server** ("fingerprints"): a header,
  timing pattern, file or phrase that is the same on every instance or that
  no real software produces. Published, such a finding helps exactly the
  people the project slows down, so it goes through the private channel too.
- Vulnerabilities in the deployment tooling (`deploy/`, the Docker image,
  the Windows service scripts).

## Supported versions

Only the latest release and the `main` branch receive fixes. The project is
pre-1.0; upgrade to the newest release before reporting.

## Scope and conduct

Test only against instances you run yourself. Scanning or attacking other
people's bot-cooker instances is out of scope and not something this project
will thank you for.
