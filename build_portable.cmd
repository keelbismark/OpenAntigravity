@echo off
rem Обратная совместимость: перенаправление на единый build.cmd
call "%~dp0build.cmd" %*
exit /b %errorlevel%
