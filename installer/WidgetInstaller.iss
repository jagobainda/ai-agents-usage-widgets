#ifndef AppVersion
  #define AppVersion "2.0.0"
#endif

#ifndef RepoRoot
  #define RepoRoot ".."
#endif

#define AppPublisher "Jagoba Inda"
#define AppExe AppName + ".exe"

#ifndef AppSource
  #define AppSource RepoRoot + "\target\release\" + AppExe
#endif

[Setup]
AppId={#StableAppId}
CloseApplications=yes
RestartApplications=no
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
SetupIconFile={#RepoRoot}\apps\{#AppPackage}\assets\{#AppName}.ico
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
Source: "{#AppSource}"; DestDir: "{app}"; DestName: "{#AppExe}"; Flags: ignoreversion

[Icons]
Name: "{userprograms}\{#AppDisplay}"; Filename: "{app}\{#AppExe}"; Tasks: startmenuicon

[Registry]
Root: HKCU; Subkey: "Software\{#AppName}"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "{#AppName}"; ValueData: """{app}\{#AppExe}"""; Tasks: autostart; Flags: uninsdeletevalue

[Run]
Filename: "{app}\{#AppExe}"; Flags: nowait skipifsilent

[UninstallRun]
Filename: "{sys}\taskkill.exe"; Parameters: "/F /IM {#AppExe}"; Flags: runhidden; RunOnceId: "KillRunning"
