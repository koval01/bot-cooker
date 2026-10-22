# Releasing

Releases are cut from `main` by pushing a version tag.

1. Make sure CI is green on `main` and `CHANGELOG.md` has an entry for the
   version: move the *Unreleased* items under a new `## [X.Y.Z] - YYYY-MM-DD`
   heading and add the comparison link at the bottom.
2. Bump `version` in `Cargo.toml` and run `cargo build` so `Cargo.lock`
   follows. Commit both with the changelog.
3. Tag and push:

   ```bash
   git tag -a vX.Y.Z -m "bot-cooker X.Y.Z"
   git push origin vX.Y.Z
   ```

The `Release` workflow then:

- builds static binaries for Linux x86-64 and ARM64 (musl) and Windows
  x86-64, with `README.md`, `LICENSE`, `NOTICE` and the sample configuration,
  and attaches them with a `SHA256SUMS` file to a GitHub release;
- builds the container image natively on x86-64 and ARM64 runners and
  publishes `ghcr.io/koval01/bot-cooker:X.Y.Z` and `:latest`.

After the first release, set the package's visibility to **public** in the
repository's *Packages* settings; the one-line installer pulls the image
without logging in.

If a release changes what existing keys' personas look like, say so at the
top of its changelog entry.
