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

# 3. Синхронизация docs/version.json
$versionJsonPath = Join-Path $rootDir "docs\version.json"
if (Test-Path $versionJsonPath) {
    $json = [ordered]@{
        "version" = $version
        "notes" = "Open Antigravity v$version"
        "page" = "https://github.com/keelbismark/OpenAntigravity/releases/tag/v$version"
        "url_windows" = "https://github.com/keelbismark/OpenAntigravity/releases/download/v$version/OpenAntigravity_windows_v$version`_portable.zip"
        "url_windows_setup" = "https://github.com/keelbismark/OpenAntigravity/releases/download/v$version/OpenAntigravity_windows_v$version`_setup.exe"
        "url_windows_portable" = "https://github.com/keelbismark/OpenAntigravity/releases/download/v$version/OpenAntigravity_windows_v$version`_portable.zip"
        "url_linux" = "https://github.com/keelbismark/OpenAntigravity/releases/download/v$version/OpenAntigravity_linux_v$version.tar.gz"
        "url_appimage" = "https://github.com/keelbismark/OpenAntigravity/releases/download/v$version/OpenAntigravity-x86_64.AppImage"
    }
    $jsonText = $json | ConvertTo-Json -Depth 4
    [System.IO.File]::WriteAllText($versionJsonPath, $jsonText + [Environment]::NewLine)
}

# 4. Синхронизация docs/index.html
$indexPath = Join-Path $rootDir "docs\index.html"
if (Test-Path $indexPath) {
    $idxContent = [System.IO.File]::ReadAllText($indexPath)
    $idxContent = $idxContent -replace 'v[0-9]+\.[0-9]+\.[0-9]+', "v$version"
    [System.IO.File]::WriteAllText($indexPath, $idxContent)
}

Write-Host "[*] Версия $version успешно записана в VERSION, Cargo.toml, docs/version.json и docs/index.html"
