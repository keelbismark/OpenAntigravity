@echo off
chcp 65001 >nul
setlocal enabledelayedexpansion

title Установка Open Antigravity

echo ================================================================
echo   Установка Open Antigravity
echo ================================================================
echo.

set "INSTALL_DIR=%LOCALAPPDATA%\Programs\OpenAntigravity"
set "SRC_DIR=%~dp0"
set "EXE_NAME=OpenAntigravity.exe"

if not exist "%SRC_DIR%%EXE_NAME%" (
    echo [!] Ошибка: файл %EXE_NAME% не найден в папке с установщиком:
    echo     %SRC_DIR%
    pause
    exit /b 1
)

echo [*] Завершение запущенных копий приложения...
taskkill /F /IM "%EXE_NAME%" >nul 2>&1
timeout /t 1 /nobreak >nul 2>&1

echo [*] Создание каталога установки:
echo     %INSTALL_DIR%
if not exist "%INSTALL_DIR%" mkdir "%INSTALL_DIR%"

echo [*] Копирование файлов программы...
copy /y "%SRC_DIR%%EXE_NAME%" "%INSTALL_DIR%\%EXE_NAME%" >nul
if errorlevel 1 (
    echo [!] Ошибка при копировании файлов. Проверьте права доступа.
    pause
    exit /b 1
)

if exist "%SRC_DIR%uninstall.cmd" (
    copy /y "%SRC_DIR%uninstall.cmd" "%INSTALL_DIR%\uninstall.cmd" >nul
)
if exist "%SRC_DIR%README.txt" (
    copy /y "%SRC_DIR%README.txt" "%INSTALL_DIR%\README.txt" >nul
)

echo [*] Создание ярлыков (Рабочий стол и меню «Пуск»)...
powershell -NoProfile -ExecutionPolicy Bypass -Command ^
  "$ws = New-Object -ComObject WScript.Shell; " ^
  "$exe = '%INSTALL_DIR%\%EXE_NAME%'.Replace('\', '\\'); " ^
  "$dir = '%INSTALL_DIR%'.Replace('\', '\\'); " ^
  "$desk = [Environment]::GetFolderPath('Desktop'); " ^
  "$s1 = $ws.CreateShortcut(\"$desk\Open Antigravity.lnk\"); " ^
  "$s1.TargetPath = $exe; $s1.WorkingDirectory = $dir; $s1.IconLocation = \"$exe,0\"; $s1.Description = 'Open Antigravity'; $s1.Save(); " ^
  "$start = [Environment]::GetFolderPath('Programs'); " ^
  "$s2 = $ws.CreateShortcut(\"$start\Open Antigravity.lnk\"); " ^
  "$s2.TargetPath = $exe; $s2.WorkingDirectory = $dir; $s2.IconLocation = \"$exe,0\"; $s2.Description = 'Open Antigravity'; $s2.Save();"

echo [*] Регистрация в списке установленных программ Windows...
set "UNINSTALL_REG=HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\OpenAntigravity"
reg add "%UNINSTALL_REG%" /v "DisplayName" /d "Open Antigravity" /f >nul
reg add "%UNINSTALL_REG%" /v "Publisher" /d "keelbismark" /f >nul
reg add "%UNINSTALL_REG%" /v "DisplayIcon" /d "%INSTALL_DIR%\%EXE_NAME%,0" /f >nul
reg add "%UNINSTALL_REG%" /v "InstallLocation" /d "%INSTALL_DIR%" /f >nul
reg add "%UNINSTALL_REG%" /v "UninstallString" /d "\"%INSTALL_DIR%\uninstall.cmd\"" /f >nul
reg add "%UNINSTALL_REG%" /v "NoModify" /t REG_DWORD /d 1 /f >nul
reg add "%UNINSTALL_REG%" /v "NoRepair" /t REG_DWORD /d 1 /f >nul

echo.
echo ================================================================
echo   [✓] Open Antigravity успешно установлен!
echo ================================================================
echo.
echo Путь установки: %INSTALL_DIR%\%EXE_NAME%
echo Ярлыки добавлены на Рабочий стол и в меню «Пуск».
echo Удалить приложение всегда можно через «Параметры Windows -> Приложения».
echo.

set /p "LAUNCH=Запустить Open Antigravity прямо сейчас? (Y/N, Enter = Да): "
if /i "%LAUNCH%"=="" set "LAUNCH=Y"
if /i "%LAUNCH%"=="Y" (
    start "" "%INSTALL_DIR%\%EXE_NAME%"
)

exit /b 0
