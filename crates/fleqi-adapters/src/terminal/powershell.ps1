# Fleqi integration runs only in this terminal; the user's profile is not changed.
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false
$OutputEncoding = [Console]::OutputEncoding
Import-Module PSReadLine -ErrorAction Stop
$global:__FleqiPipe = New-Object System.IO.Pipes.NamedPipeClientStream('.', $env:FLEQI_PIPE_NAME, [System.IO.Pipes.PipeDirection]::InOut, [System.IO.Pipes.PipeOptions]::Asynchronous)
$global:__FleqiPipe.Connect(5000)
$env:FLEQI_PIPE_NAME = $null
$global:__FleqiReader = New-Object System.IO.StreamReader($global:__FleqiPipe, [System.Text.Encoding]::UTF8)
$global:__FleqiWriter = New-Object System.IO.StreamWriter($global:__FleqiPipe, (New-Object System.Text.UTF8Encoding $false))
$global:__FleqiWriter.AutoFlush = $true
$global:__FleqiRead = $global:__FleqiReader.ReadLineAsync()
$global:__FleqiReading = $false
$global:__FleqiPending = $null

function global:__FleqiSend($value) {
    if ($global:__FleqiPipe.IsConnected) {
        $global:__FleqiWriter.WriteLine(($value | ConvertTo-Json -Compress))
    }
}

$global:__FleqiOriginalReadLine = $function:PSConsoleHostReadLine
function global:PSConsoleHostReadLine {
    $global:__FleqiReading = $true
    try { $global:__FleqiOriginalReadLine.Invoke() }
    finally {
        $global:__FleqiReading = $false
        __FleqiSend @{ event = 'preexec' }
    }
}

# PSReadLine invokes OnIdle on its runspace. Recheck the actual editing buffer
# here: no host-generated keystrokes or cd text ever enter an interactive child.
$null = Register-EngineEvent -SourceIdentifier PowerShell.OnIdle -Action {
    if (-not $global:__FleqiReading -or -not $global:__FleqiPipe.IsConnected) { return }
    try {
        $line = $null
        $cursor = 0
        [Microsoft.PowerShell.PSConsoleReadLine]::GetBufferState([ref]$line, [ref]$cursor)
        if ($global:__FleqiRead.IsCompleted) {
            $json = $global:__FleqiRead.GetAwaiter().GetResult()
            if ($null -eq $json) { return }
            $global:__FleqiPending = $json | ConvertFrom-Json
            $global:__FleqiRead = $global:__FleqiReader.ReadLineAsync()
        }
        if ($line.Length -eq 0 -and $null -ne $global:__FleqiPending) {
            $request = $global:__FleqiPending
            $global:__FleqiPending = $null
            $ok = $false
            $message = $null
            try { Set-Location -LiteralPath $request.path -ErrorAction Stop; $ok = $true }
            catch { $message = $_.Exception.Message }
            __FleqiSend @{ event = 'cd'; revision = [string]$request.revision; ok = $ok; cwd = $PWD.ProviderPath; message = $message }
            [Microsoft.PowerShell.PSConsoleReadLine]::InvokePrompt()
        }
        __FleqiSend @{ event = 'state'; cwd = $PWD.ProviderPath; edit = $line.Length }
    } catch {
        $global:__FleqiReading = $false
        try { __FleqiSend @{ event = 'preexec' } } catch { }
    }
}
