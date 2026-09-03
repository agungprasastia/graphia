$ErrorActionPreference = 'Stop'

$Repo = "agungprasastia/graphia"
$Target = "windows-x64"
$ArchiveName = "graphia-$Target.zip"
$DownloadUrl = "https://github.com/$Repo/releases/latest/download/$ArchiveName"

$InstallDir = Join-Path $HOME ".graphia\bin"
if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}

$TempZip = Join-Path ([System.IO.Path]::GetTempPath()) $ArchiveName

Write-Host "Downloading $DownloadUrl..." -ForegroundColor Cyan
Invoke-WebRequest -Uri $DownloadUrl -OutFile $TempZip -UseBasicParsing

Write-Host "Extracting to $InstallDir..." -ForegroundColor Cyan
Expand-Archive -Path $TempZip -DestinationPath $InstallDir -Force
Remove-Item $TempZip -Force

# Add to user PATH if not present
$UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($UserPath -notlike "*$InstallDir*") {
    Write-Host "Adding $InstallDir to User PATH..." -ForegroundColor Yellow
    [Environment]::SetEnvironmentVariable("Path", "$UserPath;$InstallDir", "User")
    $env:Path = "$env:Path;$InstallDir"
}

$ExePath = Join-Path $InstallDir "graphia.exe"
if (Test-Path $ExePath) {
    Write-Host "Successfully installed Graphia!" -ForegroundColor Green
    & $ExePath --version
} else {
    Write-Error "Failed to verify graphia.exe installation in $InstallDir"
}
