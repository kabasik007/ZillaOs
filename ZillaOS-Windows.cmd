@echo off
setlocal
cd /d "%~dp0"
title ZillaOS - Windows UEFI ISO Builder

:menu
cls
echo ===============================================================
echo                  ZILLAOS - WINDOWS BUILD MENU
echo ===============================================================
echo  1. Check WSL / Rust / xorriso requirements
echo  2. Install Ubuntu dependencies and pinned Rust [CHANGES WSL]
echo  3. Build ZillaOS UEFI ISO [Rust + Limine + xorriso]
echo  4. Build ISO and open it in VMware Workstation
echo  5. Open existing ISO in isolated VMware VM
echo  6. Open ISO output folder in Windows Explorer
echo  7. Run QEMU graphical boot smoke test [optional QEMU]
echo  8. Setup Ubuntu including QEMU / OVMF [CHANGES WSL]
echo  0. Exit
echo ===============================================================
echo Note: Test OS only. Never installs on physical disks.
echo.

choice /C 123456780 /N /M "Choose (1-8, 0): "
set "pick=%errorlevel%"
if "%pick%"=="9" goto done
if "%pick%"=="1" set "args=-Action Doctor" & goto run
if "%pick%"=="2" set "args=-Action Setup" & goto run
if "%pick%"=="3" set "args=-Action Build" & goto run
if "%pick%"=="4" set "args=-Action BuildRun" & goto run
if "%pick%"=="5" set "args=-Action Run" & goto run
if "%pick%"=="6" set "args=-Action OpenISO" & goto run
if "%pick%"=="7" set "args=-Action Smoke" & goto run
if "%pick%"=="8" set "args=-Action Setup -WithQemu" & goto run
goto menu

:run
echo.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\windows.ps1" %args%
if errorlevel 1 (
  echo.
  echo [ZillaOS] FAILED. Review the error log above before retrying.
) else (
  echo.
  echo [ZillaOS] Command finished successfully.
)
echo.
pause
goto menu

:done
endlocal
exit /b 0
