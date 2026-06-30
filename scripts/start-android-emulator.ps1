param(
    [string]$AvdName = "dipecs_emu",
    [string]$SystemImage = "system-images;android-35;google_apis;x86_64",
    [string]$Device = "pixel_6",
    [int]$Port = 46321,
    [string]$Token = "dipecs-dev-emulator-shared-token-00000000",
    [int]$BootTimeoutSeconds = 180,
    [switch]$Headless,
    [switch]$RecreateAvd,
    [switch]$SkipImageInstall,
    [switch]$SkipApkInstall,
    [switch]$SkipHealthCheck
)

$ErrorActionPreference = "Stop"

function Write-Step([string]$Message) {
    Write-Host "`n==> $Message" -ForegroundColor Cyan
}

function Fail([string]$Message) {
    Write-Error $Message
    exit 1
}

function Resolve-AndroidSdkRoot {
    $candidates = @()
    if ($env:ANDROID_HOME) { $candidates += $env:ANDROID_HOME }
    if ($env:ANDROID_SDK_ROOT) { $candidates += $env:ANDROID_SDK_ROOT }
    if ($env:LOCALAPPDATA) { $candidates += (Join-Path $env:LOCALAPPDATA "Android\Sdk") }
    if ($env:USERPROFILE) { $candidates += (Join-Path $env:USERPROFILE "AppData\Local\Android\Sdk") }

    foreach ($candidate in $candidates | Select-Object -Unique) {
        if ($candidate -and (Test-Path -LiteralPath $candidate)) {
            return (Resolve-Path -LiteralPath $candidate).Path
        }
    }
    Fail "Android SDK not found. Set ANDROID_HOME or ANDROID_SDK_ROOT, or install it under %LOCALAPPDATA%\Android\Sdk."
}

function Require-File([string]$Path, [string]$Name) {
    if (-not (Test-Path -LiteralPath $Path)) {
        Fail "$Name not found: $Path"
    }
    return $Path
}

function Invoke-Tool([string]$FilePath, [string[]]$Arguments, [switch]$AllowFailure) {
    $display = "$FilePath $($Arguments -join ' ')"
    Write-Host "  $display" -ForegroundColor DarkGray
    & $FilePath @Arguments
    $code = $LASTEXITCODE
    if (-not $AllowFailure -and $code -ne 0) {
        Fail "Command failed with exit code ${code}: $display"
    }
    return $code
}

function Get-AdbDevices([string]$Adb) {
    $lines = & $Adb devices
    return $lines | Where-Object { $_ -match "\t(device|offline|unauthorized)$" }
}

function Invoke-Adb([string]$Adb, [string]$Serial, [string[]]$Arguments, [switch]$AllowFailure) {
    $args = @()
    if ($Serial) { $args += @("-s", $Serial) }
    $args += $Arguments
    return Invoke-Tool -FilePath $Adb -Arguments $args -AllowFailure:$AllowFailure
}
function Invoke-ActionSocketPing([string]$HostName, [int]$Port, [string]$Token) {
    $payload = @{ message_type = "ping"; auth_token = $Token } | ConvertTo-Json -Compress
    $client = [System.Net.Sockets.TcpClient]::new()
    $client.ReceiveTimeout = 5000
    $client.SendTimeout = 5000
    try {
        $client.Connect($HostName, $Port)
        $stream = $client.GetStream()
        $bytes = [System.Text.Encoding]::UTF8.GetBytes($payload)
        $stream.Write($bytes, 0, $bytes.Length)
        $stream.Flush()
        $client.Client.Shutdown([System.Net.Sockets.SocketShutdown]::Send)

        $buffer = New-Object byte[] 4096
        $read = $stream.Read($buffer, 0, $buffer.Length)
        if ($read -le 0) {
            Fail "Android action bridge returned an empty response."
        }
        $response = [System.Text.Encoding]::UTF8.GetString($buffer, 0, $read)
        $json = $response | ConvertFrom-Json
        if ($json.status -ne "ok") {
            Fail "Android action bridge returned unexpected response: $response"
        }
        Write-Host "  bridge response: $response"
    } finally {
        $client.Close()
    }
}

function Get-FirstEmulatorSerial([string]$Adb) {
    $devices = Get-AdbDevices $Adb
    $emulators = @($devices | Where-Object { $_ -match "^emulator-\d+\tdevice$" })
    if ($emulators.Count -gt 0) {
        return (($emulators[0] -split "\s+")[0])
    }
    return $null
}

Write-Step "0. Detect Android SDK and tools"
$SdkRoot = Resolve-AndroidSdkRoot
$Emulator = Require-File (Join-Path $SdkRoot "emulator\emulator.exe") "emulator.exe"
$Adb = Require-File (Join-Path $SdkRoot "platform-tools\adb.exe") "adb.exe"
$AvdManager = Require-File (Join-Path $SdkRoot "cmdline-tools\latest\bin\avdmanager.bat") "avdmanager.bat"
$SdkManager = Join-Path $SdkRoot "cmdline-tools\latest\bin\sdkmanager.bat"
if (-not (Test-Path -LiteralPath $SdkManager)) {
    $SdkManager = Join-Path $SdkRoot "tools\bin\sdkmanager.bat"
}
$SdkManager = Require-File $SdkManager "sdkmanager.bat"
$Gradle = Require-File (Join-Path $PSScriptRoot "..\apps\android-collector\gradlew.bat") "gradlew.bat"

Write-Host "  ANDROID_SDK_ROOT=$SdkRoot"
Write-Host "  Current adb devices:"
& $Adb devices | ForEach-Object { Write-Host "    $_" }

Write-Step "1. Ensure system image and AVD"
if (-not $SkipImageInstall) {
    Invoke-Tool $SdkManager @("--install", $SystemImage)
}

$avdList = & $AvdManager list avd
$avdExists = ($avdList | Select-String -SimpleMatch "Name: $AvdName") -ne $null
if ($avdExists -and $RecreateAvd) {
    Invoke-Tool $AvdManager @("delete", "avd", "-n", $AvdName) -AllowFailure
    $avdExists = $false
}
if (-not $avdExists) {
    Invoke-Tool $AvdManager @("create", "avd", "-n", $AvdName, "-k", $SystemImage, "-d", $Device, "-f")
} else {
    Write-Host "  AVD '$AvdName' already exists; skipping create."
}

Write-Step "2. Start emulator"
$existingSerial = Get-FirstEmulatorSerial $Adb
if ($existingSerial) {
    Write-Host "  Reusing already-running emulator: $existingSerial"
} else {
    $emuArgs = @("-avd", $AvdName, "-no-audio", "-memory", "4096", "-cores", "4", "-netdelay", "none", "-netspeed", "full")
    if ($Headless) {
        $emuArgs += @("-no-window", "-gpu", "swiftshader_indirect")
    } else {
        $emuArgs += @("-gpu", "host")
    }
    if ($Headless) {
        Start-Process -FilePath $Emulator -ArgumentList $emuArgs -WindowStyle Hidden | Out-Null
    } else {
        Start-Process -FilePath $Emulator -ArgumentList $emuArgs | Out-Null
    }
}

Write-Step "3. Wait for boot completion"
Invoke-Tool $Adb @("wait-for-device")
$serial = $null
$elapsed = 0
while (-not $serial -and $elapsed -lt 30) {
    Start-Sleep -Seconds 1
    $elapsed += 1
    $serial = Get-FirstEmulatorSerial $Adb
}
if (-not $serial) {
    Fail "No running emulator device appeared in adb devices."
}
Write-Host "  Target emulator: $serial"

$elapsed = 0
$status = ""
do {
    Start-Sleep -Seconds 3
    $elapsed += 3
    $status = (& $Adb -s $serial shell getprop sys.boot_completed 2>$null).Trim()
    Write-Host "  waiting boot... (${elapsed}s) sys.boot_completed=$status"
} while ($status -ne "1" -and $elapsed -lt $BootTimeoutSeconds)
if ($status -ne "1") {
    Fail "Emulator boot timed out after ${BootTimeoutSeconds}s."
}
Start-Sleep -Seconds 3
Invoke-Adb $Adb $serial @("shell", "pm", "list", "packages") | Out-Null

Write-Step "4. Install debug APK"
if (-not $SkipApkInstall) {
    Push-Location (Join-Path $PSScriptRoot "..\apps\android-collector")
    try {
        & $Gradle ":app:installDebug" "--stacktrace"
        if ($LASTEXITCODE -ne 0) { Fail "Gradle installDebug failed with exit code $LASTEXITCODE." }
    } finally {
        Pop-Location
    }
    Invoke-Adb $Adb $serial @("shell", "pm", "list", "packages", "com.dipecs.collector") | Out-Null
} else {
    Write-Host "  Skipping APK install."
}

Write-Step "5. Configure adb port forwarding"
Invoke-Adb $Adb $serial @("forward", "--remove", "tcp:$Port") -AllowFailure | Out-Null
Invoke-Adb $Adb $serial @("forward", "tcp:$Port", "tcp:$Port") | Out-Null
& $Adb -s $serial forward --list | Select-String "$Port" | ForEach-Object { Write-Host "  $_" }

Write-Step "6. Debug token"
Write-Host "  Debug token: $Token"
Write-Host "  .env should contain: DIPECS_ANDROID_ACTION_BRIDGE_TOKEN=$Token"
Write-Host "  Optional override before first app launch: adb shell setprop debug.dipecs.token <token>"
Write-Host "  If a token is already stored, clear app data: adb shell pm clear com.dipecs.collector"

Write-Step "7. Start app and health-check socket"
Invoke-Adb $Adb $serial @("shell", "am", "start", "-n", "com.dipecs.collector/.MainActivity") | Out-Null
Invoke-Adb $Adb $serial @("shell", "pm", "grant", "com.dipecs.collector", "android.permission.POST_NOTIFICATIONS") -AllowFailure | Out-Null
Invoke-Adb $Adb $serial @("shell", "am", "start", "-n", "com.dipecs.collector/.debug.DebugCollectorControlActivity") | Out-Null
Start-Sleep -Seconds 3

if (-not $SkipHealthCheck) {
    Invoke-ActionSocketPing "127.0.0.1" $Port $Token
} else {
    Write-Host "  Skipping action socket health check."
}

Write-Step "Done"
Write-Host "  Emulator: $serial"
Write-Host "  adb forward: tcp:$Port -> tcp:$Port"
Write-Host "  token: $Token"
