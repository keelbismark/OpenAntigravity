@echo off
chcp 65001 >nul
setlocal enabledelayedexpansion

title Удаление Open Antigravity

echo ================================================================
echo   Удаление Open Antigravity
echo ================================================================
echo.

set /p "CONFIRM=Вы действительно хотите удалить Open Antigravity? (Y/N): "
if /i not "%CONFIRM%"=="Y" (
    echo [*] Удаление отменено.
    pause
    exit /b 0
)

set "INSTALL_DIR=%LOCALAPPDATA%\Programs\OpenAntigravity"

echo.
echo [*] Завершение процессов программы...
taskkill /F /IM "OpenAntigravity.exe" >nul 2>&1
taskkill /F /IM "OpenAntigravityRelay.exe" >nul 2>&1
timeout /t 1 /nobreak >nul 2>&1

echo [*] Удаление системных служб и фоновых задач...
schtasks /End /TN "OpenAntigravityRelay" >nul 2>&1
schtasks /Delete /TN "OpenAntigravityRelay" /F >nul 2>&1
schtasks /End /TN "OpenAntigravityWatchdog" >nul 2>&1
schtasks /Delete /TN "OpenAntigravityWatchdog" /F >nul 2>&1

if exist "%ProgramData%\OpenAntigravity" (
    rd /s /q "%ProgramData%\OpenAntigravity" >nul 2>&1
)

echo [*] Удаление ярлыков с Рабочего стола и меню «Пуск»...
if exist "%USERPROFILE%\Desktop\Open Antigravity.lnk" (
    del /f /q "%USERPROFILE%\Desktop\Open Antigravity.lnk" >nul 2>&1
)
if exist "%APPDATA%\Microsoft\Windows\Start Menu\Programs\Open Antigravity.lnk" (
    del /f /q "%APPDATA%\Microsoft\Windows\Start Menu\Programs\Open Antigravity.lnk" >nul 2>&1
)

echo [*] Удаление записей из реестра...
reg delete "HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\OpenAntigravity" /f >nul 2>&1
reg delete "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v "Open Antigravity" /f >nul 2>&1

echo.
set /p "DEL_DATA=Удалить пользовательские настройки и профили (%LOCALAPPDATA%\OpenAntigravity)? (Y/N): "
if /i "%DEL_DATA%"=="Y" (
    if exist "%LOCALAPPDATA%\OpenAntigravity" (
        rd /s /q "%LOCALAPPDATA%\OpenAntigravity" >nul 2>&1
        echo [*] Настройки и кэш удалены.
    )
)

echo.
echo ================================================================
echo   [✓] Open Antigravity успешно удалён с компьютера.
echo ================================================================
echo.

rem Самоудаление каталога программы после завершения скрипта
start "" cmd /c "timeout /t 1 /nobreak >nul & rd /s /q \"%INSTALL_DIR%\" >nul 2>&1"
exit /b 0
