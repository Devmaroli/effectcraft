#!/usr/bin/env bash
# Stage the office-friendly Windows x64 portable zip from already-built
# (usually mingw) binaries, and compile effectcraft-Setup-x64.exe when
# Inno Setup is available (native ISCC on Windows, or ISCC under Wine).
#
#   ./packaging/windows/cross-gnu.sh          # build the exes + zip
#   ./packaging/windows/package-office.sh     # rename/stamp portable + installer
#   SKIP_BUILD=1 DIST=/tmp/out ./packaging/windows/package-office.sh
#
# Needs: zip. Optional: wine + Inno Setup 6, or ISCC on PATH.
set -euo pipefail

Root="$(cd "$(dirname "$0")/../.." && pwd)"
# shellcheck source=../env.sh
. "$Root/packaging/env.sh"

Target="${OFFICE_TARGET:-x86_64-pc-windows-gnu}"
Bin="${OFFICE_BIN:-$CARGO_TARGET_DIR/$Target/release}"
OfficeDist="${OFFICE_DIST:-$Root/dist/office}"
mkdir -p "$OfficeDist"

if [[ ! -f "$Bin/effectcraft.exe" || ! -f "$Bin/effectcraft-cli.exe" ]]; then
  echo "missing $Bin/effectcraft.exe or effectcraft-cli.exe (build first, or set OFFICE_BIN)" >&2
  exit 1
fi

Stage="$OfficeDist/effectcraft"
rm -rf "$Stage"
mkdir -p "$Stage"
cp "$Bin/effectcraft.exe" "$Bin/effectcraft-cli.exe" "$Stage/"
cp "$Root/packaging/windows/portable.txt" "$Stage/"
cp "$Root/packaging/windows/README-Windows.txt" "$Stage/"
for f in LICENSE-MIT LICENSE-APACHE NOTICE; do
  [[ -f "$Root/$f" ]] && cp "$Root/$f" "$Stage/"
done

# Copy leftover mingw runtime DLLs (none if fully static). They may sit next to
# the built exes or in the folder staging from cross-gnu.sh.
shopt -s nullglob
for dll in "$Bin"/*.dll "$Root/dist/windows/effectcraft-$VERSION-windows-x64"/*.dll; do
  cp "$dll" "$Stage/"
done
shopt -u nullglob

Zip="$OfficeDist/effectcraft-Portable-x64.zip"
rm -f "$Zip"
( cd "$OfficeDist" && zip -r -9 "$(basename "$Zip")" effectcraft )
echo "ZIP=$Zip"

find_iscc() {
  if [[ -n "${ISCC:-}" && -e "$ISCC" ]]; then
    echo "$ISCC"
    return 0
  fi
  if command -v iscc >/dev/null; then
    command -v iscc
    return 0
  fi
  local p
  for p in \
    "/c/Program Files (x86)/Inno Setup 6/ISCC.exe" \
    "/mnt/c/Program Files (x86)/Inno Setup 6/ISCC.exe" \
    "$HOME/.wine/drive_c/Program Files (x86)/Inno Setup 6/ISCC.exe"; do
    if [[ -f "$p" ]]; then
      echo "$p"
      return 0
    fi
  done
  if [[ -n "${WINEPREFIX:-}" && -f "$WINEPREFIX/drive_c/Program Files (x86)/Inno Setup 6/ISCC.exe" ]]; then
    echo "$WINEPREFIX/drive_c/Program Files (x86)/Inno Setup 6/ISCC.exe"
    return 0
  fi
  return 1
}

wine_path() {
  python3 - "$1" <<'PY'
import os, sys
p = os.path.abspath(sys.argv[1])
print("Z:" + p.replace("/", "\\"))
PY
}

Setup="$OfficeDist/effectcraft-Setup-x64.exe"
Iss="$Root/packaging/windows/effectcraft.iss"
Icon="$Root/assets/app-icon/effectcraft.ico"
if Iscc="$(find_iscc)"; then
  echo "==> Inno Setup ($Iscc)"
  rm -f "$Setup"
  if [[ "$Iscc" == *.exe ]] && command -v wine >/dev/null && [[ ! -x /c/Windows/system32/cmd.exe ]]; then
    wine "$Iscc" /Qp "/O$(wine_path "$OfficeDist")" /Feffectcraft-Setup-x64 \
      "/DMyAppVersion=$VERSION" "/DMyVersionInfo=${VERSION%%-*}" "/DBinDir=$(wine_path "$Stage")" "/DIconPath=$(wine_path "$Icon")" \
      "$(wine_path "$Iss")"
  else
    "$Iscc" /Qp /O"$OfficeDist" /Feffectcraft-Setup-x64 \
      /DMyAppVersion="$VERSION" /DMyVersionInfo="${VERSION%%-*}" /DBinDir="$Stage" /DIconPath="$Icon" \
      "$Iss"
  fi
  test -f "$Setup"
  echo "SETUP=$Setup"
else
  echo "warning: Inno Setup (ISCC) not found; portable zip only. Install Inno Setup 6 or set ISCC=." >&2
fi

Sums="$OfficeDist/SHA256SUMS.txt"
{
  if [[ -f "$Setup" ]]; then
    echo "$(sha256 "$Setup")  $(basename "$Setup")"
  fi
  echo "$(sha256 "$Zip")  $(basename "$Zip")"
} >"$Sums"
cat "$Sums"
ls -lh "$Zip"
if [[ -f "$Setup" ]]; then
  ls -lh "$Setup"
fi
