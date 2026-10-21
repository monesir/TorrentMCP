#!/usr/bin/env bash
# Minimal static ffprobe for torrentx: http/tcp/file input, every native
# demuxer (stream probing inside containers needs the raw ones; torrentx's
# -format_whitelist limits the top level), parser, bsf and decoder, and
# swresample for the Opus decoder, and zlib for zlib-compressed Matroska tracks
# (otherwise "Unsupported encoding type"); no filters, encoders, muxers,
# devices or other external libraries. Not --enable-small: it drops profile
# names.
#
# Run in an MSYS2 UCRT64 shell with make, mingw-w64-ucrt-x86_64-gcc and
# mingw-w64-ucrt-x86_64-zlib (linked statically):
#   scripts/build-ffprobe.sh ffmpeg-9.0.2.tar.xz OUT_DIR
# Verify the tarball's signature (FFmpeg release key
# FCF986EA15E6E293A5644F10B4322F04D67658D8) first. OUT_DIR gets bin/ffprobe.exe,
# LICENSE and README.txt, the layout package-windows.ps1 -FFprobePath expects.
# OUT_DIR must be an ASCII path: MSYS2's strip rejects other characters.
set -euo pipefail
TARBALL=$(realpath "$1")
OUT=$(realpath -m "$2")
VERSION=$(basename "$TARBALL" .tar.xz)
VERSION=${VERSION#ffmpeg-}
BUILD=$(mktemp -d)
trap 'rm -rf "$BUILD"' EXIT
rm -rf "$OUT"
mkdir -p "$OUT/bin"
tar -xf "$TARBALL" -C "$BUILD"
cd "$BUILD/ffmpeg-$VERSION"
FLAGS=(
  --arch=x86_64 --target-os=mingw32
  --enable-static --disable-shared --extra-ldflags=-static
  --disable-debug --disable-doc
  --disable-autodetect --disable-everything
  --disable-programs --enable-ffprobe
  --disable-avdevice --disable-avfilter --disable-swscale
  --enable-network --enable-protocol=http,tcp,file
  --enable-demuxers
  --enable-parsers --enable-bsfs --enable-decoders
  --enable-zlib
  --disable-x86asm
)
./configure "${FLAGS[@]}" > "$OUT/configure.log" 2>&1 || { tail -30 "$OUT/configure.log"; exit 1; }
make -j"$(nproc)" ffprobe.exe > "$OUT/make.log" 2>&1 || { tail -30 "$OUT/make.log"; exit 1; }
strip -o "$OUT/bin/ffprobe.exe" ffprobe.exe
cp COPYING.LGPLv2.1 "$OUT/LICENSE"
{
  echo "ffprobe $VERSION, minimal static build for torrentx"
  echo "Source: https://ffmpeg.org/releases/ffmpeg-$VERSION.tar.xz"
  echo "Source SHA-256: $(sha256sum "$TARBALL" | cut -d' ' -f1)"
  echo "Signature: verified with the FFmpeg release signing key FCF986EA15E6E293A5644F10B4322F04D67658D8"
  echo "Compiler: $(gcc --version | head -1)"
  echo "License: LGPL v2.1 or later (no GPL or nonfree components enabled)"
  echo "Includes zlib $(pacman -Q mingw-w64-ucrt-x86_64-zlib | cut -d" " -f2) (zlib license, https://zlib.net/zlib_license.html), linked statically"
  echo "Configure: ${FLAGS[*]}"
  echo
  echo "Built from unmodified FFmpeg source; see https://ffmpeg.org/legal.html."
} > "$OUT/README.txt"
"$OUT/bin/ffprobe.exe" -version | head -1
ls -la "$OUT/bin/ffprobe.exe"
objdump -p "$OUT/bin/ffprobe.exe" | grep "DLL Name" | sort -u
