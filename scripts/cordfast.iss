; CordFast's setup program (Inno Setup 6), built by the release workflow:
;   iscc /DAppVersion=<version> scripts\cordfast.iss
; It installs per user, with no administrator prompt, in the folder
; scripts\install.ps1 uses, with the marker fastframe-update reads to
; update this copy through the next release's setup program.
;
; Like DiscordSetup.exe (Squirrel), opening it is the whole installation: no
; pages, no questions. It starts itself again silently, installs, makes the
; Start menu and desktop shortcuts and opens CordFast with --installed, whose
; first start turns "open at login" on, as Discord's installer does. The
; updater runs it with /VERYSILENT itself, without /LAUNCH, and starts the app
; on its own.

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
CloseApplications=force
RestartApplications=no
ShowLanguageDialog=no
DisableWelcomePage=yes
DisableFinishedPage=yes

[Languages]
; The system's language picks one; the dialog shows only when none matches.
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "brazilianportuguese"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"
Name: "spanish"; MessagesFile: "compiler:Languages\Spanish.isl"
Name: "french"; MessagesFile: "compiler:Languages\French.isl"
Name: "german"; MessagesFile: "compiler:Languages\German.isl"

[Files]
Source: "..\target\release\cordfast.exe"; DestDir: "{app}"; Flags: ignoreversion
; fastframe-update's marker: this copy updates through the next setup program.
Source: "cordfast-installer.txt"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
; Both, as Discord makes them. An update rewrites them with the same target.
Name: "{userprograms}\CordFast"; Filename: "{app}\cordfast.exe"
Name: "{userdesktop}\CordFast"; Filename: "{app}\cordfast.exe"

[Run]
; Only the installation someone opened starts the app; an update's helper
; starts it itself.
Filename: "{app}\cordfast.exe"; Parameters: "--installed"; Flags: nowait; Check: Launching

[UninstallDelete]
; What updates leave beside the program.
Type: filesandordirs; Name: "{app}\.cordfast-update-*"

[Code]
function Launching: Boolean;
begin
  Result := ExpandConstant('{param:LAUNCH|0}') = '1';
end;

// Opened by hand: start again silently and let that copy do the work, so
// nothing is asked and only CordFast's own window shows, as with Discord.
function InitializeSetup: Boolean;
var
  Code: Integer;
begin
  Result := True;
  if not WizardSilent then
  begin
    Exec(ExpandConstant('{srcexe}'), '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /SP- /LAUNCH=1', '', SW_SHOW, ewNoWait, Code);
    Result := False;
  end;
end;

// "Open at login" goes with the program (fastframe-autostart's Run value).
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    RegDeleteValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'CordFast');
end;
