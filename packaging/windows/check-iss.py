#!/usr/bin/env python3
"""Static checks for the per-user Inno Setup script (used by packaging-lint)."""
from pathlib import Path

root = Path(__file__).resolve().parents[2]
iss = (root / "packaging/windows/effectcraft.iss").read_text()
need = [
    "PrivilegesRequired=lowest",
    r"{localappdata}\Programs\Craft\effectcraft",
    "effectcraft-Setup-x64",
    "desktopicon",
    "addtopath",
    "UninstallDisplayIcon",
    "effectcraft.ico",
    "effectcraft-cli.exe",
]
missing = [s for s in need if s not in iss]
if missing:
    raise SystemExit(f"effectcraft.iss missing {missing}")
if "PrivilegesRequiredOverridesAllowed" in iss:
    raise SystemExit("effectcraft.iss must not offer an elevation dialog")
marker = root / "packaging/windows/portable.txt"
if not marker.is_file():
    raise SystemExit("missing packaging/windows/portable.txt")
nsi = (root / "packaging/windows/effectcraft.nsi").read_text()
nsi_need = [
    "RequestExecutionLevel user",
    r"$LOCALAPPDATA\Programs\Craft\effectcraft",
    "effectcraft-Setup-x64.exe",
    "effectcraft-cli",
    "Uninstall.exe",
    "effectcraft.ico",
]
nsi_missing = [s for s in nsi_need if s not in nsi]
if nsi_missing:
    raise SystemExit(f"effectcraft.nsi missing {nsi_missing}")
print("ok effectcraft.iss + effectcraft.nsi + portable.txt")
