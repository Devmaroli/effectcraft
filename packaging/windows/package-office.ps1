<#
.SYNOPSIS
  Build the office-friendly Windows x64 installer and portable zip.

.DESCRIPTION
  Produces, in $env:DIST (default: dist/office):
    effectcraft-Setup-x64.exe       per-user Inno Setup installer (no admin)
    effectcraft-Portable-x64.zip    zip that runs from any folder; portable.txt
                                    next to the exe keeps settings in that folder
    SHA256SUMS.txt

  Needs: Rust (MSVC x64), Inno Setup 6 (`choco install innosetup`), Windows SDK
  only if you also sign via sign.ps1.

.EXAMPLE
  pwsh packaging/windows/package-office.ps1
  pwsh packaging/windows/package-office.ps1 -SkipBuild
#>
param(
  [switch] $SkipBuild
)
$ErrorActionPreference = 'Stop'
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

function Invoke-Native([string] $What, [scriptblock] $Block) {
  Write-Output "==> $What"
  & $Block
  if ($LASTEXITCODE -ne 0) { throw "$What failed with exit code $LASTEXITCODE" }
}

function Find-Iscc {
  $candidates = @(
    $env:ISCC,
    (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe'),
    (Join-Path $env:ProgramFiles 'Inno Setup 6\ISCC.exe')
  )
  $cmd = Get-Command iscc -ErrorAction SilentlyContinue
  if ($cmd) { $candidates += $cmd.Source }
  foreach ($c in $candidates) {
    if ($c -and (Test-Path -LiteralPath $c)) { return $c }
  }
  return $null
}

$Version = $env:EFFECTCRAFT_VERSION
if (-not $Version) {
  $inPkg = $false
  foreach ($line in Get-Content (Join-Path $Root 'Cargo.toml')) {
    if ($line -match '^\s*\[') { $inPkg = ($line.Trim() -eq '[workspace.package]'); continue }
    if ($inPkg -and $line -match '^\s*version\s*=\s*"([^"]+)"') { $Version = $Matches[1]; break }
  }
}
if (-not $Version) { throw 'could not read [workspace.package] version from Cargo.toml' }

$Target = 'x86_64-pc-windows-msvc'
$Dist = if ($env:DIST) { $env:DIST } else { Join-Path $Root 'dist\office' }
$TargetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Root 'target' }
New-Item -ItemType Directory -Force -Path $Dist | Out-Null

if (-not $env:EFFECTCRAFT_BUILD_SHA) { $env:EFFECTCRAFT_BUILD_SHA = (git -C $Root rev-parse HEAD 2>$null) }
if (-not $env:EFFECTCRAFT_BUILD_DATE) { $env:EFFECTCRAFT_BUILD_DATE = (Get-Date).ToUniversalTime().ToString('yyyy-MM-dd') }

Write-Output "EffectCraft $Version office x64 ($Target)"

if (-not $SkipBuild) {
  $env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = '-C target-feature=+crt-static'
  $env:EFFECTCRAFT_REQUIRE_WINRES = '1'
  Invoke-Native "cargo build ($Target)" { cargo build --release --locked -p effectcraft -p effectcraft-cli --target $Target }
}

$Bin = Join-Path $TargetDir "$Target\release"
function Get-PeHeader([string] $Path) {
  $bytes = [System.IO.File]::ReadAllBytes($Path)
  $pe = [BitConverter]::ToInt32($bytes, 0x3C)
  return @{ Machine = [BitConverter]::ToUInt16($bytes, $pe + 4); Subsystem = [BitConverter]::ToUInt16($bytes, $pe + 0x5C) }
}
foreach ($check in @(@('effectcraft.exe', 2), @('effectcraft-cli.exe', 3))) {
  $h = Get-PeHeader (Join-Path $Bin $check[0])
  if ($h.Machine -ne 0x8664) { throw "$($check[0]) is not x64 (machine 0x$('{0:X}' -f $h.Machine))" }
  if ($h.Subsystem -ne $check[1]) { throw "$($check[0]) has PE subsystem $($h.Subsystem), expected $($check[1])" }
  Write-Output "ok $($check[0]): x64, PE subsystem $($h.Subsystem)"
}

$Stage = Join-Path $TargetDir 'windows-package\office-x64'
Remove-Item -Recurse -Force $Stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Stage | Out-Null
Copy-Item (Join-Path $Bin 'effectcraft.exe'), (Join-Path $Bin 'effectcraft-cli.exe') $Stage

$Sign = Join-Path $PSScriptRoot 'sign.ps1'
if (Test-Path $Sign) {
  & $Sign (Join-Path $Stage 'effectcraft.exe') (Join-Path $Stage 'effectcraft-cli.exe')
}

# ---- portable zip (settings next to the exe via portable.txt) ----------------
$PortableName = 'effectcraft'
$Portable = Join-Path $TargetDir "windows-package\$PortableName"
Remove-Item -Recurse -Force $Portable -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Portable | Out-Null
Copy-Item (Join-Path $Stage '*.exe') $Portable
Copy-Item (Join-Path $PSScriptRoot 'portable.txt') $Portable
Copy-Item (Join-Path $PSScriptRoot 'README-Windows.txt') $Portable
foreach ($f in 'LICENSE-MIT', 'LICENSE-APACHE', 'NOTICE') {
  $p = Join-Path $Root $f
  if (Test-Path $p) { Copy-Item $p $Portable }
}
$Zip = Join-Path $Dist 'effectcraft-Portable-x64.zip'
Remove-Item -Force $Zip -ErrorAction SilentlyContinue
Compress-Archive -Path $Portable -DestinationPath $Zip

# ---- Inno Setup per-user installer ------------------------------------------
$Iscc = Find-Iscc
if (-not $Iscc) {
  throw 'Inno Setup 6 (ISCC.exe) not found. Install with: choco install innosetup'
}
$Icon = Join-Path $Root 'assets\app-icon\effectcraft.ico'
$Iss = Join-Path $PSScriptRoot 'effectcraft.iss'
$Setup = Join-Path $Dist 'effectcraft-Setup-x64.exe'
Remove-Item -Force $Setup -ErrorAction SilentlyContinue
$InfoVersion = ($Version -split '-')[0]
Invoke-Native 'Inno Setup (ISCC)' {
  & $Iscc /Qp /O"$Dist" /F"effectcraft-Setup-x64" `
    /DMyAppVersion="$Version" /DMyVersionInfo="$InfoVersion" /DBinDir="$Stage" /DIconPath="$Icon" `
    $Iss
}
if (-not (Test-Path $Setup)) { throw "ISCC did not write $Setup" }
if (Test-Path $Sign) { & $Sign $Setup }

Invoke-Native 'effectcraft-cli --version' { & (Join-Path $Stage 'effectcraft-cli.exe') --version }

$Sums = Join-Path $Dist 'SHA256SUMS.txt'
$lines = @()
foreach ($p in @($Setup, $Zip)) {
  $hash = (Get-FileHash -Algorithm SHA256 $p).Hash.ToLower()
  $lines += "$hash  $(Split-Path $p -Leaf)"
}
Set-Content -Path $Sums -Value ($lines -join "`n") -Encoding ascii
Get-Content $Sums
Get-Item $Setup, $Zip | Format-Table Name, Length
