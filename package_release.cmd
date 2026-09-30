@echo off
rem Обратная совместимость: перенаправление на единый build.cmd --package
call "%~dp0build.cmd" --package %*
exit /b %errorlevel%
