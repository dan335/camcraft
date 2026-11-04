# Making a release

Releases carry builds for Windows and Linux, x86-64, made by hand from a
Windows machine with WSL's Ubuntu (glibc 2.39). macOS builds from source.

1. **Version.** Set `version` in the workspace `Cargo.toml`, and each internal
   crate's `version` under `[workspace.dependencies]`, and the README's
   "Version" line. Commit as "Release X.Y.Z".
2. **Gates.** `scripts/check.sh` (or `.\scripts\check.ps1`), all green.
3. **Licences.** With [cargo-about](https://github.com/EmbarkStudios/cargo-about)
   (`cargo install --locked cargo-about --features cli`), from the repository's
   root, once for each platform's target:

   ```sh
   for t in x86_64-pc-windows-msvc x86_64-unknown-linux-gnu; do
       cargo about generate -c scripts/release/about.toml --workspace --target $t \
           scripts/release/about.hbs -o "$OUT/THIRD-PARTY-$t.txt"
   done
   ```

4. **Windows.** `cargo build --release --locked -p camcraft-cli -p camcraft-gui`,
   then, from Git Bash:

   ```sh
   scripts/release/package.sh X.Y.Z "$OUT" target "$OUT/THIRD-PARTY-x86_64-pc-windows-msvc.txt"
   ```

5. **Linux**, in WSL from a fresh clone of the release commit, so nothing built
   for Windows is reused:

   ```sh
   git clone /mnt/d/rust/camcraft ~/camcraft-release-src && cd ~/camcraft-release-src
   cargo build --release --locked -p camcraft-cli -p camcraft-gui
   scripts/release/package.sh X.Y.Z "$OUT" target "$OUT/THIRD-PARTY-x86_64-unknown-linux-gnu.txt"
   ```

   Its `camcraft-gui` needs no newer glibc than the README says:
   `objdump -T camcraft-gui | grep -o "GLIBC_[0-9.]*" | sort -V | tail -1`.

6. **Checksums,** in `$OUT`: `sha256sum camcraft-X.Y.Z-*.zip camcraft-X.Y.Z-*.tar.gz > SHA256SUMS`.
7. **Publish.** Push main, tag `vX.Y.Z` on the release commit (annotated,
   "camcraft X.Y.Z"), push the tag, then
   `gh release create vX.Y.Z --title "camcraft X.Y.Z" --notes-file NOTES --verify-tag`
   with the two archives and `SHA256SUMS`. The notes keep the use-at-your-own-risk
   warning, the downloads table, what each archive holds, and what changed.
