@echo off
cd /d "%~dp0"
where tgsum >nul 2>nul
if %errorlevel%==0 ( tgsum ) else ( npx tgsum )
echo.
pause
