# Clicks at screen point (X, Y) with the real mouse, then types Text with real key presses.
param([int]$X, [int]$Y, [string]$Text)
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class In {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint x, uint y, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, System.Text.StringBuilder s, int n);
}
"@
[In]::SetCursorPos($X, $Y) | Out-Null
[In]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero); Start-Sleep -Milliseconds 60; [In]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
Start-Sleep -Milliseconds 500
$sb = New-Object System.Text.StringBuilder 256; [In]::GetWindowText([In]::GetForegroundWindow(), $sb, 256) | Out-Null
"foreground window: '$sb'"
Add-Type -AssemblyName System.Windows.Forms
[System.Windows.Forms.SendKeys]::SendWait($Text)
