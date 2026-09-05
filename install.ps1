$ErrorActionPreference = 'Stop'

$Repo = "agungprasastia/graphia"
$Target = "windows-x64"
$ArchiveName = "graphia-$Target.zip"
$DownloadUrl = "https://github.com/$Repo/releases/latest/download/$ArchiveName"

$AgentHome = if ($env:GRAPHIA_INSTALL_HOME) { $env:GRAPHIA_INSTALL_HOME } else { [Environment]::GetFolderPath("UserProfile") }
$InstallDir = Join-Path $AgentHome ".graphia\bin"
if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}

$TempZip = Join-Path ([System.IO.Path]::GetTempPath()) $ArchiveName

Write-Host "Downloading $DownloadUrl..." -ForegroundColor Cyan
if ($env:GRAPHIA_ARCHIVE_PATH) {
    Copy-Item -LiteralPath $env:GRAPHIA_ARCHIVE_PATH -Destination $TempZip -Force
} else {
    Invoke-WebRequest -Uri $DownloadUrl -OutFile $TempZip -UseBasicParsing
}

Write-Host "Extracting to $InstallDir..." -ForegroundColor Cyan
Expand-Archive -Path $TempZip -DestinationPath $InstallDir -Force
Remove-Item $TempZip -Force

# Add to user PATH if not present
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$InstallDir*") {
    if (-not $env:GRAPHIA_INSTALL_HOME) {
        Write-Host "Adding $InstallDir to User PATH..." -ForegroundColor Yellow
        [Environment]::SetEnvironmentVariable("Path", "$UserPath;$InstallDir", "User")
    }
    $env:Path = "$env:Path;$InstallDir"
}

$ExePath = Join-Path $InstallDir "graphia.exe"
if (Test-Path $ExePath) {
    Write-Host "Successfully installed Graphia!" -ForegroundColor Green
    & $ExePath --version
} else {
    Write-Error "Failed to verify graphia.exe installation in $InstallDir"
}

$SkillSource = Join-Path $InstallDir "skills\graphia"
if (Test-Path (Join-Path $SkillSource "SKILL.md")) {
    $SkillTargets = @(
        (Join-Path $AgentHome ".codex\skills\graphia"),
        (Join-Path $AgentHome ".claude\skills\graphia"),
        (Join-Path $AgentHome ".agents\skills\graphia"),
        (Join-Path $AgentHome ".copilot\skills\graphia"),
        (Join-Path $AgentHome ".config\opencode\skills\graphia")
    )
    foreach ($SkillTarget in $SkillTargets) {
        $SkillParent = Split-Path -Parent $SkillTarget
        New-Item -ItemType Directory -Path $SkillParent -Force | Out-Null
        New-Item -ItemType Directory -Path $SkillTarget -Force | Out-Null
        Copy-Item -Path (Join-Path $SkillSource "*") -Destination $SkillTarget -Recurse -Force
    }
    Write-Host "Installed Graphia skill for Codex, Claude Code, Copilot, OpenCode, and Agent Skills clients." -ForegroundColor Green
} else {
    Write-Warning "Release does not contain skills\graphia; binary installation remains usable."
}
