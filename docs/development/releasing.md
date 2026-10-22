# CI and releases

## Continuous integration

[`ci.yml`](https://github.com/stblassitude/vst-send-via-whip/blob/main/.github/workflows/ci.yml)
runs on every push and pull request:

- `cargo fmt --check` and `cargo clippy`;
- `cargo test` on Linux, macOS and Windows;
- the plugin bundles and the test sender, built on all three platforms. The
  bundles are kept as workflow artifacts for 14 days.

[`docs.yml`](https://github.com/stblassitude/vst-send-via-whip/blob/main/.github/workflows/docs.yml)
builds this documentation with `mkdocs build --strict` and publishes it to
GitHub Pages from `main`. In the repository settings, set *Pages → Source* to
*GitHub Actions*.

## Releases

[`release.yml`](https://github.com/stblassitude/vst-send-via-whip/blob/main/.github/workflows/release.yml)
runs when a tag starting with `v` is pushed:

```sh
git tag -a v0.2.0 -m "v0.2.0"
git push origin v0.2.0
```

The workflow then:

1. **Checks the tag:** it must have the form `vMAJOR.MINOR.PATCH`,
   optionally followed by `-suffix`.
2. **Sets the version** with `scripts/set-version.sh`. This is the version
   the VST3 and CLAP plugins report to hosts. On macOS it is also written into
   the bundles' `Info.plist`, which are then re-signed (ad hoc).
3. **Builds** Linux x64 (on Ubuntu 22.04), macOS universal (deployment
   target 11.0) and Windows x64.
4. **Packages** each platform as
   `whip-send-<version>-<platform>.zip`. Each archive contains both
   bundles, `whip-send-cli`, `README.md` and `LICENSE`.
5. **Creates the GitHub release:**
    - the release notes list the subjects of all commits since the previous
      `v*` tag, with a compare link (or all commits for the first release);
    - tags with a suffix, such as `v1.0.0-rc.1`, are marked as pre-releases.

The version in `Cargo.toml` only matters for local builds; releases always
take it from the tag. Commit subjects become the release notes, so write them
for readers of the changelog.
