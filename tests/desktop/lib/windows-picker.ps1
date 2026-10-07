param([ValidateSet('select', 'cancel')] [string]$Action, [int]$HostProcessId)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false
$process = Get-CimInstance Win32_Process -Filter "ProcessId = $HostProcessId"
$expected = Join-Path $PSScriptRoot '../../../target/debug/fleqi-desktop.exe'
if ($process.ExecutablePath -ne [IO.Path]::GetFullPath($expected)) { throw 'Not the owned test binary' }
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type -AssemblyName System.Windows.Forms
$processCondition = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ProcessIdProperty, $HostProcessId)
for ($i = 0; $i -lt 50; $i++) {
  $dialogs = @([System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, $processCondition) | Where-Object { $_.Current.ClassName -eq '#32770' })
  if ($dialogs.Count -eq 1) { break }
  Start-Sleep -Milliseconds 100
}
if ($dialogs.Count -ne 1) { throw 'Owned folder dialog is not unique' }
$dialog = $dialogs[0]
if ($Action -eq 'cancel') {
  $dialog.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).Close()
  exit 0
}
$root = [IO.Path]::GetFullPath($env:FLEQI_WINDOWS_FIXTURE_DIR)
if (-not $root.Contains('\tests\.artifacts\desktop\windows-data-')) { throw 'Not an isolated desktop fixture' }
$target = [IO.Path]::GetFullPath($env:FLEQI_PICKER_TARGET)
if (-not $target.StartsWith($root + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Picker target escaped the fixture' }
if (-not [IO.Directory]::Exists($target)) { throw 'Fixture directory is missing' }
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class FleqiPickerControls {
 [DllImport("user32.dll", EntryPoint="SendMessageW", CharSet=CharSet.Unicode, ExactSpelling=true)] public static extern IntPtr SetText(IntPtr window, uint message, IntPtr wParam, string text);
 [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr dialog, int id);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);
}
'@
$handle = [IntPtr]$dialog.Current.NativeWindowHandle
Start-Sleep -Milliseconds 500
$null = [FleqiPickerControls]::SetForegroundWindow($handle)
if ([FleqiPickerControls]::GetForegroundWindow() -ne $handle) { throw 'Owned picker is not foreground' }
[System.Windows.Forms.SendKeys]::SendWait('%d')
$edits = @($dialog.FindAll([System.Windows.Automation.TreeScope]::Descendants, (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::AutomationIdProperty, '41477'))) | Where-Object { $_.Current.ClassName -eq 'Edit' -and $_.Current.NativeWindowHandle -ne 0 })
if ($edits.Count -ne 1) { throw 'Address edit is not unique' }
$edit = [IntPtr]$edits[0].Current.NativeWindowHandle
if ([FleqiPickerControls]::SetText($edit, 0xC, [IntPtr]::Zero, $target) -eq [IntPtr]::Zero) { throw 'Cannot set the literal address' }
[System.Windows.Forms.SendKeys]::SendWait('{ENTER}')
Start-Sleep -Milliseconds 500
$button = [FleqiPickerControls]::GetDlgItem($handle, 1)
if ($button -eq [IntPtr]::Zero -or -not [FleqiPickerControls]::PostMessageW($button, 0xF5, [IntPtr]::Zero, [IntPtr]::Zero)) { throw 'Select button not found' }
