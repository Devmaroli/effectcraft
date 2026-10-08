; EffectCraft per-user Windows installer (Inno Setup 6).
;
; Office-friendly: no administrator rights. Installs to
; %LOCALAPPDATA%\Programs\Craft\effectcraft, Start Menu + desktop shortcuts,
; the pink unicorn icon, an uninstaller, and an optional user PATH entry for
; effectcraft-cli.exe.
;
; Built by packaging/windows/package-office.ps1 (and the v* tag workflow):
;
;   iscc /O<out> /Feffectcraft-Setup-x64 ^
;        /DMyAppVersion=0.6.0-beta /DBinDir=<stage> /DIconPath=<ico> ^
;        packaging\windows\effectcraft.iss
;
; Do not ship portable.txt in this installer: the installed copy uses AppData.

#ifndef MyAppVersion
  #define MyAppVersion "0.6.0-beta"
#endif
#ifndef MyVersionInfo
  #define MyVersionInfo "0.6.0"
#endif
#ifndef BinDir
  #define BinDir "..\..\target\x86_64-pc-windows-msvc\release"
#endif
#ifndef IconPath
  #define IconPath "..\..\assets\app-icon\effectcraft.ico"
#endif

#define MyAppName "EffectCraft"
#define MyAppPublisher "Learning Machines LLC"
#define MyAppURL "https://getartcraft.com/apps/effectcraft"
#define MyAppExeName "effectcraft.exe"
#define MyAppCliName "effectcraft-cli.exe"

[Setup]
AppId={{7B3E1C5A-9D24-4F8A-B6E1-2C4A8F91D0E3}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL=https://discord.gg/artcraft
AppUpdatesURL={#MyAppURL}
DefaultDirName={localappdata}\Programs\Craft\effectcraft
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
; Per-user: never prompt for elevation.
PrivilegesRequired=lowest
OutputBaseFilename=effectcraft-Setup-x64
SetupIconFile={#IconPath}
UninstallDisplayIcon={app}\{#MyAppExeName}
UninstallDisplayName={#MyAppName}
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
UsePreviousAppDir=yes
CloseApplications=yes
RestartApplications=no
ChangesEnvironment=yes
; Same-version reinstalls replace files (pre-releases of one X.Y.Z).
AllowNoIcons=yes
VersionInfoVersion={#MyVersionInfo}
VersionInfoCompany={#MyAppPublisher}
VersionInfoDescription=EffectCraft motion graphics and visual effects
VersionInfoProductName={#MyAppName}
LicenseFile=..\..\LICENSE-MIT

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: checkedonce
Name: "addtopath"; Description: "Add &effectcraft-cli to PATH (this user only)"; GroupDescription: "Command line:"; Flags: unchecked

[Files]
Source: "{#BinDir}\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#BinDir}\{#MyAppCliName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#IconPath}"; DestDir: "{app}"; DestName: "effectcraft.ico"; Flags: ignoreversion
Source: "..\..\LICENSE-MIT"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "..\..\LICENSE-APACHE"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "README-Windows.txt"; DestDir: "{app}"; DestName: "README.txt"; Flags: ignoreversion skipifsourcedoesntexist

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"; IconFilename: "{app}\effectcraft.ico"; Comment: "EffectCraft motion graphics and visual effects"
Name: "{group}\Uninstall {#MyAppName}"; Filename: "{uninstallexe}"; IconFilename: "{app}\effectcraft.ico"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; WorkingDir: "{app}"; IconFilename: "{app}\effectcraft.ico"; Tasks: desktopicon; Comment: "EffectCraft motion graphics and visual effects"

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "Launch {#MyAppName}"; Flags: nowait postinstall skipifsilent

[Code]
const
  EnvironmentKey = 'Environment';

function GetUserPath: String;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, EnvironmentKey, 'Path', Result) then
    Result := '';
end;

function PathListContains(const PathList, Dir: String): Boolean;
begin
  Result := Pos(';' + AnsiLowercase(Dir) + ';', ';' + AnsiLowercase(PathList) + ';') > 0;
end;

procedure AddToUserPath(const Dir: String);
var
  Path: String;
begin
  if Dir = '' then
    Exit;
  Path := GetUserPath();
  if PathListContains(Path, Dir) then
    Exit;
  if Path = '' then
    Path := Dir
  else if Path[Length(Path)] = ';' then
    Path := Path + Dir
  else
    Path := Path + ';' + Dir;
  if not RegWriteExpandStringValue(HKEY_CURRENT_USER, EnvironmentKey, 'Path', Path) then
    Log('Could not write HKCU Path');
end;

procedure RemoveFromUserPath(const Dir: String);
var
  Path: String;
begin
  if Dir = '' then
    Exit;
  Path := ';' + GetUserPath() + ';';
  StringChangeEx(Path, ';' + Dir + ';', ';', True);
  if Length(Path) >= 2 then
    Path := Copy(Path, 2, Length(Path) - 2)
  else
    Path := '';
  while (Length(Path) > 0) and (Path[1] = ';') do
    Delete(Path, 1, 1);
  while (Length(Path) > 0) and (Path[Length(Path)] = ';') do
    Delete(Path, Length(Path), 1);
  RegWriteExpandStringValue(HKEY_CURRENT_USER, EnvironmentKey, 'Path', Path);
end;

function CmdLineHasSilentS: Boolean;
var
  I: Integer;
begin
  Result := False;
  for I := 1 to ParamCount do
    if CompareText(ParamStr(I), '/S') = 0 then
    begin
      Result := True;
      Exit;
    end;
end;

function RelaunchSilentParams: String;
var
  I: Integer;
begin
  Result := '/VERYSILENT /NORESTART /SUPPRESSMSGBOXES';
  for I := 1 to ParamCount do
    if CompareText(ParamStr(I), '/S') <> 0 then
      Result := Result + ' ' + AddQuotes(ParamStr(I));
end;

procedure RemoveLegacyNsisInstall;
var
  Uninst: String;
  ResultCode: Integer;
begin
  { Previous office builds used NSIS (HKCU Uninstall\EffectCraft). Inno Setup writes a
    different key, so an upgrade left two Add/Remove Programs rows until this cleanup. }
  if RegQueryStringValue(HKEY_CURRENT_USER, 'Software\Microsoft\Windows\CurrentVersion\Uninstall\EffectCraft', 'UninstallString', Uninst) then
  begin
    Uninst := RemoveQuotes(Uninst);
    if (Uninst <> '') and FileExists(Uninst) then
      Exec(Uninst, '/S', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
    RegDeleteKeyIncludingSubkeys(HKEY_CURRENT_USER, 'Software\Microsoft\Windows\CurrentVersion\Uninstall\EffectCraft');
    RegDeleteKeyIncludingSubkeys(HKEY_CURRENT_USER, 'Software\EffectCraft');
  end;
end;

function InitializeSetup: Boolean;
var
  ResultCode: Integer;
begin
  Result := True;
  if CmdLineHasSilentS and (not WizardSilent) then
  begin
    Exec(ExpandConstant('{srcexe}'), RelaunchSilentParams(), '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
    Result := False;
    Exit;
  end;
  RemoveLegacyNsisInstall;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if (CurStep = ssPostInstall) and WizardIsTaskSelected('addtopath') then
    AddToUserPath(ExpandConstant('{app}'));
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    RemoveFromUserPath(ExpandConstant('{app}'));
end;
