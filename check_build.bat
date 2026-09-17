@echo off
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
cargo check --package deepseek-tui 2>&1
echo === EXIT CODE: %ERRORLEVEL% ===
if %ERRORLEVEL% equ 0 (echo SUCCESS) else (echo FAILED)
pause
