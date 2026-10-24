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

# 3. Синхронизация docs/index.html (docs/version.json обновляется в CI после завершения сборки релиза)
$indexPath = Join-Path $rootDir "docs\index.html"
if (Test-Path $indexPath) {
    $idxContent = [System.IO.File]::ReadAllText($indexPath)
    $idxContent = $idxContent -replace 'v[0-9]+\.[0-9]+\.[0-9]+', "v$version"
    [System.IO.File]::WriteAllText($indexPath, $idxContent)
}

Write-Host "[*] Версия $version успешно записана в VERSION, Cargo.toml и docs/index.html"
