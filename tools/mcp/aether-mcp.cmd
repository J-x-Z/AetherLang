@echo off
setlocal
if not defined AETHER_MCP_PYTHON set "AETHER_MCP_PYTHON=python.exe"
"%AETHER_MCP_PYTHON%" -B "%~dp0tools\mcp\aether_mcp.py" %*
exit /b %errorlevel%
