@echo off
call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvarsall.bat" x64 >nul 2>&1
set RUSTC_WRAPPER=
set CARGO_TARGET_DIR=T:\RustCache\cargo-target
cd /d C:\codedev\litho-workspace
T:\RustCache\rustup\toolchains\stable-x86_64-pc-windows-msvc\bin\cargo.exe test -p litho-extract
