; The Windows installer, built with Inno Setup 7 by the release workflow:
;
;   ISCC /DVersion=0.10.0 /DSource=<folder with umlcraft.exe and the licences> umlcraft.iss
;
; It installs for the current user without administrator rights, or for all users after
; asking (PrivilegesRequiredOverridesAllowed). UmlCraft becomes the program for .ucraft files
; and one of the programs offered for .asta and .nsd files, without taking those over.

#ifndef Version
  #error Pass the version: /DVersion=X.Y.Z
#endif
#ifndef Source
  #error Pass the folder with umlcraft.exe: /DSource=...
#endif

[Setup]
; Never change: Windows recognizes the installed UmlCraft by it, for updates and uninstalling.
AppId={{FE50DF86-2B21-4FB0-8EF5-B5C58D1EFD4E}
AppName=UmlCraft
AppVersion={#Version}
AppPublisher=UmlCraft
AppPublisherURL=https://umlcraft.com/
AppSupportURL=https://github.com/udvlp/UmlCraft/issues
AppUpdatesURL=https://github.com/udvlp/UmlCraft/releases
VersionInfoVersion={#Version}
SetupArchitecture=x64
MinVersion=10.0
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
DefaultDirName={autopf}\UmlCraft
DisableProgramGroupPage=yes
ChangesAssociations=yes
WizardStyle=modern
SetupIconFile=umlcraft.ico
UninstallDisplayIcon={app}\umlcraft.exe
OutputDir=output
OutputBaseFilename=UmlCraft-{#Version}-windows-x64-setup
Compression=lzma2/ultra64
SolidCompression=yes
ShowLanguageDialog=auto

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "de"; MessagesFile: "compiler:Languages\German.isl"

[CustomMessages]
en.ProjectType=UmlCraft project
de.ProjectType=UmlCraft-Projekt
en.AstaType=.asta project
de.AstaType=.asta-Projekt
en.NsdType=Structogram
de.NsdType=Struktogramm

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#Source}\umlcraft.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Source}\umlcraft-cli.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Source}\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Source}\LICENSE-MIT"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Source}\LICENSE-APACHE"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Source}\THIRD-PARTY-LICENSES.html"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\UmlCraft"; Filename: "{app}\umlcraft.exe"
Name: "{autodesktop}\UmlCraft"; Filename: "{app}\umlcraft.exe"; Tasks: desktopicon

[Registry]
; HKA is the current user's classes, or the machine's when installing for all users.
Root: HKA; Subkey: "Software\Classes\.ucraft"; ValueType: string; ValueName: ""; ValueData: "UmlCraft.Project"; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.ucraft\OpenWithProgids"; ValueType: string; ValueName: "UmlCraft.Project"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\UmlCraft.Project"; ValueType: string; ValueName: ""; ValueData: "{cm:ProjectType}"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\UmlCraft.Project\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\umlcraft.exe,0"
Root: HKA; Subkey: "Software\Classes\UmlCraft.Project\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\umlcraft.exe"" ""%1"""
; .asta files only get UmlCraft into "Open with"; their default program stays as it is.
Root: HKA; Subkey: "Software\Classes\.asta\OpenWithProgids"; ValueType: string; ValueName: "UmlCraft.Asta"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\UmlCraft.Asta"; ValueType: string; ValueName: ""; ValueData: "{cm:AstaType}"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\UmlCraft.Asta\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\umlcraft.exe,0"
Root: HKA; Subkey: "Software\Classes\UmlCraft.Asta\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\umlcraft.exe"" ""%1"""
; Structorizer's .nsd files likewise.
Root: HKA; Subkey: "Software\Classes\.nsd\OpenWithProgids"; ValueType: string; ValueName: "UmlCraft.Nsd"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\UmlCraft.Nsd"; ValueType: string; ValueName: ""; ValueData: "{cm:NsdType}"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\UmlCraft.Nsd\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\umlcraft.exe,0"
Root: HKA; Subkey: "Software\Classes\UmlCraft.Nsd\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\umlcraft.exe"" ""%1"""
Root: HKA; Subkey: "Software\Classes\Applications\umlcraft.exe\SupportedTypes"; ValueType: string; ValueName: ".ucraft"; ValueData: ""; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Applications\umlcraft.exe\SupportedTypes"; ValueType: string; ValueName: ".asta"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\umlcraft.exe\SupportedTypes"; ValueType: string; ValueName: ".nsd"; ValueData: ""

[Run]
Filename: "{app}\umlcraft.exe"; Description: "{cm:LaunchProgram,UmlCraft}"; Flags: nowait postinstall skipifsilent
; UmlCraft installing its own update runs Setup silently with /RELAUNCH and quits.
Filename: "{app}\umlcraft.exe"; Flags: nowait runasoriginaluser; Check: Relaunch

[Code]
function Relaunch: Boolean;
var
  I: Integer;
begin
  Result := False;
  for I := 1 to ParamCount do
    if CompareText(ParamStr(I), '/RELAUNCH') = 0 then
      Result := True;
end;
