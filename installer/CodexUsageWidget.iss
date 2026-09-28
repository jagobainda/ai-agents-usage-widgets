; Per-user installer for Codex Usage Widget. No administrator rights required.

#ifndef AppVersion
  #define AppVersion "1.0.0"
#endif

#ifndef RepoRoot
  #define RepoRoot ".."
#endif

#define AppName       "CodexUsageWidget"
#define AppDisplay    "Codex Usage Widget"
#define AppPublisher  "Jagoba Inda"
#define AppExe        "CodexUsageWidget.exe"

[Setup]
; Independent stable GUID so Claude and Codex can be installed side-by-side.
AppId={{27F41A09-AB62-47A5-927D-8F22F2C5B0E4}
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
SetupIconFile={#RepoRoot}\build\icons\CodexUsageWidget.ico
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
