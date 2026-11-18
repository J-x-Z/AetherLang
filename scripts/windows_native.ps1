param(
    [Parameter(Mandatory=$true)][string]$Clang,
    [string]$Python = "python",
    [string]$WorkDir = (Join-Path $PSScriptRoot "native-results")
)
$ErrorActionPreference = "Stop"
if ($env:OS -ne "Windows_NT") { throw "Run this entry point on actual Windows x64." }
& $Python (Join-Path $PSScriptRoot "scripts\validate_windows.py") --package $PSScriptRoot --cc $Clang --work-dir $WorkDir
exit $LASTEXITCODE
