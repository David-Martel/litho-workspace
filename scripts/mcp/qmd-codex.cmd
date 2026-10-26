@echo off
setlocal
rem qmd-codex - Codex-compatible wrapper that relays framed MCP to newline JSON
set "NODE_EXE="
if exist "%USERPROFILE%\.local\bin\node.exe" set "NODE_EXE=%USERPROFILE%\.local\bin\node.exe"
if not defined NODE_EXE if exist "%ProgramFiles%\nodejs\node.exe" set "NODE_EXE=%ProgramFiles%\nodejs\node.exe"
if not defined NODE_EXE set "NODE_EXE=node"
"%NODE_EXE%" "%USERPROFILE%\.codex\tools\qmd-mcp\run-litho-qmd-codex.js" %*
endlocal
