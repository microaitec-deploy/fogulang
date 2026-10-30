# Installs Fogu on Windows: downloads fogu.exe (or copies a local one), puts it in
# a folder and adds that folder to the PATH.
#
#   powershell -c "iex (irm @FOGU_SERVER@/install.ps1)"          # from cmd or PowerShell
#   $env:FOGU_VERSION = "0.7.1"; iex (irm @FOGU_SERVER@/install.ps1)   # a specific version
#   .\scripts\install.ps1                      # %LOCALAPPDATA%\Fogu\bin, the user's PATH
#   .\scripts\install.ps1 -System              # %ProgramFiles%\Fogu, the system PATH (run as administrator)
#   .\scripts\install.ps1 -From target\release\fogu.exe   # copy a local build instead of downloading
#   .\scripts\install.ps1 -Dir D:\tools\fogu   # another folder
#   .\scripts\install.ps1 -Uninstall
#
# Use `iex (irm URL)`, not `irm URL | iex`: inside `powershell -c "..."` a pipe
# hands the script to iex line by line, which breaks multi-line statements.
#
# The executable comes from the Fogu download server (server/ in this project):
# $FOGU_SERVER/download/<version>/fogu.exe. The server writes its own address
# in place of @FOGU_SERVER@ when it sends this script.
#
# Environment: FOGU_SERVER (the download server), FOGU_VERSION (default: latest),
# FOGU_BASE_URL (the folder fogu.exe is downloaded from), FOGU_DOWNLOAD_URL
# (a full URL of fogu.exe), FOGU_INSTALL (= -Dir).
param(
    [string]$Dir = "",
    [string]$From = "",
    [switch]$System,
    [switch]$Uninstall
)
$ErrorActionPreference = "Stop"

$server = if ($env:FOGU_SERVER) { $env:FOGU_SERVER } else { "@FOGU_SERVER@" }
# run from a checkout (not sent by a server): the official server
if ($server.StartsWith("@")) { $server = "https://fogulang.microaitec.com" }
$version = if ($env:FOGU_VERSION) { $env:FOGU_VERSION } else { "latest" }
$base = if ($env:FOGU_BASE_URL) { $env:FOGU_BASE_URL } else { "$server/download/$version" }
if (-not $Dir) {
    if ($env:FOGU_INSTALL) { $Dir = $env:FOGU_INSTALL }
    elseif ($System) { $Dir = Join-Path $env:ProgramFiles "Fogu" }
    else { $Dir = Join-Path $env:LOCALAPPDATA "Fogu\bin" }
}
$scope = if ($System) { "Machine" } else { "User" }

if ($System) {
    $admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    if (-not $admin) { throw "-System needs an administrator PowerShell (or install for your user without -System)" }
}

function Get-PathList([string]$scope) {
    $p = [Environment]::GetEnvironmentVariable("Path", $scope)
    if (-not $p) { return @() }
    return $p.Split(";") | Where-Object { $_ -ne "" }
}

# new terminals see the change without signing out
function Send-SettingChange {
    $sig = '[DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Auto)] public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint Msg, UIntPtr wParam, string lParam, uint fuFlags, uint uTimeout, out UIntPtr lpdwResult);'
    try {
        $t = Add-Type -MemberDefinition $sig -Name "FoguWin" -Namespace "FoguInstall" -PassThru -ErrorAction SilentlyContinue
        $r = [UIntPtr]::Zero
        [void]$t::SendMessageTimeout([IntPtr]0xffff, 0x1A, [UIntPtr]::Zero, "Environment", 2, 5000, [ref]$r)
    } catch { }
}

if ($Uninstall) {
    $exe = Join-Path $Dir "fogu.exe"
    if (Test-Path $exe) { Remove-Item $exe -Force; Write-Host "removed $exe" }
    $paths = Get-PathList $scope | Where-Object { $_.TrimEnd("\") -ne $Dir.TrimEnd("\") }
    [Environment]::SetEnvironmentVariable("Path", ($paths -join ";"), $scope)
    Send-SettingChange
    Write-Host "removed $Dir from the $scope PATH"
    return
}

New-Item -ItemType Directory -Force -Path $Dir | Out-Null
$exe = Join-Path $Dir "fogu.exe"
$tmp = Join-Path ([IO.Path]::GetTempPath()) ("fogu-" + [guid]::NewGuid().ToString("N") + ".exe")
try {
    if ($From) {
        if (-not (Test-Path $From)) { throw "no such file: $From" }
        Copy-Item $From $tmp -Force
        Write-Host "copying $From"
    } else {
        # 32-bit Windows gets the x86 build (no JIT: the interpreter only)
        $file = if ([Environment]::Is64BitOperatingSystem) { "fogu.exe" } else { "fogu-windows-x86.exe" }
        $url = if ($env:FOGU_DOWNLOAD_URL) { $env:FOGU_DOWNLOAD_URL } else { "$base/$file" }
        Write-Host "downloading $url"
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
        $ProgressPreference = "SilentlyContinue"
        Invoke-WebRequest -Uri $url -OutFile $tmp -UseBasicParsing
    }
    $version = & $tmp --version 2>$null
    if ($LASTEXITCODE -ne 0) { throw "the downloaded file does not run on this machine" }
    Move-Item $tmp $exe -Force
} finally {
    if (Test-Path $tmp) { Remove-Item $tmp -Force }
}
Write-Host "installed $exe ($version)"

# remember the server: `fogu build --compile --target <platform>` downloads the
# runtimes of other platforms from it
if ($server -match '^https?://') {
    $foguHome = if ($env:FOGU_HOME) { $env:FOGU_HOME } else { Join-Path $env:USERPROFILE ".fogu" }
    New-Item -ItemType Directory -Force -Path $foguHome | Out-Null
    Set-Content -Path (Join-Path $foguHome "server") -Value $server -Encoding ascii
}

$paths = Get-PathList $scope
if ($paths | Where-Object { $_.TrimEnd("\") -eq $Dir.TrimEnd("\") }) {
    Write-Host "$Dir is already on the $scope PATH"
} else {
    [Environment]::SetEnvironmentVariable("Path", (($paths + $Dir) -join ";"), $scope)
    Send-SettingChange
    Write-Host "added $Dir to the $scope PATH"
}
if (-not (($env:Path -split ";") -contains $Dir)) { $env:Path = "$env:Path;$Dir" }
Write-Host "try: fogu --version   (other terminals: open a new one)"
