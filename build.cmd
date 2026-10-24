@echo off
setlocal enabledelayedexpansion

rem ================================================================
rem  Open Antigravity - Единый скрипт сборки и упаковки (Windows)
rem ================================================================
rem  Использование:
rem    build.cmd               - сборка обоих дистрибутивов (Portable + Standard)
rem    build.cmd --portable    - сборка только портативной версии
rem    build.cmd --standard    - сборка только стандартной версии (с инсталлятором)
rem    build.cmd --package     - сборка и упаковка всех релизных архивов в release/
rem    build.cmd --check       - проверка компиляции и прогон всех юнит-тестов
rem    build.cmd --clean       - очистка артефактов сборки
rem ================================================================

cd /d "%~dp0"

set "DO_PACKAGE=0"
set "DO_CHECK=0"
set "DO_CLEAN=0"
set "DO_RELEASE=0"
set "DO_PORTABLE=0"
set "DO_STANDARD=0"
set "NEW_REL_VER="
set "CUSTOM_PROXY="

:argloop
if "%~1"=="" goto argdone
if /i "%~1"=="--release"   ( set "DO_RELEASE=1"& set "NEW_REL_VER=%~2"& shift& shift& goto argloop )
if /i "%~1"=="-r"          ( set "DO_RELEASE=1"& set "NEW_REL_VER=%~2"& shift& shift& goto argloop )
if /i "%~1"=="--package"   ( set "DO_PACKAGE=1"& shift& goto argloop )
if /i "%~1"=="-p"          ( set "DO_PACKAGE=1"& shift& goto argloop )
if /i "%~1"=="--check"     ( set "DO_CHECK=1"& shift& goto argloop )
if /i "%~1"=="-c"          ( set "DO_CHECK=1"& shift& goto argloop )
if /i "%~1"=="--clean"     ( set "DO_CLEAN=1"& shift& goto argloop )
if /i "%~1"=="--portable"  ( set "DO_PORTABLE=1"& shift& goto argloop )
if /i "%~1"=="--standard"  ( set "DO_STANDARD=1"& shift& goto argloop )
if /i "%~1"=="--setup"     ( set "DO_STANDARD=1"& shift& goto argloop )
if /i "%~1"=="--proxy"     ( set "CUSTOM_PROXY=%~2"& shift& shift& goto argloop )
shift
goto argloop
:argdone

rem Если не выбран конкретный режим сборки, собираем оба варианта
if %DO_PORTABLE%==0 if %DO_STANDARD%==0 (
    set "DO_PORTABLE=1"
    set "DO_STANDARD=1"
)

if not %DO_RELEASE%==1 goto :skip_release
if "%NEW_REL_VER%"=="" (
    echo [!] Ошибка: укажите версию для релиза, например: build.cmd --release 1.1.4
    exit /b 1
)
if not exist "%~dp0.git" (
    echo [!] Ошибка: --release доступен только при сборке из git-репозитория.
    exit /b 1
)
git diff-index --quiet HEAD --
if errorlevel 1 (
    echo [!] Ошибка: в репозитории есть незакоммиченные изменения. Сначала закоммитьте их или очистите working tree.
    exit /b 1
)
echo ================================================================
echo   Автоматический релиз Open Antigravity v%NEW_REL_VER%
echo ================================================================
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\bump_version.ps1" "%NEW_REL_VER%"
if errorlevel 1 goto :err
echo [*] Проверка кода и тесты перед релизом...
cargo check
if errorlevel 1 goto :err
cargo test --bin open_antigravity
if errorlevel 1 goto :err
echo [*] Фиксация изменений в git и создание тега...
git add "%~dp0VERSION" "%~dp0Cargo.toml" "%~dp0Cargo.lock" "%~dp0docs\version.json" "%~dp0docs\index.html"
git commit -m "chore(release): bump version to v%NEW_REL_VER%"
if errorlevel 1 goto :err
git tag "v%NEW_REL_VER%"
if errorlevel 1 goto :err
echo [*] Отправка в GitHub (запуск CI/CD релиза)...
git push origin main
if errorlevel 1 (
    echo [!] Ошибка отправки в origin. Убедитесь, что у вас есть права на запись в репозиторий.
    exit /b 1
)
git push origin "v%NEW_REL_VER%"
if errorlevel 1 (
    echo [!] Ошибка отправки тега в origin. Убедитесь, что у вас есть права на запись в репозиторий.
    exit /b 1
)
echo.
echo [*] Релиз v%NEW_REL_VER% успешно запущен в GitHub Actions!
exit /b 0
:skip_release

if %DO_CLEAN%==1 (
    echo [*] Очистка артефактов сборки...
    if exist "dist_portable" rd /s /q "dist_portable"
    if exist "dist_standard" rd /s /q "dist_standard"
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

set "OAG_FULL_VERSION=%VERSION%"
if defined CUSTOM_PROXY (
    set "OAG_BUILTIN_PROXY=%CUSTOM_PROXY%"
)
set "RELDIR=%~dp0release"
if not exist "!RELDIR!" mkdir "!RELDIR!"

rem ================================================================
rem  1. Сборка стандартной версии (с инсталлятором, без OAG_PORTABLE)
rem ================================================================
if %DO_STANDARD%==1 (
    echo ================================================================
    echo   Сборка Open Antigravity v%VERSION% (Windows Standard / Setup)
    echo ================================================================
    set "OAG_PORTABLE="
    cargo build --release
    if errorlevel 1 goto :err

    set "STD_OUTDIR=%~dp0dist_standard\OpenAntigravity_windows_v%VERSION%_setup"
    if not exist "!STD_OUTDIR!" mkdir "!STD_OUTDIR!"

    copy /y "target\release\open_antigravity.exe" "!STD_OUTDIR!\OpenAntigravity.exe" >nul
    copy /y "%~dp0scripts\install.cmd" "!STD_OUTDIR!\install.cmd" >nul
    copy /y "%~dp0scripts\uninstall.cmd" "!STD_OUTDIR!\uninstall.cmd" >nul

    > "!STD_OUTDIR!\README.txt" (
        echo Open Antigravity v%VERSION% (Стандартная версия с установщиком^)
        echo ==============================================================
        echo.
        echo Быстрая установка:
        echo   Запустите OpenAntigravity_windows_v%VERSION%_setup.exe или install.cmd
        echo.
        echo - Приложение установится в %%LOCALAPPDATA%%\Programs\OpenAntigravity.
        echo - Создадутся ярлыки на Рабочем столе и в меню «Пуск».
        echo - Фоновая служба и автозапуск интегрируются в систему.
        echo - Удаление доступно через «Параметры Windows -^> Приложения» или uninstall.cmd.
        echo.
        echo Прямой запуск без установки:
        echo   Можно запустить OpenAntigravity.exe напрямую.
        echo   В этом случае настройки будут храниться в %%LOCALAPPDATA%%\OpenAntigravity.
    )

    echo [*] Стандартная сборка готова: !STD_OUTDIR!

    rem Поиск компилятора Inno Setup (ISCC.exe)
    set "ISCC_EXE="
    where iscc >nul 2>&1 && set "ISCC_EXE=iscc"
    if not defined ISCC_EXE if exist "%ProgramFiles(x86)%\Inno Setup 6\ISCC.exe" set "ISCC_EXE=%ProgramFiles(x86)%\Inno Setup 6\ISCC.exe"
    if not defined ISCC_EXE if exist "%ProgramFiles%\Inno Setup 6\ISCC.exe" set "ISCC_EXE=%ProgramFiles%\Inno Setup 6\ISCC.exe"
    if not defined ISCC_EXE if exist "%LOCALAPPDATA%\Programs\Inno Setup 6\ISCC.exe" set "ISCC_EXE=%LOCALAPPDATA%\Programs\Inno Setup 6\ISCC.exe"

    if defined ISCC_EXE (
        echo [*] Компиляция единого exe-установщика (Inno Setup^)...
        "!ISCC_EXE!" /DMyAppVersion=%VERSION% "%~dp0scripts\installer.iss"
        if errorlevel 1 goto :err

        set "STD_EXE=!RELDIR!\OpenAntigravity_windows_v%VERSION%_setup.exe"
        if exist "!STD_EXE!" (
            echo [*] Exe-установщик готов: !STD_EXE!
            copy /y "!STD_EXE!" "!STD_OUTDIR!\OpenAntigravity_windows_v%VERSION%_setup.exe" >nul
            for /f %%H in ('certutil -hashfile "!STD_EXE!" SHA256 ^| findstr /v ":"') do (
                echo [*] SHA256 (Setup.exe^): %%H
            )
        )
    ) else (
        echo [!] Внимание: Inno Setup не найден. Для генерации .exe установщика установите Inno Setup 6.
    )


)

rem ================================================================
rem  2. Сборка портативной версии (OAG_PORTABLE=1)
rem ================================================================
if %DO_PORTABLE%==1 (
    echo ================================================================
    echo   Сборка Open Antigravity v%VERSION% (Windows Portable)
    echo ================================================================
    set "OAG_PORTABLE=1"
    cargo build --release
    if errorlevel 1 goto :err

    set "PORT_OUTDIR=%~dp0dist_portable\OpenAntigravity_windows_v%VERSION%_portable"
    if not exist "!PORT_OUTDIR!" mkdir "!PORT_OUTDIR!"

    copy /y "target\release\open_antigravity.exe" "!PORT_OUTDIR!\OpenAntigravity.exe" >nul
    type nul > "!PORT_OUTDIR!\portable.flag"

    > "!PORT_OUTDIR!\README.txt" (
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

    echo [*] Портативная сборка готова: !PORT_OUTDIR!

    if %DO_PACKAGE%==1 (
        set "PORT_ZIP=!RELDIR!\OpenAntigravity_windows_v%VERSION%_portable.zip"
        if exist "!PORT_ZIP!" del "!PORT_ZIP!"
        echo [*] Упаковка портативной версии: !PORT_ZIP!
        powershell -NoProfile -Command "Compress-Archive -Path '!PORT_OUTDIR!' -DestinationPath '!PORT_ZIP!' -Force"
        if errorlevel 1 goto :err

        for /f %%H in ('certutil -hashfile "!PORT_ZIP!" SHA256 ^| findstr /v ":"') do (
            echo [*] SHA256 (Portable^): %%H
        )
    )
)

echo.
echo [✓] Готово! Все дистрибутивы собраны.
exit /b 0

:err
echo.
echo [X] ОШИБКА СБОРКИ.
exit /b 1
