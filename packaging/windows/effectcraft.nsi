; EffectCraft per-user Windows installer (NSIS 3).
;
; Same layout as effectcraft.iss (Inno): no administrator rights, install to
; %LOCALAPPDATA%\Programs\Craft\effectcraft, Start menu + desktop shortcuts,
; the pink unicorn icon, an uninstaller, optional user PATH for effectcraft-cli.
;
; Used when Inno Setup is not available (Linux makensis). The GitHub tag
; workflow compiles the Inno script on windows-latest.
;
;   makensis -DVERSION=0.5.0 -DBINDIR=<stage> -DICONPATH=<ico> -DOUTFILE=<exe> \
;            packaging/windows/effectcraft.nsi

!ifndef VERSION
  !define VERSION "0.5.0"
!endif
!ifndef BINDIR
  !define BINDIR "..\..\target\x86_64-pc-windows-gnu\release"
!endif
!ifndef ICONPATH
  !define ICONPATH "..\..\assets\app-icon\effectcraft.ico"
!endif
!ifndef OUTFILE
  !define OUTFILE "effectcraft-Setup-x64.exe"
!endif

Unicode true
SetCompressor /SOLID lzma
RequestExecutionLevel user
ManifestDPIAware true

Name "EffectCraft"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\Craft\effectcraft"
InstallDirRegKey HKCU "Software\EffectCraft" "InstallDir"
ShowInstDetails show
ShowUninstDetails show

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "EffectCraft"
VIAddVersionKey "FileDescription" "EffectCraft motion graphics and visual effects"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "CompanyName" "Learning Machines LLC"
VIAddVersionKey "LegalCopyright" "Copyright (c) Learning Machines LLC"

!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "WinMessages.nsh"
!include "FileFunc.nsh"

!define MUI_ICON "${ICONPATH}"
!define MUI_UNICON "${ICONPATH}"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\effectcraft.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Launch EffectCraft"

!insertmacro MUI_PAGE_LICENSE "../../LICENSE-MIT"
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

; ---- PATH helpers (HKCU, this user only) ------------------------------------
Function StrStr
  Exch $R1
  Exch
  Exch $R2
  Push $R3
  Push $R4
  Push $R5
  StrLen $R3 $R1
  StrCpy $R4 0
  loop:
    StrCpy $R5 $R2 $R3 $R4
    StrCmp $R5 $R1 found
    StrCmp $R5 "" done
    IntOp $R4 $R4 + 1
    Goto loop
  found:
    StrCpy $R1 $R2 "" $R4
    Goto done2
  done:
    StrCpy $R1 ""
  done2:
    Pop $R5
    Pop $R4
    Pop $R3
    Pop $R2
    Exch $R1
FunctionEnd

Function AddToUserPath
  Push $0
  Push $1
  ReadRegStr $0 HKCU "Environment" "Path"
  Push ";$0;"
  Push ";$INSTDIR;"
  Call StrStr
  Pop $1
  ${If} $1 == ""
    ${If} $0 == ""
      StrCpy $0 "$INSTDIR"
    ${Else}
      StrCpy $0 "$0;$INSTDIR"
    ${EndIf}
    WriteRegExpandStr HKCU "Environment" "Path" $0
    SendMessage ${HWND_BROADCAST} ${WM_WININICHANGE} 0 "STR:Environment" /TIMEOUT=5000
  ${EndIf}
  Pop $1
  Pop $0
FunctionEnd

Function un.RemoveFromUserPath
  Push $0
  Push $1
  Push $2
  ReadRegStr $0 HKCU "Environment" "Path"
  ; Remove ";dir" or "dir;" or exact "dir"
  Push "$0"
  Push ";$INSTDIR"
  Call un.StrReplace
  Pop $0
  Push "$0"
  Push "$INSTDIR;"
  Call un.StrReplace
  Pop $0
  ${If} $0 == "$INSTDIR"
    StrCpy $0 ""
  ${EndIf}
  WriteRegExpandStr HKCU "Environment" "Path" $0
  SendMessage ${HWND_BROADCAST} ${WM_WININICHANGE} 0 "STR:Environment" /TIMEOUT=5000
  Pop $2
  Pop $1
  Pop $0
FunctionEnd

Function un.StrReplace
  ; stack: string, old  -> new string (old replaced with empty)
  Exch $R0 ; old
  Exch
  Exch $R1 ; string
  Push $R2
  Push $R3
  Push $R4
  Push $R5
  StrLen $R2 $R0
  StrCpy $R3 ""
  loop:
    Push $R1
    Push $R0
    Call un.StrStr
    Pop $R4
    ${If} $R4 == ""
      StrCpy $R3 "$R3$R1"
      Goto done
    ${EndIf}
    StrLen $R5 $R4
    IntOp $R5 $R5 - $R2
    StrCpy $R5 $R1 $R5
    StrCpy $R3 "$R3$R5"
    StrCpy $R1 $R4 "" $R2
    Goto loop
  done:
    StrCpy $R1 $R3
    Pop $R5
    Pop $R4
    Pop $R3
    Pop $R2
    Pop $R0
    Exch $R1
FunctionEnd

Function un.StrStr
  Exch $R1
  Exch
  Exch $R2
  Push $R3
  Push $R4
  Push $R5
  StrLen $R3 $R1
  StrCpy $R4 0
  uloop:
    StrCpy $R5 $R2 $R3 $R4
    StrCmp $R5 $R1 ufound
    StrCmp $R5 "" udone
    IntOp $R4 $R4 + 1
    Goto uloop
  ufound:
    StrCpy $R1 $R2 "" $R4
    Goto udone2
  udone:
    StrCpy $R1 ""
  udone2:
    Pop $R5
    Pop $R4
    Pop $R3
    Pop $R2
    Exch $R1
FunctionEnd

Section "EffectCraft" SecApp
  SectionIn RO
  SetOutPath "$INSTDIR"
  File "${BINDIR}/effectcraft.exe"
  File "${BINDIR}/effectcraft-cli.exe"
  File "/oname=effectcraft.ico" "${ICONPATH}"
  File /nonfatal "../../LICENSE-MIT"
  File /nonfatal "../../LICENSE-APACHE"
  File /nonfatal "/oname=README.txt" "README-Windows.txt"

  WriteUninstaller "$INSTDIR\Uninstall.exe"
  WriteRegStr HKCU "Software\EffectCraft" "InstallDir" "$INSTDIR"

  CreateDirectory "$SMPROGRAMS\EffectCraft"
  CreateShortCut "$SMPROGRAMS\EffectCraft\EffectCraft.lnk" "$INSTDIR\effectcraft.exe" "" "$INSTDIR\effectcraft.ico" 0
  CreateShortCut "$SMPROGRAMS\EffectCraft\Uninstall EffectCraft.lnk" "$INSTDIR\Uninstall.exe" "" "$INSTDIR\effectcraft.ico" 0
  CreateShortCut "$DESKTOP\EffectCraft.lnk" "$INSTDIR\effectcraft.exe" "" "$INSTDIR\effectcraft.ico" 0

  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\EffectCraft" "DisplayName" "EffectCraft ${VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\EffectCraft" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\EffectCraft" "Publisher" "Learning Machines LLC"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\EffectCraft" "DisplayIcon" "$INSTDIR\effectcraft.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\EffectCraft" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\EffectCraft" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\EffectCraft" "URLInfoAbout" "https://getartcraft.com/apps/effectcraft"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\EffectCraft" "HelpLink" "https://discord.gg/artcraft"
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\EffectCraft" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\EffectCraft" "NoRepair" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\EffectCraft" "EstimatedSize" 65536
SectionEnd

Section /o "Add effectcraft-cli to PATH" SecPath
  Call AddToUserPath
SectionEnd

LangString DESC_SecApp ${LANG_ENGLISH} "EffectCraft and effectcraft-cli. Installs for this user only; no administrator rights."
LangString DESC_SecPath ${LANG_ENGLISH} "Append the install folder to this user's PATH so EncodeCraft and a terminal can run effectcraft-cli."
!insertmacro MUI_FUNCTION_DESCRIPTION_BEGIN
  !insertmacro MUI_DESCRIPTION_TEXT ${SecApp} $(DESC_SecApp)
  !insertmacro MUI_DESCRIPTION_TEXT ${SecPath} $(DESC_SecPath)
!insertmacro MUI_FUNCTION_DESCRIPTION_END

Section "Uninstall"
  Call un.RemoveFromUserPath
  Delete "$INSTDIR\effectcraft.exe"
  Delete "$INSTDIR\effectcraft-cli.exe"
  Delete "$INSTDIR\effectcraft.ico"
  Delete "$INSTDIR\LICENSE-MIT"
  Delete "$INSTDIR\LICENSE-APACHE"
  Delete "$INSTDIR\README.txt"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir "$INSTDIR"
  RMDir "$LOCALAPPDATA\Programs\Craft"
  Delete "$DESKTOP\EffectCraft.lnk"
  Delete "$SMPROGRAMS\EffectCraft\EffectCraft.lnk"
  Delete "$SMPROGRAMS\EffectCraft\Uninstall EffectCraft.lnk"
  RMDir "$SMPROGRAMS\EffectCraft"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\EffectCraft"
  DeleteRegKey HKCU "Software\EffectCraft"
SectionEnd
