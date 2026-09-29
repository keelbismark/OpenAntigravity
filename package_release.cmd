@echo off
rem ================================================================
rem  Release packager for AGUnlocker (Windows).
rem
rem  Builds the binary, packages dist_portable into a versioned ZIP,
rem  calculates SHA256, and optionally generates version.json.
rem
rem  Usage:
rem    package_release.cmd                              (build + package)
rem    package_release.cmd --skip-build                 (package only)
rem    package_release.cmd --version 2.17.0.4           (explicit version)
rem    package_release.cmd --site https://example.com   (URLs for version.json)
rem ================================================================
setlocal enabledelayedexpansion

cd /d "%~dp0"

rem --- Parse arguments -------------------------------------------
set SKIP_BUILD=0
set "VERSION="
set "SITE_URL="
set "NOTES="

:argloop
if "%~1"=="" goto argdone
if /i "%~1"=="--skip-build" ( set SKIP_BUILD=1& shift& goto argloop )
if /i "%~1"=="--version"    ( set "VERSION=%~2"& shift& shift& goto argloop )
if /i "%~1"=="--site"       ( set "SITE_URL=%~2"& shift& shift& goto argloop )
if /i "%~1"=="--notes"      ( set "NOTES=%~2"& shift& shift& goto argloop )
echo Unknown argument: %~1 >&2
exit /b 1
:argdone

rem --- Version ---------------------------------------------------
if "%VERSION%"=="" (
    if defined AG_FULL_VERSION (
        set "VERSION=%AG_FULL_VERSION%"
    ) else if exist "%~dp0VERSION" (
        set /p VERSION=<"%~dp0VERSION"
    ) else (
        set "VERSION=1.1.0"
    )
)

set "DISTDIR=%~dp0dist_portable"

rem --- Stage 1: Build -------------------------------------------
if %SKIP_BUILD%==0 (
    echo ==============================================================
    echo   Stage 1: Building portable distribution
    echo ==============================================================
    if defined VERSION set "AG_FULL_VERSION=%VERSION%"
    call build_portable.cmd
    if errorlevel 1 goto :err
    echo.
)

rem --- Validate dist contents ------------------------------------
set "EXE_NAME="
if exist "%DISTDIR%\OpenAntigravity.exe" set "EXE_NAME=OpenAntigravity.exe"
if exist "%DISTDIR%\AGUnlocker.exe" set "EXE_NAME=AGUnlocker.exe"
if "%EXE_NAME%"=="" (
    echo ERROR: OpenAntigravity.exe not found in %DISTDIR%. Build first. >&2
    exit /b 1
)

echo ==============================================================
echo   Stage 2: Packaging release v%VERSION%
echo ==============================================================

rem --- Create release directory -----------------------------------
set "RELDIR=%~dp0release"
if not exist "%RELDIR%" mkdir "%RELDIR%"

set "ARCHIVE_NAME=OpenAntigravity_windows_v%VERSION%.zip"
set "ARCHIVE_PATH=%RELDIR%\%ARCHIVE_NAME%"

rem --- Zip using PowerShell (available on Win10+) -----------------
if exist "%ARCHIVE_PATH%" del "%ARCHIVE_PATH%"

rem Create a staging folder to get a nice top-level dir in the zip
set "STAGING=%TEMP%\OpenAntigravity_windows_v%VERSION%"
if exist "%STAGING%" rd /s /q "%STAGING%"
mkdir "%STAGING%"
xcopy /s /e /y "%DISTDIR%\*" "%STAGING%\" >nul

echo Packaging: %ARCHIVE_PATH%
powershell -NoProfile -Command ^
  "Compress-Archive -Path '%STAGING%' -DestinationPath '%ARCHIVE_PATH%' -Force"
if errorlevel 1 (
    echo ERROR: Failed to create ZIP. Ensure PowerShell is available. >&2
    goto :err
)
rd /s /q "%STAGING%"

rem --- SHA256 -----------------------------------------------------
set "SHA_PATH=%ARCHIVE_PATH%.sha256"
for /f %%H in ('certutil -hashfile "%ARCHIVE_PATH%" SHA256 ^| findstr /v "hash"') do (
    set "SHA=%%H"
    goto :sha_done
)
:sha_done
echo %SHA%> "%SHA_PATH%"
echo SHA256:    %SHA%

rem --- version.json (if --site given) -----------------------------
if not "%SITE_URL%"=="" (
    rem Strip trailing slash
    if "%SITE_URL:~-1%"=="/" set "SITE_URL=%SITE_URL:~0,-1%"

    > "%RELDIR%\version.json" (
        echo {
        echo   "version": "%VERSION%",
        echo   "notes": "%NOTES%",
        echo   "page": "%SITE_URL%/",
        echo   "url_windows": "%SITE_URL%/AGUnlocker_windows_v%VERSION%.zip",
        echo   "url_linux": "%SITE_URL%/AGUnlocker_linux_v%VERSION%.tar.gz"
        echo }
    )
    echo version.json generated: %RELDIR%\version.json
)

rem --- Summary ----------------------------------------------------
echo.
echo ==============================================================
echo   Release v%VERSION% ready!
echo ==============================================================
echo.
echo   Archive:   %ARCHIVE_PATH%
echo   SHA256:    %SHA%
if not "%SITE_URL%"=="" (
echo   Feed:      %RELDIR%\version.json
)
echo.
echo   Next steps:
echo     1. Upload %ARCHIVE_NAME% to your site
if not "%SITE_URL%"=="" (
echo     2. Upload version.json alongside it
echo     3. Users will see the "New version" banner within 8 hours
) else (
echo     2. For auto-update: re-run with --site https://your-site.com
)
echo.
pause
exit /b 0

:err
echo.
echo PACKAGING FAILED.
pause
exit /b 1
