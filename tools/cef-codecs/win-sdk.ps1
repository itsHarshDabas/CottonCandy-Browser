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
  # Microsoft's installer for that SDK, as listed in winget's catalogue (winget's own silent switches
  # make it fail with E_INVALIDARG). All features: the headers and libraries plus the Debugging Tools.
  $installers = @{
    '10.0.28000.0' = 'https://download.microsoft.com/download/e6cfffdc-0d3e-4c6c-96d7-d491da459a07/KIT_BUNDLE_WINDOWSSDK_MEDIACREATION/winsdksetup.exe'
  }
  $url = $installers[$version]
  if (-not $url) { throw "No installer known for Windows SDK $version: add it to win-sdk.ps1" }
  $setup = Join-Path $env:RUNNER_TEMP 'winsdksetup.exe'
  Invoke-WebRequest $url -OutFile $setup
  $p = Start-Process $setup -ArgumentList '/features', '+', '/quiet', '/norestart', '/ceip', 'off', '/log', (Join-Path $env:RUNNER_TEMP 'winsdk.log') -Wait -PassThru
  Write-Output "winsdksetup exit code: $($p.ExitCode)"
  if (-not (Test-Path (Join-Path $kits "Include\$version\um"))) {
    Get-ChildItem (Join-Path $kits 'Include') | Format-Table Name
    Get-Content (Join-Path $env:RUNNER_TEMP 'winsdk.log') -Tail 40 -ErrorAction SilentlyContinue
    throw "Windows SDK $version is still missing after running its installer"
  }
}
if (-not (Test-Path (Join-Path $kits 'Debuggers\x64\dbghelp.dll'))) {
  throw 'The Debugging Tools for Windows (dbghelp.dll) are missing'
}
Get-ChildItem (Join-Path $kits 'Include') | Format-Table Name
