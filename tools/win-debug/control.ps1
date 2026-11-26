# Control test: can SendKeys type into an ordinary text box on this machine?
Add-Type -AssemblyName System.Windows.Forms
$f = New-Object System.Windows.Forms.Form; $t = New-Object System.Windows.Forms.TextBox; $f.Controls.Add($t)
$f.Add_Shown({ $f.Activate(); $t.Focus() | Out-Null })
$timer = New-Object System.Windows.Forms.Timer; $timer.Interval = 1500
$timer.Add_Tick({ [System.Windows.Forms.SendKeys]::SendWait("control ok"); Start-Sleep -Milliseconds 500; "control textbox: '$($t.Text)'" | Write-Host; $timer.Stop(); $f.Close() })
$timer.Start(); [void]$f.ShowDialog()
