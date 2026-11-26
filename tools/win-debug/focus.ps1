# Prints which native window is active and which has keyboard focus.
Add-Type @"
using System; using System.Runtime.InteropServices; using System.Text;
public static class F {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int l, t, r, b; }
  [StructLayout(LayoutKind.Sequential)] public struct GUITHREADINFO { public int cbSize, flags; public IntPtr hwndActive, hwndFocus, hwndCapture, hwndMenuOwner, hwndMoveSize, hwndCaret; public RECT rcCaret; }
  [DllImport("user32.dll")] public static extern bool GetGUIThreadInfo(uint id, ref GUITHREADINFO info);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  public static string D(IntPtr h) { if (h == IntPtr.Zero) return "none"; var c = new StringBuilder(256); var t = new StringBuilder(256); GetClassName(h, c, 256); GetWindowText(h, t, 256); RECT r; GetWindowRect(h, out r);
    return String.Format("{0} [{1}] '{2}' {3},{4} {5}x{6}", h, c, t, r.l, r.t, r.r - r.l, r.b - r.t); }
}
"@
$i = New-Object F+GUITHREADINFO; $i.cbSize = [Runtime.InteropServices.Marshal]::SizeOf($i)
[F]::GetGUIThreadInfo(0, [ref]$i) | Out-Null
"$($args[0]) foreground=$([F]::D([F]::GetForegroundWindow())) active=$([F]::D($i.hwndActive)) focus=$([F]::D($i.hwndFocus))"
