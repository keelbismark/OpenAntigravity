@echo off
rem ================================================================
rem  Build a portable, key-free Open Antigravity with YOUR proxy baked in.
rem  1) Put your real proxy line into AG_BUILTIN_PROXY below
rem     (format: login:password@host:port, HTTP proxy with CONNECT)
rem  2) Run this file from the repository root (cargo required)
rem ================================================================

rem --- BAKED-IN PROXY (optional: leave empty for clean public build) ---
rem Format: login:password@host:port (or multiple separated by semicolon: proxy1;proxy2)
if not defined AG_BUILTIN_PROXY set "AG_BUILTIN_PROXY="

rem --- PORTABLE MODE BAKED INTO THE BUILD -------------------------
set "AG_PORTABLE=1"

echo Building (release, first build takes 10-20 minutes)...
cargo build --release
if errorlevel 1 goto :err

set "OUTDIR=%~dp0dist_portable"
if not exist "%OUTDIR%" mkdir "%OUTDIR%"
if exist "target\release\open_antigravity.exe" (
    copy /y "target\release\open_antigravity.exe" "%OUTDIR%\OpenAntigravity.exe" >nul
) else (
    copy /y "target\release\ag_unlocker.exe" "%OUTDIR%\OpenAntigravity.exe" >nul
)

rem Ultra-compact portable build: icon is baked into exe resources,
rem AG_PORTABLE=1 is compiled into the binary, diagnostics and shortcuts
rem are directly accessible inside the GUI and via CLI flags.
> "%OUTDIR%\README.txt" (
echo Open Antigravity (Portable^)
echo ============================
echo.
echo Запуск:
echo   OpenAntigravity.exe
echo.
echo - Программа полностью портативна: настройки и кэш живут в папке "data" рядом.
echo - Удаление папки "data" = полный сброс. В систему ничего не ставится.
echo - Встроенный прокси уже активен.
echo - Диагностика, ярлыки и управление доступны прямо в окне программы.
)

echo.
echo Done. Output folder:
echo   %OUTDIR%
pause
exit /b 0

:err
echo BUILD FAILED - see the error text above.
pause
exit /b 1
