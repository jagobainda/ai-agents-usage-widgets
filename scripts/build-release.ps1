<#
.SYNOPSIS
    Builds one or all Windows usage widgets and optionally their installers.
.EXAMPLE
    .\scripts\build-release.ps1 -App all -Version "2.0.0" -Installer
#>

param(
    [ValidateSet("claude", "codex", "opencode", "kimi", "antigravity", "all")]
    [string]$App = "claude",
    [switch]$Sign,
    [string]$CertThumbprint = "",
    [string]$KeyInfoFile = "C:\Users\Jagoba\keyinfo.inf",
    [string]$Version = "2.0.0",
    [switch]$Installer
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path $PSScriptRoot -Parent
$ReleaseDir = Join-Path $RepoRoot "releases"
$TimestampUrl = "http://time.certum.pl"
$SigntoolExe = "C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\signtool.exe"

if ($Version -ne "ci" -or $Installer) {
    if ($Version -notmatch '^[0-9]{1,5}(\.[0-9]{1,5}){1,3}\z' -or
        ($Version.Split('.') | Where-Object { [int]$_ -gt 65535 })) {
        throw "Use a numeric version such as 2.0.0. The label 'ci' is available for portable builds."
    }
}

if ($Sign) {
    if (-not $CertThumbprint) { throw "-CertThumbprint is required when using -Sign." }
    if (-not (Test-Path -LiteralPath $SigntoolExe)) { throw "signtool.exe not found at: $SigntoolExe" }
    if (-not (Test-Path -LiteralPath $KeyInfoFile)) { throw "keyinfo.inf not found at: $KeyInfoFile" }
    $KeyInfoFile = (Resolve-Path -LiteralPath $KeyInfoFile).Path
}

if ($Installer) {
    $IsccCandidates = @(
        "C:\Program Files (x86)\Inno Setup 6\ISCC.exe",
        "C:\Program Files\Inno Setup 6\ISCC.exe",
        (Join-Path $env:LOCALAPPDATA "Programs\Inno Setup 6\ISCC.exe")
    )
    $IsccExe = $IsccCandidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
    if (-not $IsccExe) {
        throw "Inno Setup compiler not found. Install it with: winget install JRSoftware.InnoSetup"
    }
}

function Invoke-SignFile {
    param([string]$Path, [string]$Description)
    & $SigntoolExe sign /sha1 $CertThumbprint /tr $TimestampUrl /td SHA256 /fd SHA256 /d $Description $Path
    if ($LASTEXITCODE -ne 0) { throw "signtool failed for $Path" }
    & $SigntoolExe verify /pa $Path
    if ($LASTEXITCODE -ne 0) { throw "signature verification failed for $Path" }
}

$Targets = if ($App -eq "all") { @("claude", "codex", "opencode", "kimi", "antigravity") } else { @($App) }
Push-Location -LiteralPath $RepoRoot
try {
    $MetadataJson = cargo metadata --format-version 1 --no-deps --locked
    if ($LASTEXITCODE -ne 0) { throw "cargo metadata failed." }
    $Metadata = $MetadataJson | ConvertFrom-Json
    $Packages = foreach ($Target in $Targets) {
        $Package = $Metadata.packages | Where-Object name -eq "$Target-usage-widget"
        if (-not $Package) { throw "Cargo package not found for $Target." }
        $Package
    }
    $WorkspaceVersion = $Metadata.packages | Where-Object name -eq "widget-common" | Select-Object -ExpandProperty version
    if ($Version -ne "ci" -and $Version -cne $WorkspaceVersion) {
        Write-Warning "Requested release version '$Version' differs from the Cargo workspace version '$WorkspaceVersion'. The executables keep the workspace version."
    }

    if ($App -eq "all") {
        cargo build --release --locked --workspace
    } else {
        cargo build --release --locked -p "$App-usage-widget"
    }
    if ($LASTEXITCODE -ne 0) { throw "Cargo release build failed." }

    New-Item -ItemType Directory -Force -Path $ReleaseDir | Out-Null
    if ($Sign) {
        certutil -repairstore -user MY $CertThumbprint $KeyInfoFile
        if ($LASTEXITCODE -ne 0) { throw "certutil -repairstore failed" }
    }

    foreach ($Package in $Packages) {
        $Binary = $Package.targets | Where-Object { $_.kind -contains "bin" }
        $Name = $Binary.name
        $ExePath = Join-Path $Metadata.target_directory "release\$Name.exe"
        $ReleaseExe = Join-Path $ReleaseDir "$Name-$Version.exe"
        if (-not (Test-Path -LiteralPath $ExePath)) { throw "Expected executable not found: $ExePath" }
        Copy-Item -LiteralPath $ExePath -Destination $ReleaseExe -Force
        $InstallerSource = $ExePath
        if ($Sign) {
            Invoke-SignFile -Path $ReleaseExe -Description $Package.description
            $InstallerSource = $ReleaseExe
        }
        Write-Host "Portable build: $ReleaseExe" -ForegroundColor Green

        if ($Installer) {
            $InstallerPath = Join-Path $RepoRoot "installer\$Name.iss"
            & $IsccExe "/DAppVersion=$Version" "/DRepoRoot=$RepoRoot" "/DAppSource=$InstallerSource" $InstallerPath
            if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed for $Name" }
            $SetupExe = Join-Path $ReleaseDir "$Name-Setup-$Version.exe"
            if (-not (Test-Path -LiteralPath $SetupExe)) { throw "Expected installer not found: $SetupExe" }
            if ($Sign) {
                Invoke-SignFile -Path $SetupExe -Description "$($Package.description) Setup"
            }
            Write-Host "Installer: $SetupExe" -ForegroundColor Green
        }
    }
} finally {
    Pop-Location
}
