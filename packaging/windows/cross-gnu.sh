#!/usr/bin/env bash
# Cross-compile EffectCraft's GUI app and CLI for 64-bit Windows from Linux
# (mingw-w64 / x86_64-pc-windows-gnu). Used to produce a portable zip for users
# who do not have a Rust toolchain.
#
#   ./packaging/windows/cross-gnu.sh
#   DIST=/tmp/out ./packaging/windows/cross-gnu.sh
#   SKIP_BUILD=1 DIST=/tmp/out ./packaging/windows/cross-gnu.sh   # reuse already-built exes
#
# Needs: rustup target x86_64-pc-windows-gnu, gcc-mingw-w64-x86-64, zip.
# Optional: wine, to smoke-test effectcraft-cli.exe --version.
set -euo pipefail

Root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$Root"

Target=x86_64-pc-windows-gnu
Version="$(python3 - <<'PY'
import pathlib, re
text = pathlib.Path("Cargo.toml").read_text()
m = re.search(r'(?ms)^\[workspace\.package\].*?^version\s*=\s*"([^"]+)"', text)
print(m.group(1) if m else "0.0.0")
PY
)"
Dist="${DIST:-"$Root/dist/windows"}"
Stage="$Dist/effectcraft-$Version-windows-x64"
Zip="$Dist/effectcraft-$Version-windows-x64.zip"

echo "EffectCraft $Version → $Target"

if ! rustup target list --installed | grep -qx "$Target"; then
  rustup target add "$Target"
fi
if ! command -v x86_64-w64-mingw32-gcc >/dev/null; then
  echo "missing x86_64-w64-mingw32-gcc (Debian/Ubuntu: sudo apt install gcc-mingw-w64-x86-64 mingw-w64)" >&2
  exit 1
fi

# Prefer static libgcc/libstdc++ so the zip is less dependent on extra mingw DLLs.
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER="${CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER:-x86_64-w64-mingw32-gcc}"
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS="${CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS:--C link-arg=-static-libgcc -C link-arg=-static-libstdc++}"

if [[ "${SKIP_BUILD:-}" != "1" ]]; then
  cargo build --release --locked -p effectcraft -p effectcraft-cli --target "$Target"
fi

Bin="$Root/target/$Target/release"
test -f "$Bin/effectcraft.exe"
test -f "$Bin/effectcraft-cli.exe"

rm -rf "$Stage"
mkdir -p "$Stage"
cp "$Bin/effectcraft.exe" "$Bin/effectcraft-cli.exe" "$Stage/"

# Copy any leftover mingw runtime DLLs that still sit next to the exe (none if
# fully static). Also search the mingw sysroot.
copy_dll() {
  local name="$1"
  local found
  found="$(find /usr/lib/gcc/x86_64-w64-mingw32 /usr/x86_64-w64-mingw32 -name "$name" 2>/dev/null | head -n 1 || true)"
  if [[ -n "$found" ]]; then
    cp "$found" "$Stage/"
    echo "bundled $name from $found"
  fi
}
# If ldd-style listing is available via objdump, pick up needed mingw DLLs.
# grep exits 1 when none match (fully static libgcc); `|| true` keeps pipefail from aborting.
if command -v x86_64-w64-mingw32-objdump >/dev/null; then
  dlls="$(
    x86_64-w64-mingw32-objdump -p "$Bin/effectcraft.exe" "$Bin/effectcraft-cli.exe" \
      | awk '/DLL Name:/{print $3}' | sort -u | grep -iE 'libgcc|libstdc|libwinpthread|libssp' || true
  )"
  while IFS= read -r dll; do
    [[ -n "$dll" ]] && copy_dll "$dll"
  done <<< "$dlls"
fi

cp "$Root/packaging/windows/README-Windows.txt" "$Stage/"
for f in LICENSE-MIT LICENSE-APACHE NOTICE; do
  [[ -f "$Root/$f" ]] && cp "$Root/$f" "$Stage/"
done

# PE subsystem: 2 = GUI (effectcraft.exe), 3 = console (effectcraft-cli.exe).
python3 - <<PY
import pathlib, struct, sys
def pe(path):
    data = pathlib.Path(path).read_bytes()
    pe = struct.unpack_from('<I', data, 0x3C)[0]
    machine = struct.unpack_from('<H', data, pe + 4)[0]
    subsystem = struct.unpack_from('<H', data, pe + 0x5C)[0]
    return machine, subsystem
for name, want_sub in [('effectcraft.exe', 2), ('effectcraft-cli.exe', 3)]:
    machine, sub = pe('$Stage/' + name)
    if machine != 0x8664:
        sys.exit(f'{name} is not x64 (machine 0x{machine:x})')
    if sub != want_sub:
        sys.exit(f'{name} has PE subsystem {sub}, expected {want_sub}')
    print(f'ok {name}: x64, PE subsystem {sub}')
PY

mkdir -p "$Dist"
rm -f "$Zip"
# Zip from inside Stage so extract is a single folder.
( cd "$Dist" && zip -r -9 "$(basename "$Zip")" "$(basename "$Stage")" )
ls -lh "$Zip"
echo "ZIP=$Zip"

if command -v wine >/dev/null; then
  echo "==> wine smoke-test (CLI)"
  wine "$Stage/effectcraft-cli.exe" --version || echo "wine CLI --version failed (see above); the zip was still built."
fi
