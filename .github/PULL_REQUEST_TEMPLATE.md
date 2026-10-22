## What and why

<!-- What changes for people running the trap, and why. Link the issue if there is one. -->

## Checklist

- [ ] `cargo fmt --check`, `cargo clippy --all-targets` (no warnings) and `cargo test` pass
- [ ] New behaviour has tests (stable for one key, different across keys)
- [ ] Content copies real software output and uses versions that were really released
- [ ] Nothing new connects out, and nothing in a response can harm a client
- [ ] Docs (`docs/`) and `CHANGELOG.md` are updated if behaviour or configuration changed
- [ ] If personas of existing keys change (new stack, server or weight), it is noted here
