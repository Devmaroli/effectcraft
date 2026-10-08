EffectCraft for Windows (64-bit)
================================

This folder is a ready-to-run copy of EffectCraft: the main program and the
command-line helper that EncodeCraft uses to render compositions.

What is in this folder
----------------------
- effectcraft.exe       The EffectCraft window (double-click this)
- effectcraft-cli.exe   The renderer EncodeCraft calls in the background
- README-Windows.txt    This file

No installer is needed. You can put this whole folder anywhere you like,
for example:

  C:\Users\YourName\Apps\EffectCraft\

This copy is portable: portable.txt next to effectcraft.exe tells EffectCraft
to keep settings, shortcut presets and downloaded models in this folder
instead of %APPDATA%\EffectCraft. Leave that file here. If you delete it,
the next launch uses the usual per-user settings folder.

A per-user installer (effectcraft-Setup-x64.exe) is also available. It
installs to %LOCALAPPDATA%\Programs\Craft\effectcraft, does not need
administrator rights, and adds Start menu and desktop shortcuts. That
installed copy does not include portable.txt, so its settings stay in
AppData. You can optionally put effectcraft-cli on your user PATH.

Silent install (same switch as the older NSIS setup):

  effectcraft-Setup-x64.exe /S

/SILENT and /VERYSILENT also work. An older NSIS uninstall entry is
removed so Windows shows one EffectCraft in Apps & features.

How to open EffectCraft
-----------------------
1. Open the folder in File Explorer.
2. Double-click effectcraft.exe.
3. Windows may show a SmartScreen warning because this copy is not digitally
   signed (it was built for you, not by Microsoft). That is expected.

   If you see "Windows protected your PC":
     - Click "More info"
     - Click "Run anyway"

   If Windows Defender or another antivirus quarantines the file, restore it
   from that app's history. It is unsigned open-source software, not a virus,
   but you should only run a zip you got from a person you trust.

4. The first start can take a little while. There is no extra console (black)
   window; that is on purpose.

How EncodeCraft finds effectcraft-cli
-------------------------------------
EncodeCraft does not magically know where EffectCraft lives. Put both apps
in the same folder, which is the setup this build is meant for:

  C:\...\YourFolder\
    effectcraft.exe
    effectcraft-cli.exe
    EncodeCraft.exe          (from your EncodeCraft zip)

Then:

- In EffectCraft: save your project (File > Save), open a composition, and
  choose Composition > Add to EncodeCraft Queue.
- EncodeCraft looks next to itself for effectcraft-cli.exe, so keeping them
  together is enough.

If you prefer them in different folders, set a user environment variable
named EFFECTCRAFT_CLI to the full path of effectcraft-cli.exe (for example
C:\Users\YourName\Apps\EffectCraft\effectcraft-cli.exe), or put that folder
on your PATH. EncodeCraft also honours EFFECTCRAFT_CLI if that is how your
copy is written.

A few things that help
----------------------
- Always save the project before Add to EncodeCraft Queue. EncodeCraft
  renders the file on disk, not unsaved work in the window.
- EncodeCraft should be installed and, the first time, started once so it
  can create its inbox folder and an ipc-token file. EffectCraft sends
  that token on every queue request (or ENCODECRAFT_TOKEN). If you see
  "Open EncodeCraft once so it can set up the connection", start EncodeCraft
  and try again. After that, EffectCraft will start it if it is not already
  running.
- This zip is 64-bit Windows 10 or 11. It will not run on 32-bit Windows.

If a .dll is missing
--------------------
This build tries to include any extra files it needs. If Windows says a
DLL is missing, copy that file from a friend who has a working copy, or
ask whoever built this zip to rebuild it with the DLL included. Do not
download random DLLs from the web.
