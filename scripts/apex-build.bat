@echo off
setlocal enableextensions
rem ===========================================================================
rem ApexLauncher native Windows builder.
rem
rem Runs from the C: mirror created by scripts/build.sh on WSL. Builds entirely
rem on the native NTFS filesystem (no \\wsl.localhost UNC), so incremental
rem compilation works and the toolchain reads source at full speed.
rem
rem Invoked as: apex-build.bat [check^|test^|build^|dev] [extra args...]
rem May also be run directly from a Windows shell inside the mirror.
rem ===========================================================================

rem cd to the repo root (this script lives in scripts\).
cd /d "%~dp0.."

rem Put the Windows toolchains on PATH for this process only.
set "PATH=C:\Users\drgor\.cargo\bin;C:\Program Files\nodejs;%PATH%"

rem Native NTFS supports incremental compilation (the 9p UNC path did not).
set CARGO_INCREMENTAL=1

set "MODE=%~1"
if "%MODE%"=="" set "MODE=test"
shift
rem Collect up to 8 forwarded args (e.g. a cargo test filter).
set "REST=%1 %2 %3 %4 %5 %6 %7 %8 %9"

echo [apex-build] mode=%MODE% dir=%CD%

if /i "%MODE%"=="check" goto check
if /i "%MODE%"=="test"  goto test
if /i "%MODE%"=="build" goto build
if /i "%MODE%"=="dev"   goto dev
echo [apex-build] unknown mode "%MODE%" (use: check^|test^|build^|dev)
exit /b 2

:ensure_node
if not exist node_modules (
  echo [apex-build] npm install...
  call npm.cmd install || exit /b 1
)
exit /b 0

:check
cargo check --manifest-path src-tauri\Cargo.toml || exit /b 1
call :ensure_node || exit /b 1
call npx.cmd tsc --noEmit || exit /b 1
goto done

:test
cargo test --manifest-path src-tauri\Cargo.toml %REST% || exit /b 1
goto done

:build
call :ensure_node || exit /b 1
call npm.cmd run tauri build || exit /b 1
echo [apex-build] bundle output: src-tauri\target\release\bundle
goto done

:dev
call :ensure_node || exit /b 1
call npm.cmd run tauri dev
goto done

:done
echo [apex-build] OK (%MODE%)
exit /b 0
