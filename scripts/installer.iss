; Script generated for Inno Setup 6
; Open Antigravity Windows Installer

#ifndef MyAppVersion
#define MyAppVersion "1.3.1"
#endif

#define MyAppName "Open Antigravity"
#define MyAppPublisher "keelbismark"
#define MyAppURL "https://github.com/keelbismark/OpenAntigravity"
#define MyAppExeName "OpenAntigravity.exe"

[Setup]
AppId={{9F82A4D1-5C6E-4E7B-A3B0-4A63D78E2C91}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} v{#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL=https://t.me/bismark_dev
AppUpdatesURL=https://keelbismark.github.io/OpenAntigravity/

; Per-user install: %LOCALAPPDATA%\Programs\OpenAntigravity (no UAC admin rights needed to install)
DefaultDirName={localappdata}\Programs\OpenAntigravity
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog

OutputDir=..\release
OutputBaseFilename=OpenAntigravity_windows_v{#MyAppVersion}_setup
SetupIconFile=..\icon.ico
UninstallDisplayIcon={app}\{#MyAppExeName}

Compression=lzma2/ultra64
SolidCompression=yes

CloseApplications=yes
CloseApplicationsFilter={#MyAppExeName},OpenAntigravityRelay.exe
WizardStyle=modern
WizardImageFile=..\assets\installer_banner.bmp
WizardSmallImageFile=..\assets\installer_small.bmp
ArchitecturesInstallIn64BitMode=x64compatible

[Languages]
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"
Name: "autostart"; Description: "Запускать Open Antigravity при входе в Windows"; GroupDescription: "Параметры запуска:"; Flags: unchecked

[Files]
Source: "..\target\release\open_antigravity.exe"; DestDir: "{app}"; DestName: "{#MyAppExeName}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; DestName: "README.txt"; Flags: ignoreversion isreadme

[Icons]
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "OpenAntigravity"; ValueData: """{app}\{#MyAppExeName}"" --minimized"; Flags: uninsdeletevalue; Tasks: autostart

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "taskkill.exe"; Parameters: "/F /IM {#MyAppExeName} /IM OpenAntigravityRelay.exe"; Flags: runhidden; RunOnceId: "KillProcesses"
