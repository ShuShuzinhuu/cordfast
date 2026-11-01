; CordFast's setup program (Inno Setup 6), built by the release workflow:
;   iscc /DAppVersion=<version> scripts\cordfast.iss
; It installs per user, with no administrator prompt, in the folder
; scripts\install.ps1 uses, with the marker fastframe-update reads to
; update this copy through the next release's setup program.

#ifndef AppVersion
  #error Pass /DAppVersion=<version>
#endif

[Setup]
AppId={{6B0F3C2E-8D4A-4C55-9E21-7A3B9C1D4E58}
AppName=CordFast
AppVersion={#AppVersion}
AppVerName=CordFast {#AppVersion}
AppPublisher=CordFast
DefaultDirName={localappdata}\Programs\CordFast
DisableDirPage=yes
DisableProgramGroupPage=yes
DisableReadyPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\dist
OutputBaseFilename=cordfast-v{#AppVersion}-x86_64-pc-windows-msvc-setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
SetupIconFile=..\assets\cordfast.ico
UninstallDisplayIcon={app}\cordfast.exe
UninstallDisplayName=CordFast
CloseApplications=yes
ShowLanguageDialog=auto

[Languages]
; The system's language picks one; the dialog shows only when none matches.
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "brazilianportuguese"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"
Name: "spanish"; MessagesFile: "compiler:Languages\Spanish.isl"
Name: "french"; MessagesFile: "compiler:Languages\French.isl"
Name: "german"; MessagesFile: "compiler:Languages\German.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\target\release\cordfast.exe"; DestDir: "{app}"; Flags: ignoreversion
; fastframe-update's marker: this copy updates through the next setup program.
Source: "cordfast-installer.txt"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{userprograms}\CordFast"; Filename: "{app}\cordfast.exe"
Name: "{userdesktop}\CordFast"; Filename: "{app}\cordfast.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\cordfast.exe"; Description: "{cm:LaunchProgram,CordFast}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; What updates leave beside the program.
Type: filesandordirs; Name: "{app}\.cordfast-update-*"
