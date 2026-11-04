#!/usr/bin/env bash
# Package a camcraft release for this platform from built executables, and
# smoke-test it: the archive unpacked, the command line run on the example job.
#
#   scripts/release/package.sh VERSION OUT_DIR TARGET_DIR LICENCES
#
# VERSION is the workspace's (0.3.0); TARGET_DIR holds release/camcraft and
# release/camcraft-gui (.exe on Windows); LICENCES is the THIRD-PARTY-LICENSES
# text for this platform (scripts/release/README.md). Writes
# camcraft-VERSION-PLATFORM.tar.gz on Linux, .zip on Windows, into OUT_DIR.
set -euo pipefail

version="$1"
out="$2"
target="$3"
licences="$4"
repo="$(cd "$(dirname "$0")/../.." && pwd)"

case "$(uname -s)" in
    Linux) platform=linux-x86_64; exe="" ;;
    MINGW* | MSYS* | CYGWIN*) platform=windows-x86_64; exe=".exe" ;;
    *) echo "no release is made for $(uname -s)" >&2; exit 1 ;;
esac
name="camcraft-$version-$platform"
stage="$out/$name"

rm -rf "$stage"
mkdir -p "$stage/examples" "$stage/fixtures/step" "$stage/licenses/fonts"
for bin in camcraft camcraft-gui; do
    cp "$target/release/$bin$exe" "$stage/"
done
if [ "$platform" = linux-x86_64 ]; then
    strip "$stage/camcraft" "$stage/camcraft-gui"
fi
cp "$repo/examples/rail-jig.camcraft" "$stage/examples/"
cp "$repo/fixtures/step/sled_base.step" "$stage/fixtures/step/"
cp "$repo/LICENSE-MIT" "$repo/LICENSE-APACHE" "$repo/NOTICE" "$repo/README.md" "$stage/"
fonts="$repo/crates/camcraft-gui/assets/fonts"
cp "$fonts/OFL-FiraCode.txt" "$fonts/OFL-IBMPlexSans.txt" "$fonts/LICENSE-Lucide.txt" \
    "$stage/licenses/fonts/"
cp "$licences" "$stage/THIRD-PARTY-LICENSES.txt"

# The binaries alone executable.
find "$stage" -type f ! -name "camcraft$exe" ! -name "camcraft-gui$exe" -exec chmod 644 {} +

cd "$out"
if [ "$platform" = linux-x86_64 ]; then
    archive="$name.tar.gz"
    rm -f "$archive"
    tar czf "$archive" "$name"
else
    archive="$name.zip"
    rm -f "$archive"
    powershell.exe -NoProfile -Command "Compress-Archive -Path '$name' -DestinationPath '$archive'"
fi
echo "packed $out/$archive"

# Unpacked somewhere new, the command line runs the example and its checks pass.
smoke="$out/smoke-$platform"
rm -rf "$smoke"
mkdir -p "$smoke"
cd "$smoke"
if [ "$platform" = linux-x86_64 ]; then
    tar xzf "$out/$archive"
else
    powershell.exe -NoProfile -Command "Expand-Archive -Path '$out/$archive' -DestinationPath '.'"
fi
cd "$name"
"./camcraft$exe" --version
"./camcraft$exe" job examples/rail-jig.camcraft 2>&1 | tail -3
