param([ValidateSet('open','select','move','tab','close')] [string]$Action, [string]$Folder = 'A', [string]$File = 'one.txt', [long]$WindowId = 0)
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false
$root = [IO.Path]::GetFullPath($env:FLEQI_WINDOWS_FIXTURE_DIR)
if (-not $root.Contains('\tests\.artifacts\desktop\windows-data-')) { throw 'Not an isolated desktop fixture' }
$target = [IO.Path]::GetFullPath((Join-Path $root $Folder))
if (-not $target.StartsWith($root + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Fixture path escaped' }
$shell = New-Object -ComObject Shell.Application
if ($Action -eq 'close') {
  foreach ($window in @($shell.Windows())) {
    try { if ($window.Document.Folder.Self.Path.StartsWith($root + '\', [StringComparison]::OrdinalIgnoreCase)) { $window.Quit() } } catch { }
  }
  Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class FleqiFixtureCredential { [DllImport("advapi32.dll", CharSet=CharSet.Unicode, SetLastError=true)] public static extern bool CredDeleteW(string target, uint type, uint flags); }'
  $fixtureId = 'win-' + (Split-Path (Split-Path $root -Parent) -Leaf)
  $null = [FleqiFixtureCredential]::CredDeleteW(('fleqi:app.fleqi.desktop.test:provider.' + $fixtureId), 1, 0)
  exit 0
}
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class FleqiFixtureWindow {
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hwnd);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr hwnd, System.Text.StringBuilder name, int count);
 [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr hwnd, int command);
 [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr hwnd, int x, int y, int width, int height, bool repaint);
 [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);
 [DllImport("user32.dll")] static extern bool AttachThreadInput(uint from, uint to, bool attach);
 [DllImport("user32.dll")] static extern bool IsGUIThread(bool convert);
 [StructLayout(LayoutKind.Sequential)] struct POINT { public int X; public int Y; }
 [StructLayout(LayoutKind.Sequential)] struct RECT { public int Left, Top, Right, Bottom; }
 [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr hwnd, out RECT rect);
 [DllImport("user32.dll")] static extern bool SetWindowPos(IntPtr hwnd, IntPtr after, int x, int y, int width, int height, uint flags);
 [DllImport("user32.dll")] static extern bool GetCursorPos(out POINT point);
 [DllImport("user32.dll")] static extern bool SetCursorPos(int x, int y);
 [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(POINT point);
 [DllImport("user32.dll")] static extern IntPtr GetAncestor(IntPtr hwnd, uint flags);
 [DllImport("user32.dll")] static extern void mouse_event(uint flags, uint x, uint y, uint data, UIntPtr extra);
 [DllImport("user32.dll")] static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
 [DllImport("user32.dll")] static extern uint GetDpiForWindow(IntPtr hwnd);
 public static bool FocusFixture(IntPtr hwnd) {
   IsGUIThread(true);
   uint pid; uint foreground = GetWindowThreadProcessId(GetForegroundWindow(), out pid);
   uint own = GetCurrentThreadId(); bool attached = own != foreground && AttachThreadInput(own, foreground, true);
   try { ShowWindow(hwnd, 9); SetForegroundWindow(hwnd); if (GetForegroundWindow() == hwnd) return true; }
   finally { if (attached) AttachThreadInput(own, foreground, false); }
   IntPtr dpi = SetThreadDpiAwarenessContext(new IntPtr(-4)); POINT previous; GetCursorPos(out previous);
   try {
     SetWindowPos(hwnd, IntPtr.Zero, 0, 0, 0, 0, 1 | 2 | 16 | 64);
     RECT rect; if (!GetWindowRect(hwnd, out rect)) return false;
     POINT point = new POINT { X = (rect.Left + rect.Right) / 2, Y = rect.Top + (int)(24 * GetDpiForWindow(hwnd) / 96) };
     if (GetAncestor(WindowFromPoint(point), 2) != hwnd) return false;
     SetCursorPos(point.X, point.Y); mouse_event(2, 0, 0, 0, UIntPtr.Zero); mouse_event(4, 0, 0, 0, UIntPtr.Zero);
     System.Threading.Thread.Sleep(100); return GetForegroundWindow() == hwnd;
   } finally { SetCursorPos(previous.X, previous.Y); SetThreadDpiAwarenessContext(dpi); }
 }
}
'@
if ($Action -eq 'open') { $shell.Explore($target) }
if ($Action -eq 'tab') {
  $owners = @($shell.Windows() | Where-Object { $_.HWND -eq $WindowId })
  if ($owners.Count -ne 1 -or -not $owners[0].Document.Folder.Self.Path.StartsWith($root + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Tab fixture must start with one owned tab' }
  $oldPath = $owners[0].Document.Folder.Self.Path
  Add-Type -AssemblyName UIAutomationClient
  Add-Type -AssemblyName UIAutomationTypes
  $element = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]$WindowId)
  $add = $element.FindFirst([System.Windows.Automation.TreeScope]::Descendants, (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::AutomationIdProperty, 'AddButton')))
  $add.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
  for ($i = 0; $i -lt 30; $i++) {
    $newTabs = @($shell.Windows() | Where-Object { $_.HWND -eq $WindowId -and $_.Document.Folder.Self.Path -ne $oldPath })
    if ($newTabs.Count -eq 1) { break }
    Start-Sleep -Milliseconds 100
  }
  if ($newTabs.Count -ne 1) { throw 'New tab is not unique' }
  $newTabs[0].Navigate2($target)
}
$found = @()
for ($i = 0; $i -lt 50; $i++) {
  $found = @($shell.Windows() | Where-Object { try { $_.Document.Folder.Self.Path -eq $target } catch { $false } })
  if ($found.Count -eq 1) { break }
  Start-Sleep -Milliseconds 100
}
if ($found.Count -ne 1) { throw 'Fixture Explorer window is not unique' }
$window = $found[0]
if ($WindowId -ne 0 -and $window.HWND -ne $WindowId) { throw 'Tab navigation changed the owning window' }
if (@($shell.Windows() | Where-Object { $_.HWND -eq $window.HWND }).Count -gt 1) {
  Add-Type -AssemblyName UIAutomationClient
  Add-Type -AssemblyName UIAutomationTypes
  $element = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]$window.HWND)
  $tabs = @($element.FindAll([System.Windows.Automation.TreeScope]::Descendants, (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::TabItem))) | Where-Object { $_.Current.Name -eq (Split-Path $target -Leaf) })
  if ($tabs.Count -ne 1) { throw 'Owned tab cannot be uniquely selected' }
  $tabs[0].GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
}
if ($Action -eq 'move') { $null = [FleqiFixtureWindow]::MoveWindow([IntPtr]$window.HWND, 180, 120, 900, 600, $true) }
$item = $window.Document.Folder.ParseName($File)
if ($null -eq $item) { throw 'Fixture selected file is missing' }
$window.Document.SelectItem($item, 29)
$null = [FleqiFixtureWindow]::ShowWindow([IntPtr]$window.HWND, 9)
$focused = [FleqiFixtureWindow]::FocusFixture([IntPtr]$window.HWND)
$class = New-Object System.Text.StringBuilder 128
$foreground = [FleqiFixtureWindow]::GetForegroundWindow()
$null = [FleqiFixtureWindow]::GetClassName($foreground, $class, 128)
@{ hwnd = $window.HWND; focused = $focused; foreground = $foreground.ToInt64(); foregroundClass = $class.ToString(); directory = $target; count = $window.Document.SelectedItems().Count } | ConvertTo-Json -Compress
