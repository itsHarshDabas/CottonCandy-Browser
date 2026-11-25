# Installs the Windows SDK version this Chromium asks for (build/vs_toolchain.py), with the Debugging
# Tools it also needs, when the runner image doesn't have it.
param([string]$Work)
$ErrorActionPreference = 'Stop'
$toolchain = Get-Content (Join-Path $Work 'chromium\src\build\vs_toolchain.py') -Raw
$version = [regex]::Match($toolchain, "SDK_VERSION\s*=\s*'([0-9.]+)'").Groups[1].Value
if (-not $version) { throw 'No SDK_VERSION in build/vs_toolchain.py' }
$kits = 'C:\Program Files (x86)\Windows Kits\10'
Write-Output "Chromium wants Windows SDK $version"
if (Test-Path (Join-Path $kits "Include\$version\um")) {
  Write-Output 'Already installed.'
} else {
  $short = ($version -split '\.')[0..2] -join '.'
  winget install --id "Microsoft.WindowsSDK.$short" --exact --silent --disable-interactivity `
    --accept-package-agreements --accept-source-agreements
  if (-not (Test-Path (Join-Path $kits "Include\$version\um"))) {
    Get-ChildItem (Join-Path $kits 'Include') | Format-Table Name
    throw "Windows SDK $version is still missing after installing Microsoft.WindowsSDK.$short"
  }
}
if (-not (Test-Path (Join-Path $kits 'Debuggers\x64\dbghelp.dll'))) {
  throw 'The Debugging Tools for Windows (dbghelp.dll) are missing'
}
Get-ChildItem (Join-Path $kits 'Include') | Format-Table Name
