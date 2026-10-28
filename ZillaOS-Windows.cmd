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
set "args="
if "%pick%"=="9" goto done
if "%pick%"=="1" set "args=-Action Doctor"
if "%pick%"=="2" set "args=-Action Setup"
if "%pick%"=="3" set "args=-Action Build"
if "%pick%"=="4" set "args=-Action BuildRun"
if "%pick%"=="5" set "args=-Action Run"
if "%pick%"=="6" set "args=-Action OpenISO"
if "%pick%"=="7" set "args=-Action Smoke"
if "%pick%"=="8" set "args=-Action Setup -WithQemu"
if not defined args goto menu
goto run

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
