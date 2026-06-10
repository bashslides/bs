#!/usr/bin/env bash
#
# Set up everything needed to build and test `bs`:
#   1. Rust (rustc + cargo) via rustup, if cargo is missing.
#   2. A C linker — Rust links with the system `cc`. Uses, in order:
#        a. an existing system `cc`               (nothing to do)
#        b. apt `build-essential`                 (when you have root)
#        c. a self-contained gcc under ~/toolchain (no root needed)
#
# For case (c) the script also writes ~/toolchain/env.sh; source it before
# building:  source ~/toolchain/env.sh && cargo test
#
# Idempotent: safe to re-run.
set -euo pipefail

PREFIX="$HOME/toolchain"
ENV_FILE="$PREFIX/env.sh"
GCC_VER=13
TRIPLE=x86_64-linux-gnu              # gcc multiarch triple (package/dir names)
RUST_TARGET=x86_64-unknown-linux-gnu # Rust target triple (cargo env var)

have() { command -v "$1" >/dev/null 2>&1; }
say()  { printf '==> %s\n' "$*"; }

# --- 1. Rust ------------------------------------------------------------------
if ! have cargo && [ ! -x "$HOME/.cargo/bin/cargo" ]; then
  say "Installing Rust via rustup"
  curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal
fi
# shellcheck disable=SC1091
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
say "Rust: $(rustc --version 2>/dev/null || echo 'on PATH after restarting your shell')"

# --- 2. C linker --------------------------------------------------------------
if have cc; then
  say "System C linker present ($(command -v cc)) — done."
  exit 0
fi

if have apt-get && { [ "$(id -u)" = 0 ] || have sudo; }; then
  say "Installing build-essential via apt"
  if [ "$(id -u)" = 0 ]; then apt-get install -y build-essential
  else sudo apt-get install -y build-essential; fi
  say "Done."
  exit 0
fi

# --- 2c. No root: unpack a local gcc -----------------------------------------
say "No system linker and no root — unpacking a local gcc into $PREFIX"
have apt-get || { echo "error: apt-get is required for the no-root path" >&2; exit 1; }
have dpkg-deb || { echo "error: dpkg-deb is required for the no-root path" >&2; exit 1; }

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# Real binaries live in the arch-qualified packages; the bare gcc-13/cpp-13/
# binutils packages are just symlinks, and several support libs are separate.
pkgs="
  gcc-${GCC_VER} cpp-${GCC_VER}
  gcc-${GCC_VER}-${TRIPLE} cpp-${GCC_VER}-${TRIPLE}
  binutils binutils-${TRIPLE} binutils-common libbinutils
  libc6-dev libc-dev-bin linux-libc-dev libgcc-${GCC_VER}-dev libcrypt-dev
  libisl23 libmpc3 libmpfr6 libgmp10 zlib1g libsframe1 libctf0
"

say "Downloading packages"
( cd "$work" && apt-get download $pkgs )

say "Extracting into $PREFIX"
mkdir -p "$PREFIX"
for d in "$work"/*.deb; do dpkg-deb -x "$d" "$PREFIX"; done

# `cc` wrapper: points the real gcc driver at the unpacked cc1, as, ld, crt
# objects, headers and support libs, and falls back to the system runtime libc.
mkdir -p "$PREFIX/bin"
cat > "$PREFIX/bin/cc" <<EOF
#!/bin/sh
export LD_LIBRARY_PATH="$PREFIX/usr/lib/$TRIPLE:$PREFIX/usr/lib/gcc/$TRIPLE/$GCC_VER:\${LD_LIBRARY_PATH:-}"
exec "$PREFIX/usr/bin/$TRIPLE-gcc-$GCC_VER" \\
  -B "$PREFIX/usr/bin" \\
  -B "$PREFIX/usr/lib/gcc/$TRIPLE/$GCC_VER" \\
  -B "$PREFIX/usr/lib/$TRIPLE" \\
  -isystem "$PREFIX/usr/include" \\
  -isystem "$PREFIX/usr/include/$TRIPLE" \\
  -L "$PREFIX/usr/lib/gcc/$TRIPLE/$GCC_VER" \\
  -L "$PREFIX/usr/lib/$TRIPLE" \\
  -L /usr/lib/$TRIPLE \\
  -L /lib/$TRIPLE \\
  "\$@"
EOF
chmod +x "$PREFIX/bin/cc"

# Smoke-test the wrapper before declaring success.
say "Verifying the linker"
printf 'int main(void){return 0;}\n' > "$work/t.c"
"$PREFIX/bin/cc" "$work/t.c" -o "$work/t"
"$work/t"

# Env file to source for building.
cat > "$ENV_FILE" <<EOF
# Source this before building bs:  source ~/toolchain/env.sh
[ -f "\$HOME/.cargo/env" ] && . "\$HOME/.cargo/env"
export PATH="$PREFIX/bin:\$PATH"
export CARGO_TARGET_$(printf '%s' "$RUST_TARGET" | tr 'a-z-' 'A-Z_')_LINKER="$PREFIX/bin/cc"
EOF

say "Local toolchain ready. To build:"
echo "      source $ENV_FILE && cargo test"
