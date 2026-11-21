# RF Ops stage station for Windows: full-screen board (Edge kiosk) that stays on and reopens.
#   rfops-station.ps1             open the board (asks for the address the first time)
#   rfops-station.ps1 -Setup      change the address and "start at login"
param([switch]$Setup)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName Microsoft.VisualBasic
$dir = Join-Path $env:APPDATA 'RF Ops'
$conf = Join-Path $dir 'station-url.txt'
New-Item -ItemType Directory -Force -Path $dir | Out-Null

$url = if (Test-Path $conf) { (Get-Content $conf -Raw).Trim() } else { '' }
if ($Setup -or -not $url) {
    $url = [Microsoft.VisualBasic.Interaction]::InputBox('RF Ops address (e.g. https://rfops.example.com):', 'RF Ops station', $url).Trim().TrimEnd('/')
    if (-not $url) { exit 1 }
    Set-Content -Path $conf -Value $url
    $startup = Join-Path ([Environment]::GetFolderPath('Startup')) 'RF Ops Station.lnk'
    $answer = [Microsoft.VisualBasic.Interaction]::MsgBox('Open the RF Ops station automatically when you sign in to Windows?', 'YesNo,Question', 'RF Ops station')
    if ($answer -eq 'Yes') {
        $sh = (New-Object -ComObject WScript.Shell).CreateShortcut($startup)
        $sh.TargetPath = (Get-Command powershell.exe).Source
        $sh.Arguments = "-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File `"$PSCommandPath`""
        $sh.Save()
    } elseif (Test-Path $startup) { Remove-Item $startup }
    if ($Setup) { exit 0 }
}

# Keep the display on while the station runs (no admin rights needed).
Add-Type -Namespace RfOps -Name Power -MemberDefinition '[DllImport("kernel32.dll")] public static extern uint SetThreadExecutionState(uint f);'
[RfOps.Power]::SetThreadExecutionState(0x80000003) | Out-Null  # CONTINUOUS | SYSTEM | DISPLAY

$browsers = @(
    "${env:ProgramFiles(x86)}\Microsoft\Edge\Application\msedge.exe",
    "$env:ProgramFiles\Microsoft\Edge\Application\msedge.exe",
    "$env:ProgramFiles\Google\Chrome\Application\chrome.exe",
    "${env:ProgramFiles(x86)}\Google\Chrome\Application\chrome.exe"
)
$browser = $browsers | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
if (-not $browser) { [Microsoft.VisualBasic.Interaction]::MsgBox('Microsoft Edge or Google Chrome is needed.', 'Critical', 'RF Ops station') | Out-Null; exit 1 }

$profileDir = Join-Path $dir 'browser'
while ($true) {
    $args = @("--kiosk", "$url/#/board", "--edge-kiosk-type=fullscreen", "--user-data-dir=`"$profileDir`"",
              "--no-first-run", "--noerrdialogs", "--disable-session-crashed-bubble",
              "--disable-background-timer-throttling", "--disable-renderer-backgrounding")
    Start-Process -FilePath $browser -ArgumentList $args -Wait
    Start-Sleep -Seconds 2
}
