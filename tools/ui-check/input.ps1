# Real mouse and keyboard input for tools/ui-check: a click at a screen point, then keys (SendKeys syntax).
param([int]$X = -1, [int]$Y = -1, [string]$Keys = "", [switch]$Slow)
Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class UiInput {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint x, uint y, uint d, UIntPtr e);
}
"@
if ($X -ge 0) {
  [UiInput]::SetCursorPos($X, $Y) | Out-Null
  [UiInput]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 60; [UiInput]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
  Start-Sleep -Milliseconds 500
}
if ($Slow) {
  foreach ($c in $Keys.ToCharArray()) { [System.Windows.Forms.SendKeys]::SendWait("$c"); Start-Sleep -Milliseconds 120 }
} elseif ($Keys) {
  [System.Windows.Forms.SendKeys]::SendWait($Keys)
}
Start-Sleep -Seconds 2
