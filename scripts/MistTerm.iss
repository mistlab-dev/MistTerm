; Inno Setup script for MistTerm (Windows x64).
; Build via: pwsh scripts/package-windows-installer.ps1

#ifndef AppVersion
  #define AppVersion "1.0.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\target\release"
#endif

#define AppName "MistTerm"
#define AppExe "Mist.exe"
#define AppPublisher "MistLab"
#define AppURL "https://mistlab.dev"
#define AppRepo "https://github.com/mistlab-dev/MistTerm"

[Setup]
AppId={{8F4A2C19-6B3D-4E71-9A02-1C5D8E7F4B26}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#AppPublisher}
AppPublisherURL={#AppURL}
AppSupportURL={#AppRepo}/issues
AppUpdatesURL={#AppRepo}/releases
DefaultDirName={autopf}\MistTerm
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
OutputDir=..\dist
OutputBaseFilename=MistTerm-{#AppVersion}-windows-x86_64-setup
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayIcon={app}\{#AppExe}
CloseApplications=force

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon,{#AppName}}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceDir}\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "..\docs\en\INSTALL.md"; DestDir: "{app}"; DestName: "INSTALL.md"; Flags: ignoreversion skipifsourcedoesntexist
Source: "..\docs\zh\INSTALL.md"; DestDir: "{app}"; DestName: "INSTALL.zh.md"; Flags: ignoreversion skipifsourcedoesntexist

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExe}"; Comment: "MistTerm SSH Terminal"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExe}"; Description: "Launch {#AppName}"; Flags: nowait postinstall skipifsilent

[Code]
function InitializeSetup(): Boolean;
begin
  Result := True;
end;
