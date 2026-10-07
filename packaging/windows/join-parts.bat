@echo off
REM Join split EffectCraft zip parts produced for private hand-off
REM (each part is under 20 MB). Run this in the folder that contains
REM effectcraft-windows.zip.001, .002, …
setlocal EnableDelayedExpansion
cd /d "%~dp0"

set FILES=
for %%F in (effectcraft-windows.zip.*) do (
  if "!FILES!"=="" (
    set FILES=%%F
  ) else (
    set FILES=!FILES!+%%F
  )
)

if "!FILES!"=="" (
  echo No effectcraft-windows.zip.* parts found.
  exit /b 1
)

copy /b !FILES! effectcraft-0.4.0-windows-x64.zip
if errorlevel 1 (
  echo Join failed.
  exit /b 1
)
echo Wrote effectcraft-0.4.0-windows-x64.zip
endlocal
