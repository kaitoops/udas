@echo off
set VCToolsDir=C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC\14.44.35207
set PATH=%VCToolsDir%\bin\Hostx64\x64;%PATH%
set LIB=%VCToolsDir%\lib\x64
set INCLUDE=%VCToolsDir%\include
where link.exe
echo ---
cargo check --package deepseek-tui 2>&1
if %ERRORLEVEL% neq 0 pause
