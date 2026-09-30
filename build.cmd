@echo off
setlocal enabledelayedexpansion

rem ================================================================
rem  Open Antigravity - Единый скрипт сборки и упаковки (Windows)
rem ================================================================
rem  Использование:
rem    build.cmd               - сборка портативного дистрибутива в dist_portable/
rem    build.cmd --package     - сборка и упаковка в release/OpenAntigravity_windows_v{VERSION}.zip
rem    build.cmd --check       - проверка компиляции и прогон всех юнит-тестов
rem    build.cmd --clean       - очистка артефактов сборки
rem ================================================================

cd /d "%~dp0"

set "DO_PACKAGE=0"
set "DO_CHECK=0"
set "DO_CLEAN=0"
set "DO_RELEASE=0"
set "NEW_REL_VER="
set "CUSTOM_PROXY="

:argloop
if "%~1"=="" goto argdone
if /i "%~1"=="--release"  ( set "DO_RELEASE=1"& set "NEW_REL_VER=%~2"& shift& shift& goto argloop )
if /i "%~1"=="-r"         ( set "DO_RELEASE=1"& set "NEW_REL_VER=%~2"& shift& shift& goto argloop )
if /i "%~1"=="--package"  ( set "DO_PACKAGE=1"& shift& goto argloop )
if /i "%~1"=="-p"         ( set "DO_PACKAGE=1"& shift& goto argloop )
if /i "%~1"=="--check"    ( set "DO_CHECK=1"& shift& goto argloop )
if /i "%~1"=="-c"         ( set "DO_CHECK=1"& shift& goto argloop )
if /i "%~1"=="--clean"    ( set "DO_CLEAN=1"& shift& goto argloop )
if /i "%~1"=="--proxy"    ( set "CUSTOM_PROXY=%~2"& shift& shift& goto argloop )
shift
goto argloop
:argdone

if not %DO_RELEASE%==1 goto :skip_release
if "%NEW_REL_VER%"=="" (
    echo [!] Ошибка: укажите версию для релиза, например: build.cmd --release 1.1.4
    exit /b 1
)
echo ================================================================
echo   Автоматический релиз Open Antigravity v%NEW_REL_VER%
echo ================================================================
powershell -NoProfile -ExecutionPolicy Bypass -Command "$v = '%NEW_REL_VER%'.Trim(); [System.IO.File]::WriteAllText('%~dp0VERSION', $v + [Environment]::NewLine); $c = Get-Content '%~dp0Cargo.toml' -Raw; ($c -replace '(?m)^version = \"[^\"]+\"', ('version = \"' + $v + '\"')) | Set-Content '%~dp0Cargo.toml' -NoNewline"
echo [*] Проверка кода и тесты перед релизом...
cargo check
if errorlevel 1 goto :err
cargo test --bin open_antigravity
if errorlevel 1 goto :err
echo [*] Фиксация изменений в git и создание тега...
git add "%~dp0VERSION" "%~dp0Cargo.toml" "%~dp0Cargo.lock"
git commit -m "chore(release): bump version to v%NEW_REL_VER%"
git tag "v%NEW_REL_VER%"
echo [*] Отправка в GitHub (запуск CI/CD релиза)...
git push origin main
git push origin "v%NEW_REL_VER%"
echo.
echo [*] Релиз v%NEW_REL_VER% успешно запущен в GitHub Actions!
exit /b 0
:skip_release

if %DO_CLEAN%==1 (
    echo [*] Очистка артефактов сборки...
    if exist "dist_portable" rd /s /q "dist_portable"
    if exist "release" rd /s /q "release"
    cargo clean
    echo [*] Очистка завершена.
    exit /b 0
)

rem --- Определение версии из файла VERSION (SSOT) ---
if not exist "%~dp0VERSION" (
    echo [!] Ошибка: файл VERSION не найден в корне проекта.
    exit /b 1
)
set /p VERSION=<"%~dp0VERSION"
set "VERSION=!VERSION: =!"

rem --- Автоматическая синхронизация версии в Cargo.toml из VERSION ---
if not exist "%~dp0Cargo.toml" goto :skip_cargosync
powershell -NoProfile -ExecutionPolicy Bypass -Command "$v = (Get-Content '%~dp0VERSION').Trim(); $c = Get-Content '%~dp0Cargo.toml'; if ($c -match '(?m)^version = \"([^\"]+)\"' -and $Matches[1] -ne $v) { ($c -replace '(?m)^version = \"[^\"]+\"', ('version = \"' + $v + '\"')) | Set-Content '%~dp0Cargo.toml'; Write-Host ('[*] Синхронизирована версия Cargo.toml -> ' + $v) }"
:skip_cargosync

if %DO_CHECK%==1 (
    echo ================================================================
    echo   Проверка и тесты: Open Antigravity v%VERSION%
    echo ================================================================
    cargo check
    if errorlevel 1 goto :err
    cargo test --bin open_antigravity
    if errorlevel 1 goto :err
    echo.
    echo [*] Все проверки и тесты успешно пройдены!
    exit /b 0
)

rem --- Настройка окружения для портативной сборки ---
set "AG_PORTABLE=1"
set "AG_FULL_VERSION=%VERSION%"
if defined CUSTOM_PROXY (
    set "AG_BUILTIN_PROXY=%CUSTOM_PROXY%"
)

echo ================================================================
echo   Сборка Open Antigravity v%VERSION% (Windows Portable)
echo ================================================================

cargo build --release
if errorlevel 1 goto :err

set "OUTDIR=%~dp0dist_portable\OpenAntigravity_windows_v%VERSION%"
if not exist "%OUTDIR%" mkdir "%OUTDIR%"

if exist "target\release\open_antigravity.exe" (
    copy /y "target\release\open_antigravity.exe" "%OUTDIR%\OpenAntigravity.exe" >nul
) else (
    echo [!] Ошибка: целевой файл open_antigravity.exe не найден.
    goto :err
)

> "%OUTDIR%\README.txt" (
    echo Open Antigravity v%VERSION% (Портативная версия^)
    echo ================================================
    echo.
    echo Запуск:
    echo   OpenAntigravity.exe
    echo.
    echo - Программа полностью портативна: настройки и кэш сохраняются в папке "data" рядом с программой.
    echo - Удаление папки "data" = полный сброс. В систему ничего лишнего не устанавливается.
    echo - Встроенный прокси уже активен по умолчанию.
    echo - Управление и диагностика доступны прямо в интерфейсе программы.
)

echo [*] Портативная сборка готова: %OUTDIR%

if %DO_PACKAGE%==1 (
    echo.
    echo ================================================================
    echo   Упаковка релиза в ZIP
    echo ================================================================
    set "RELDIR=%~dp0release"
    if not exist "!RELDIR!" mkdir "!RELDIR!"
    
    set "ZIP_PATH=!RELDIR!\OpenAntigravity_windows_v%VERSION%.zip"
    if exist "!ZIP_PATH!" del "!ZIP_PATH!"
    
    echo [*] Сжатие в архив: !ZIP_PATH!
    powershell -NoProfile -Command "Compress-Archive -Path '%OUTDIR%' -DestinationPath '!ZIP_PATH!' -Force"
    if errorlevel 1 goto :err
    
    set "SHA_PATH=!ZIP_PATH!.sha256"
    for /f %%H in ('certutil -hashfile "!ZIP_PATH!" SHA256 ^| findstr /v ":"') do (
        set "SHA=%%H"
        goto :sha_done
    )
    :sha_done
    echo !SHA!> "!SHA_PATH!"
    echo [*] SHA256: !SHA!
    echo [*] Архив релиза готов: !ZIP_PATH!
)

echo.
echo [✓] Готово!
exit /b 0

:err
echo.
echo [X] ОШИБКА СБОРКИ.
exit /b 1
