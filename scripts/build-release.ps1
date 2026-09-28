<#
.SYNOPSIS
    Builds one or all Windows usage widgets and optionally their installers.

.PARAMETER App
    claude (default), codex, opencode, or all. The default preserves the original
    repository command behaviour.

.PARAMETER Sign
    Sign each resulting executable with a Certum smart-card certificate.

.PARAMETER Installer
    Also create the corresponding per-user Inno Setup installer(s).

.EXAMPLE
    .\scripts\build-release.ps1 -App claude -Version "1.1.0"
    .\scripts\build-release.ps1 -App codex -Version "1.1.0" -Installer
    .\scripts\build-release.ps1 -App opencode -Version "1.1.0" -Installer
    .\scripts\build-release.ps1 -App all -Version "1.1.0"
#>

param(
    [ValidateSet("claude", "codex", "opencode", "all")]
    [string]$App = "claude",
    [switch]$Sign,
    [string]$CertThumbprint = "",
    [string]$KeyInfoFile = "C:\Users\Jagoba\keyinfo.inf",
    [string]$Version = "1.0.0",
    [switch]$Installer
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$TimestampUrl = "http://time.certum.pl"
$SigntoolExe = "C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\signtool.exe"
$RepoRoot = Split-Path $PSScriptRoot -Parent
$Requirements = Join-Path $RepoRoot "requirements.txt"
$CommonPath = Join-Path $RepoRoot "packages\widget-common"
$BuildDir = Join-Path $RepoRoot "build"
$DistDir = Join-Path $RepoRoot "dist"
$ReleaseDir = Join-Path $RepoRoot "releases"
$VenvDir = Join-Path $RepoRoot ".venv"
$Python = Join-Path $VenvDir "Scripts\python.exe"
$MakeIcon = Join-Path $PSScriptRoot "_make_icon.py"

$Applications = @{
    claude = @{
        Name = "ClaudeUsageWidget"
        Display = "Claude Usage Widget"
        Main = Join-Path $RepoRoot "apps\claude-usage-widget\main.py"
        AppPath = Join-Path $RepoRoot "apps\claude-usage-widget"
        Installer = Join-Path $RepoRoot "installer\ClaudeUsageWidget.iss"
    }
    codex = @{
        Name = "CodexUsageWidget"
        Display = "Codex Usage Widget"
        Main = Join-Path $RepoRoot "apps\codex-usage-widget\main.py"
        AppPath = Join-Path $RepoRoot "apps\codex-usage-widget"
        Installer = Join-Path $RepoRoot "installer\CodexUsageWidget.iss"
    }
    opencode = @{
        Name = "OpenCodeUsageWidget"
        Display = "OpenCode Usage Widget"
        Main = Join-Path $RepoRoot "apps\opencode-usage-widget\main.py"
        AppPath = Join-Path $RepoRoot "apps\opencode-usage-widget"
        Installer = Join-Path $RepoRoot "installer\OpenCodeUsageWidget.iss"
    }
}

$IsccCandidates = @(
    "C:\Program Files (x86)\Inno Setup 6\ISCC.exe",
    "C:\Program Files\Inno Setup 6\ISCC.exe",
    (Join-Path $env:LOCALAPPDATA "Programs\Inno Setup 6\ISCC.exe")
)
$IsccExe = $IsccCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1

if ($Sign) {
    if (-not $CertThumbprint) {
        Write-Error "-CertThumbprint is required when using -Sign."
        exit 1
    }
    if (-not (Test-Path $SigntoolExe)) {
        Write-Error "signtool.exe not found at: $SigntoolExe"
        exit 1
    }
    if (-not (Test-Path $KeyInfoFile)) {
        Write-Error "keyinfo.inf not found at: $KeyInfoFile"
        exit 1
    }
}

if ($Installer -and -not $IsccExe) {
    Write-Error "Inno Setup compiler not found. Install it with: winget install JRSoftware.InnoSetup"
    exit 1
}

if ($Installer -and $Version -notmatch '^\d+(\.\d+){1,3}$') {
    Write-Error "Inno Setup requires a numeric version such as 1.2.0 when -Installer is used."
    exit 1
}

if (-not (Test-Path $Python)) {
    Write-Host "Creating build environment at $VenvDir..." -ForegroundColor Yellow
    python -m venv $VenvDir
    if ($LASTEXITCODE -ne 0) { throw "venv creation failed" }
}

Write-Host "Installing build dependencies..." -ForegroundColor Yellow
& $Python -m pip install -r $Requirements pyinstaller --quiet
if ($LASTEXITCODE -ne 0) { throw "dependency installation failed" }

New-Item -ItemType Directory -Force -Path $BuildDir, $DistDir, $ReleaseDir | Out-Null

function Invoke-SignFile {
    param([string]$Path, [string]$Description)
    & $SigntoolExe sign /sha1 $CertThumbprint /tr $TimestampUrl /td SHA256 /fd SHA256 /d $Description $Path
    if ($LASTEXITCODE -ne 0) { throw "signtool failed for $Path" }
    & $SigntoolExe verify /pa $Path
    if ($LASTEXITCODE -ne 0) { throw "signature verification failed for $Path" }
}

function Invoke-AppBuild {
    param([string]$AppKey)

    $Definition = $Applications[$AppKey]
    $Name = $Definition.Name
    $Display = $Definition.Display
    $IconDir = Join-Path $BuildDir "icons"
    $IconFile = Join-Path $IconDir "$Name.ico"
    $WorkPath = Join-Path $BuildDir "pyinstaller\$Name"
    $SpecPath = Join-Path $BuildDir "specs"
    $ExePath = Join-Path $DistDir "$Name.exe"

    Write-Host ""
    Write-Host "============================================================" -ForegroundColor Cyan
    Write-Host "  $Display - release $Version" -ForegroundColor Cyan
    Write-Host "============================================================" -ForegroundColor Cyan

    foreach ($RequiredPath in @($Definition.Main, $Definition.Installer, $CommonPath)) {
        if (-not (Test-Path $RequiredPath)) { throw "Required path not found: $RequiredPath" }
    }

    New-Item -ItemType Directory -Force -Path $IconDir, $SpecPath | Out-Null
    Remove-Item -LiteralPath $IconFile -Force -ErrorAction SilentlyContinue
    & $Python $MakeIcon --app $AppKey $IconFile
    if ($LASTEXITCODE -ne 0) { throw "icon generation failed for $AppKey" }

    Remove-Item -LiteralPath $WorkPath -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $ExePath -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath (Join-Path $SpecPath "$Name.spec") -Force -ErrorAction SilentlyContinue

    $PyInstallerArgs = @(
        "-m", "PyInstaller",
        "--clean",
        "--onefile",
        "--noconsole",
        "--name", $Name,
        "--icon", $IconFile,
        "--distpath", $DistDir,
        "--workpath", $WorkPath,
        "--specpath", $SpecPath,
        "--paths", $CommonPath,
        "--paths", $Definition.AppPath,
        "--hidden-import", "pystray._win32"
    )
    if ($AppKey -eq "claude") {
        $PyInstallerArgs += @(
            "--hidden-import", "truststore",
            "--collect-submodules", "truststore"
        )
    }
    if ($AppKey -eq "codex") {
        $CodexIcon = Join-Path $Definition.AppPath "codex_widget\codex_icon.svg"
        $PyInstallerArgs += @(
            "--add-data", "${CodexIcon};codex_widget"
        )
    }
    if ($AppKey -eq "opencode") {
        $OpenCodeIcon = Join-Path $Definition.AppPath "opencode_widget\opencode_logo.svg"
        $PyInstallerArgs += @(
            "--add-data", "${OpenCodeIcon};opencode_widget"
        )
    }
    $PyInstallerArgs += $Definition.Main
    & $Python @PyInstallerArgs
    if ($LASTEXITCODE -ne 0) { throw "PyInstaller failed for $AppKey" }
    if (-not (Test-Path $ExePath)) { throw "Expected executable not found: $ExePath" }

    if ($Sign) {
        certutil -repairstore -user MY $CertThumbprint $KeyInfoFile
        if ($LASTEXITCODE -ne 0) { throw "certutil -repairstore failed" }
        Invoke-SignFile -Path $ExePath -Description $Display
    }

    $ReleaseExe = Join-Path $ReleaseDir "$Name-$Version.exe"
    Copy-Item -LiteralPath $ExePath -Destination $ReleaseExe -Force
    Write-Host "Portable build: $ReleaseExe" -ForegroundColor Green

    if ($Installer) {
        & $IsccExe "/DAppVersion=$Version" "/DRepoRoot=$RepoRoot" $Definition.Installer
        if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed for $AppKey" }
        $SetupExe = Join-Path $ReleaseDir "$Name-Setup-$Version.exe"
        if (-not (Test-Path $SetupExe)) { throw "Expected installer not found: $SetupExe" }
        if ($Sign) {
            Invoke-SignFile -Path $SetupExe -Description "$Display Setup"
        }
        Write-Host "Installer: $SetupExe" -ForegroundColor Green
    }
}

$Targets = if ($App -eq "all") { @("claude", "codex", "opencode") } else { @($App) }
foreach ($Target in $Targets) {
    Invoke-AppBuild -AppKey $Target
}
