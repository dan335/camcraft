@echo off
rem Build and run the GUI in release mode, via scripts\run.sh.
rem
rem   scripts\run.bat [args...]   any arguments are passed through to the app
rem
rem Uses Git for Windows' bash; a bare "bash" on PATH may be WSL's instead.
setlocal
set "BASH=%ProgramFiles%\Git\bin\bash.exe"
if not exist "%BASH%" set "BASH=bash"
"%BASH%" "%~dp0run.sh" %*
exit /b %ERRORLEVEL%
