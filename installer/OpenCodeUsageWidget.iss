; Per-user installer for OpenCode Usage Widget. No administrator rights required.

#ifndef AppVersion
  #define AppVersion "1.0.0"
#endif

#ifndef RepoRoot
  #define RepoRoot ".."
#endif

#define AppName       "OpenCodeUsageWidget"
#define AppDisplay    "OpenCode Usage Widget"
#define AppPublisher  "Jagoba Inda"
#define AppExe        "OpenCodeUsageWidget.exe"

[Setup]
; Independent stable GUID so every provider widget can be installed side-by-side.
AppId={{8B0A7ED6-51BE-4D5D-B5EA-7C8356F1672A}
AppName={#AppDisplay}
AppVersion={#AppVersion}
AppVerName={#AppDisplay} {#AppVersion}
AppPublisher={#AppPublisher}
DefaultDirName={localappdata}\{#AppName}
DisableProgramGroupPage=yes
DisableDirPage=no
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppDisplay}
PrivilegesRequired=lowest
OutputDir={#RepoRoot}\releases
OutputBaseFilename={#AppName}-Setup-{#AppVersion}
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
SetupIconFile={#RepoRoot}\build\icons\OpenCodeUsageWidget.ico
VersionInfoVersion={#AppVersion}
VersionInfoCompany={#AppPublisher}
VersionInfoDescription={#AppDisplay} Setup
VersionInfoProductName={#AppDisplay}

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "spanish"; MessagesFile: "compiler:Languages\Spanish.isl"

[Tasks]
Name: "startmenuicon"; Description: "Crear acceso directo en el menu Inicio (recomendado)"; GroupDescription: "Opciones adicionales:"
Name: "autostart"; Description: "Iniciar {#AppDisplay} con Windows"; GroupDescription: "Opciones adicionales:"; Flags: unchecked

[Files]
Source: "{#RepoRoot}\dist\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{userprograms}\{#AppDisplay}"; Filename: "{app}\{#AppExe}"; Tasks: startmenuicon

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "{#AppName}"; ValueData: """{app}\{#AppExe}"""; Tasks: autostart; Flags: uninsdeletevalue

[Run]
Filename: "{app}\{#AppExe}"; Flags: nowait skipifsilent

[UninstallRun]
Filename: "{sys}\taskkill.exe"; Parameters: "/F /IM {#AppExe}"; Flags: runhidden; RunOnceId: "KillRunning"
