param(
    [Parameter(Mandatory=$true)]
    [string]$Version
)

$rootDir = Split-Path -Parent $PSScriptRoot
$version = $Version.Trim()

if (-not $version) {
    Write-Error "Версия не может быть пустой."
    exit 1
}

# 1. Запись единого источника версии VERSION
$versionPath = Join-Path $rootDir "VERSION"
[System.IO.File]::WriteAllText($versionPath, $version + [Environment]::NewLine)

# 2. Синхронизация версии в Cargo.toml
$cargoPath = Join-Path $rootDir "Cargo.toml"
$content = [System.IO.File]::ReadAllText($cargoPath)
$updated = $content -replace '(?m)^version = "[^"]+"', "version = `"$version`""
[System.IO.File]::WriteAllText($cargoPath, $updated)

Write-Host "[*] Версия $version успешно записана в VERSION и Cargo.toml"
