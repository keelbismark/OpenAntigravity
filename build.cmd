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
set "CUSTOM_PROXY="

:argloop
if "%~1"=="" goto argdone
if /i "%~1"=="--package"  ( set "DO_PACKAGE=1"& shift& goto argloop )
if /i "%~1"=="-p"         ( set "DO_PACKAGE=1"& shift& goto argloop )
if /i "%~1"=="--check"    ( set "DO_CHECK=1"& shift& goto argloop )
if /i "%~1"=="-c"         ( set "DO_CHECK=1"& shift& goto argloop )
if /i "%~1"=="--clean"    ( set "DO_CLEAN=1"& shift& goto argloop )
if /i "%~1"=="--proxy"    ( set "CUSTOM_PROXY=%~2"& shift& shift& goto argloop )
shift
goto argloop
:argdone

if %DO_CLEAN%==1 (
    echo [*] Очистка артефактов сборки...
    if exist "dist_portable" rd /s /q "dist_portable"
    if exist "release" rd /s /q "release"
    cargo clean
    echo [*] Очистка завершена.
    exit /b 0
)

rem --- Определение версии из VERSION ---
if exist "%~dp0VERSION" (
    set /p VERSION=<"%~dp0VERSION"
    set "VERSION=!VERSION: =!"
) else (
    set "VERSION=1.1.3"
)

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
