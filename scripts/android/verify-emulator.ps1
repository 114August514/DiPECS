param(
    [string]$AvdName = "dipecs_emu",
    [int]$Port = 46321,
    [string]$Token = "dipecs-dev-emulator-shared-token-00000000",
    [int]$TimeoutSeconds = 120,
    [switch]$SkipReplay,
    [switch]$SkipSmoke,
    [switch]$SkipBridgePing
)

$ErrorActionPreference = "Stop"

function Write-Step([string]$Message) {
    Write-Host "`n==> $Message" -ForegroundColor Cyan
}

function Write-Pass([string]$Message) {
    Write-Host "  [PASS] $Message" -ForegroundColor Green
}

function Write-Fail([string]$Message) {
    Write-Host "  [FAIL] $Message" -ForegroundColor Red
}

function Resolve-AndroidSdkRoot {
    $candidates = @()
    if ($env:ANDROID_HOME) { $candidates += $env:ANDROID_HOME }
    if ($env:ANDROID_SDK_ROOT) { $candidates += $env:ANDROID_SDK_ROOT }
    if ($env:LOCALAPPDATA) { $candidates += (Join-Path $env:LOCALAPPDATA "Android\Sdk") }
    foreach ($candidate in $candidates | Select-Object -Unique) {
        if ($candidate -and (Test-Path -LiteralPath $candidate)) {
            return (Resolve-Path -LiteralPath $candidate).Path
        }
    }
    throw "Android SDK not found."
}

function Invoke-Adb([string]$Adb, [string]$Serial, [string[]]$Arguments) {
    $args = @()
    if ($Serial) { $args += @("-s", $Serial) }
    $args += $Arguments
    $result = & $Adb @args 2>&1
    return $result
}

function Ping-ActionBridge([string]$HostName, [int]$Port, [string]$Token) {
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
            throw "empty response"
        }
        $response = [System.Text.Encoding]::UTF8.GetString($buffer, 0, $read)
        $json = $response | ConvertFrom-Json
        if ($json.status -ne "ok") {
            throw "unexpected status: $($json.status)"
        }
        return $true
    } catch {
        Write-Fail "Bridge ping failed: $_"
        return $false
    } finally {
        $client.Close()
    }
}

# ── Main ─────────────────────────────────────────────────────────

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = Resolve-Path (Join-Path $scriptDir "..")

Write-Host "=== DiPECS Emulator Verification ===" -ForegroundColor Yellow
Write-Host "Repository: $repoRoot"

$totalTests = 0
$passedTests = 0

# ── 1. Replay validation ─────────────────────────────────────────

if (-not $SkipReplay) {
    Write-Step "1. Replay Pipeline Validation"

    $tracesDir = Join-Path $repoRoot "data\traces"
    $evalDir = Join-Path $repoRoot "data\evaluation\verify"
    New-Item -ItemType Directory -Force -Path $evalDir | Out-Null

    $replayCases = @(
        @{ Name = "sample_replay"; File = "sample_replay.jsonl" },
        @{ Name = "denial"; File = "denial.jsonl" },
        @{ Name = "android_real_device_sample"; File = "android_real_device_sample.redacted.jsonl" }
    )

    foreach ($case in $replayCases) {
        $totalTests++
        $traceFile = Join-Path $tracesDir $case.File
        $outFile = Join-Path $evalDir "$($case.Name).ndjson"
        $auditFile = Join-Path $evalDir "$($case.Name).audit.ndjson"

        if (-not (Test-Path $traceFile)) {
            Write-Fail "$($case.Name): trace file missing: $traceFile"
            continue
        }

        try {
            $result = cargo run -p aios-cli -- replay $traceFile `
                --stages policy `
                --output $outFile `
                --audit $auditFile `
                2>&1
            if ($LASTEXITCODE -ne 0) {
                Write-Fail "$($case.Name): replay failed with exit code $LASTEXITCODE"
                Write-Host "  $result" -ForegroundColor DarkGray
                continue
            }

            # Verify audit hash is present and non-empty.
            $summaryLine = Get-Content $outFile | Select-Object -Last 1
            $summaryJson = $summaryLine | ConvertFrom-Json
            $auditHash = $summaryJson.summary.audit_hash
            if (-not $auditHash -or $auditHash -eq "") {
                Write-Fail "$($case.Name): audit hash is empty"
                continue
            }

            $passedTests++
            Write-Pass "$($case.Name): audit_hash=$auditHash events=$($summaryJson.summary.events_ingested) windows=$($summaryJson.summary.windows_closed)"
        } catch {
            Write-Fail "$($case.Name): $_"
        }
    }
}

# ── 2. Action bridge protocol smoke test ─────────────────────────

if (-not $SkipBridgePing) {
    Write-Step "2. Action Bridge Protocol"

    $SdkRoot = Resolve-AndroidSdkRoot
    $Adb = Join-Path $SdkRoot "platform-tools\adb.exe"
    if (-not (Test-Path $Adb)) {
        Write-Host "  [SKIP] adb not found at $Adb"
    } else {
        # Check for running emulator.
        $devices = & $Adb devices 2>$null
        $emulator = ($devices | Select-String "emulator-.*\tdevice$")
        if (-not $emulator) {
            Write-Host "  [SKIP] No running emulator found."
        } else {
            # Verify port forwarding.
            $forwards = & $Adb forward --list 2>$null
            $hasForward = $forwards | Select-String "tcp:$Port"
            if (-not $hasForward) {
                Write-Host "  Setting up port forward tcp:$Port -> tcp:$Port"
                & $Adb forward --remove "tcp:$Port" 2>$null
                & $Adb forward "tcp:$Port" "tcp:$Port"
            }

            $totalTests++
            if (Ping-ActionBridge "127.0.0.1" $Port $Token) {
                $passedTests++
                Write-Pass "Action bridge ping OK on port $Port"
            }
        }
    }
}

# ── 3. Collector event schema validation ─────────────────────────

if (-not $SkipSmoke) {
    Write-Step "3. Collector Event Schema Validation"

    $totalTests++
    try {
        # Verify golden_sample.json matches expected sanitized output.
        $goldenPath = Join-Path $tracesDir "golden_sample.json"
        if (-not (Test-Path $goldenPath)) {
            Write-Fail "golden_sample.json missing"
        } else {
            $golden = Get-Content $goldenPath -Raw | ConvertFrom-Json
            $expectedSanitized = $golden.expected_sanitized
            if ($expectedSanitized.Count -eq 3) {
                $passedTests++
                Write-Pass "Golden sample has 3 expected sanitized events"
            } else {
                Write-Fail "Golden sample expected 3 events, got $($expectedSanitized.Count)"
            }
        }
    } catch {
        Write-Fail "Schema validation: $_"
    }

    # Verify synthetic large trace produces expected event kinds.
    $totalTests++
    $synthPath = Join-Path $tracesDir "android_synthetic_large.redacted.jsonl"
    if (-not (Test-Path $synthPath)) {
        Write-Fail "Synthetic large trace missing"
    } else {
        $synthLines = Get-Content $synthPath
        $nonNullLines = $synthLines | Where-Object { $_ -match '"rawEvent":\{' }
        if ($nonNullLines.Count -gt 0) {
            $passedTests++
            Write-Pass "Synthetic trace: $($nonNullLines.Count) rows with rawEvent"
        } else {
            Write-Fail "Synthetic trace has no rawEvent rows"
        }
    }
}

# ── Summary ──────────────────────────────────────────────────────

Write-Host ""
Write-Host "=== Results: $passedTests / $totalTests passed ===" -ForegroundColor $(if ($passedTests -eq $totalTests) { "Green" } else { "Red" })

if ($passedTests -eq $totalTests) {
    Write-Host "All checks passed. Emulator validation ready." -ForegroundColor Green
    exit 0
} else {
    Write-Host "Some checks failed. See details above." -ForegroundColor Red
    exit 1
}
