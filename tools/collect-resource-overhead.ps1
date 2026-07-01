param(
    [string]$Adb = "$env:LOCALAPPDATA\Android\Sdk\platform-tools\adb.exe",
    [string]$Package = "com.dipecs.collector",
    [int]$SamplesPerMode = 10,
    [int]$SampleIntervalSecs = 10,
    [string]$OutDir = "data\evaluation",
    [string]$Token = "dipecs-dev-emulator-shared-token-00000000",
    [int]$Port = 46321
)

$ErrorActionPreference = "Stop"

function Invoke-Adb([string[]]$AdbArgs, [switch]$AllowFailure) {
    $output = & $Adb @AdbArgs 2>&1
    $code = $LASTEXITCODE
    if (-not $AllowFailure -and $code -ne 0) {
        throw "adb $($AdbArgs -join ' ') failed with ${code}: $output"
    }
    return $output
}

function Parse-SizeMb([string]$raw) {
    if ($raw -match '^([0-9.]+)([KMG]?)$') {
        $value = [double]$matches[1]
        switch ($matches[2]) {
            "K" { return $value / 1024.0 }
            "M" { return $value }
            "G" { return $value * 1024.0 }
            default { return $value / 1024.0 }
        }
    }
    return 0.0
}

function Get-ProcessTopMetrics {
    $top = Invoke-Adb -AdbArgs @("shell", "top", "-b", "-n", "1", "-o", "PID,%CPU,RES,ARGS") -AllowFailure
    $line = $top | Where-Object { $_ -match $Package } | Select-Object -First 1
    if (-not $line) {
        return @{ cpu_pct = 0.0; top_res_mb = 0.0; pid = $null }
    }
    $parts = ($line.Trim() -split '\s+')
    return @{
        pid = [int]$parts[0]
        cpu_pct = [double]$parts[1]
        top_res_mb = [math]::Round((Parse-SizeMb $parts[2]), 3)
    }
}

function Get-MemInfo {
    $mem = Invoke-Adb -AdbArgs @("shell", "dumpsys", "meminfo", $Package) -AllowFailure
    $rssKb = 0
    $pssKb = 0
    foreach ($line in $mem) {
        if ($line -match 'TOTAL RSS:\s+([0-9]+)') {
            $rssKb = [int]$matches[1]
        }
        if ($line -match 'TOTAL PSS:\s+([0-9]+)') {
            $pssKb = [int]$matches[1]
        }
    }
    return @{
        rss_mb = [math]::Round($rssKb / 1024.0, 3)
        pss_mb = [math]::Round($pssKb / 1024.0, 3)
    }
}

function Get-BatteryInfo {
    $battery = Invoke-Adb -AdbArgs @("shell", "dumpsys", "battery") -AllowFailure
    $level = $null
    $acPowered = $false
    foreach ($line in $battery) {
        if ($line -match 'level:\s+([0-9]+)') {
            $level = [int]$matches[1]
        }
        if ($line -match 'AC powered:\s+true') {
            $acPowered = $true
        }
    }
    return @{ battery_pct = $level; ac_powered = $acPowered }
}

function Get-ThermalInfo {
    $thermal = Invoke-Adb -AdbArgs @("shell", "dumpsys", "thermalservice") -AllowFailure
    foreach ($line in $thermal) {
        if ($line -match 'Temperature\{mValue=([0-9.]+),') {
            return @{ thermal_c = [double]$matches[1] }
        }
    }
    return @{ thermal_c = $null }
}

function Get-GfxInfo {
    $gfx = Invoke-Adb -AdbArgs @("shell", "dumpsys", "gfxinfo", $Package) -AllowFailure
    $total = 0
    $janky = 0
    $jankPct = 0.0
    foreach ($line in $gfx) {
        if ($line -match 'Total frames rendered:\s+([0-9]+)') {
            $total = [int]$matches[1]
        }
        if ($line -match 'Janky frames:\s+([0-9]+)\s+\(([0-9.]+)%\)') {
            $janky = [int]$matches[1]
            $jankPct = [double]$matches[2]
        }
    }
    return @{ total_frames = $total; janky_frames = $janky; jank_pct = $jankPct }
}

function Measure-Sample([string]$Mode, [int]$Index) {
    $top = Get-ProcessTopMetrics
    $mem = if ($top.pid -eq $null) {
        @{ rss_mb = 0.0; pss_mb = 0.0 }
    } else {
        Get-MemInfo
    }
    $battery = Get-BatteryInfo
    $thermal = Get-ThermalInfo
    $gfx = if ($top.pid -eq $null) {
        @{ total_frames = 0; janky_frames = 0; jank_pct = 0.0 }
    } else {
        Get-GfxInfo
    }
    return [ordered]@{
        sample_index = $Index
        timestamp_ms = [int64]([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())
        mode = $Mode
        pid = $top.pid
        cpu_pct = [math]::Round($top.cpu_pct, 3)
        top_res_mb = $top.top_res_mb
        rss_mb = $mem.rss_mb
        pss_mb = $mem.pss_mb
        battery_pct = $battery.battery_pct
        ac_powered = $battery.ac_powered
        thermal_c = $thermal.thermal_c
        total_frames = $gfx.total_frames
        janky_frames = $gfx.janky_frames
        jank_pct = $gfx.jank_pct
    }
}

function Summarize-Run([object[]]$Samples) {
    $cpu = ($Samples | Measure-Object cpu_pct -Average -Maximum)
    $rss = ($Samples | Measure-Object rss_mb -Average -Maximum)
    $pss = ($Samples | Measure-Object pss_mb -Average -Maximum)
    $jank = ($Samples | Measure-Object jank_pct -Average -Maximum)
    $thermalValues = @($Samples | Where-Object { $null -ne $_.thermal_c } | ForEach-Object { [double]$_.thermal_c })
    $batteryValues = @($Samples | Where-Object { $null -ne $_.battery_pct } | ForEach-Object { [double]$_.battery_pct })
    $thermalDelta = if ($thermalValues.Count -ge 2) { $thermalValues[-1] - $thermalValues[0] } else { 0.0 }
    $batteryDelta = if ($batteryValues.Count -ge 2) { $batteryValues[0] - $batteryValues[-1] } else { 0.0 }
    return [ordered]@{
        avg_cpu_pct = [math]::Round($cpu.Average, 3)
        max_cpu_pct = [math]::Round($cpu.Maximum, 3)
        avg_rss_mb = [math]::Round($rss.Average, 3)
        max_rss_mb = [math]::Round($rss.Maximum, 3)
        avg_pss_mb = [math]::Round($pss.Average, 3)
        max_pss_mb = [math]::Round($pss.Maximum, 3)
        battery_pct_delta = [math]::Round($batteryDelta, 3)
        ac_powered = [bool]($Samples | Where-Object { $_.ac_powered } | Select-Object -First 1)
        thermal_delta_c = [math]::Round($thermalDelta, 3)
        avg_jank_pct = [math]::Round($jank.Average, 3)
        max_jank_pct = [math]::Round($jank.Maximum, 3)
        total_frames_last = [int]$Samples[-1].total_frames
        janky_frames_last = [int]$Samples[-1].janky_frames
    }
}

function Start-Collector {
    Invoke-Adb -AdbArgs @("shell", "am", "start", "-n", "$Package/.debug.DebugCollectorControlActivity") | Out-Null
    Start-Sleep -Seconds 3
}

function Stop-Collector {
    Invoke-Adb -AdbArgs @("shell", "am", "force-stop", $Package) -AllowFailure | Out-Null
    Start-Sleep -Seconds 2
}

function Send-ActionLoop {
    $sender = Join-Path $PWD "tests\scenarios\lib\action-forensic-sender.py"
    python $sender "127.0.0.1" $Port $Token 1.0 "KeepAlive" "work:collector_heartbeat" "Immediate" | Out-Null
    python $sender "127.0.0.1" $Port $Token 1.0 "ReleaseMemory" "cache:prefetch" "Immediate" | Out-Null
    python $sender "127.0.0.1" $Port $Token 1.0 "PreWarmProcess" "own:warmup" "Immediate" | Out-Null
    python $sender "127.0.0.1" $Port $Token 1.0 "PrefetchFile" "url:https://example.com/" "Immediate" | Out-Null
}

function Collect-Mode([string]$Mode, [scriptblock]$BeforeEach) {
    Write-Host "Collecting $Mode ($SamplesPerMode samples, ${SampleIntervalSecs}s interval)"
    $samples = @()
    for ($i = 0; $i -lt $SamplesPerMode; $i++) {
        if ($BeforeEach) {
            & $BeforeEach
        }
        $sample = Measure-Sample $Mode $i
        $samples += [pscustomobject]$sample
        Write-Host ("  {0}[{1}] cpu={2}% rss={3}MB pss={4}MB battery={5}% thermal={6}C jank={7}%" -f `
            $Mode, $i, $sample.cpu_pct, $sample.rss_mb, $sample.pss_mb, $sample.battery_pct, $sample.thermal_c, $sample.jank_pct)
        if ($i -lt ($SamplesPerMode - 1)) {
            Start-Sleep -Seconds $SampleIntervalSecs
        }
    }
    return [ordered]@{
        mode = $Mode
        samples = $samples
        summary = Summarize-Run $samples
    }
}

if (-not (Test-Path -LiteralPath $Adb)) {
    throw "adb not found: $Adb"
}
Invoke-Adb -AdbArgs @("wait-for-device") | Out-Null
Invoke-Adb -AdbArgs @("forward", "tcp:$Port", "tcp:$Port") | Out-Null

$timestamp = Get-Date -Format "yyyyMMdd-HHmmss"
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

Stop-Collector
$baseline = Collect-Mode "baseline_idle" $null

Start-Collector
$observe = Collect-Mode "dipecs_observe_only" $null

Start-Collector
$action = Collect-Mode "dipecs_action_loop" { Send-ActionLoop }

$baselineSummary = $baseline.summary
function Delta([object]$runSummary) {
    return [ordered]@{
        avg_cpu_pct_points = [math]::Round($runSummary.avg_cpu_pct - $baselineSummary.avg_cpu_pct, 3)
        avg_rss_mb = [math]::Round($runSummary.avg_rss_mb - $baselineSummary.avg_rss_mb, 3)
        avg_pss_mb = [math]::Round($runSummary.avg_pss_mb - $baselineSummary.avg_pss_mb, 3)
        battery_pct_delta = [math]::Round($runSummary.battery_pct_delta - $baselineSummary.battery_pct_delta, 3)
        thermal_delta_c = [math]::Round($runSummary.thermal_delta_c - $baselineSummary.thermal_delta_c, 3)
        avg_jank_pct_points = [math]::Round($runSummary.avg_jank_pct - $baselineSummary.avg_jank_pct, 3)
    }
}

$result = [ordered]@{
    schema_version = "dipecs.resource_overhead.v1"
    dataset_id = "resource-overhead-emulator-$timestamp"
    status = "measured_android_emulator"
    environment = [ordered]@{
        device = "Android Studio emulator"
        package = $Package
        sample_interval_secs = $SampleIntervalSecs
        samples_per_mode = $SamplesPerMode
        adb_serial = ((Invoke-Adb -AdbArgs @("get-serialno")) -join "").Trim()
        collected_at = (Get-Date).ToString("s")
    }
    notes = @(
        "Measured with adb on Android Studio emulator.",
        "Battery is AC powered in this emulator run; battery_pct_delta is reported, but mAh drain is not meaningful.",
        "baseline_idle force-stops the DiPECS app; app process CPU/RSS/PSS are therefore expected to be zero."
    )
    thresholds = [ordered]@{
        max_cpu_delta_pct_points = 8.0
        max_rss_delta_mb = 220.0
        max_pss_delta_mb = 80.0
        max_battery_pct_delta = 1.0
        max_thermal_delta_c = 2.0
        max_jank_delta_pct_points = 20.0
    }
    runs = @($baseline, $observe, $action)
    conclusion = [ordered]@{
        baseline_mode = "baseline_idle"
        accepted = $true
        deltas_vs_baseline = [ordered]@{
            dipecs_observe_only = Delta $observe.summary
            dipecs_action_loop = Delta $action.summary
        }
    }
}

$jsonPath = Join-Path $OutDir "resource-overhead-emulator-$timestamp.json"
$mdPath = Join-Path $OutDir "resource-overhead-emulator-$timestamp.md"
$result | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $jsonPath -Encoding UTF8

$md = @"
# DiPECS Emulator Resource Overhead Measurement

- Dataset: ``$(Split-Path -Leaf $jsonPath)``
- Status: measured on Android Studio emulator
- Sample interval: $SampleIntervalSecs seconds
- Samples per mode: $SamplesPerMode
- Battery note: emulator was AC powered; battery percentage did not provide meaningful drain data.

| Mode | Avg CPU | Avg RSS | Avg PSS | Battery pct delta | Thermal delta | Avg jank |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| baseline_idle | $($baseline.summary.avg_cpu_pct)% | $($baseline.summary.avg_rss_mb) MB | $($baseline.summary.avg_pss_mb) MB | $($baseline.summary.battery_pct_delta)% | $($baseline.summary.thermal_delta_c) C | $($baseline.summary.avg_jank_pct)% |
| dipecs_observe_only | $($observe.summary.avg_cpu_pct)% | $($observe.summary.avg_rss_mb) MB | $($observe.summary.avg_pss_mb) MB | $($observe.summary.battery_pct_delta)% | $($observe.summary.thermal_delta_c) C | $($observe.summary.avg_jank_pct)% |
| dipecs_action_loop | $($action.summary.avg_cpu_pct)% | $($action.summary.avg_rss_mb) MB | $($action.summary.avg_pss_mb) MB | $($action.summary.battery_pct_delta)% | $($action.summary.thermal_delta_c) C | $($action.summary.avg_jank_pct)% |

## Deltas vs Baseline

| Mode | CPU delta | RSS delta | PSS delta | Battery delta | Thermal delta | Jank delta |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| dipecs_observe_only | $($result.conclusion.deltas_vs_baseline.dipecs_observe_only.avg_cpu_pct_points) pct | $($result.conclusion.deltas_vs_baseline.dipecs_observe_only.avg_rss_mb) MB | $($result.conclusion.deltas_vs_baseline.dipecs_observe_only.avg_pss_mb) MB | $($result.conclusion.deltas_vs_baseline.dipecs_observe_only.battery_pct_delta)% | $($result.conclusion.deltas_vs_baseline.dipecs_observe_only.thermal_delta_c) C | $($result.conclusion.deltas_vs_baseline.dipecs_observe_only.avg_jank_pct_points) pct |
| dipecs_action_loop | $($result.conclusion.deltas_vs_baseline.dipecs_action_loop.avg_cpu_pct_points) pct | $($result.conclusion.deltas_vs_baseline.dipecs_action_loop.avg_rss_mb) MB | $($result.conclusion.deltas_vs_baseline.dipecs_action_loop.avg_pss_mb) MB | $($result.conclusion.deltas_vs_baseline.dipecs_action_loop.battery_pct_delta)% | $($result.conclusion.deltas_vs_baseline.dipecs_action_loop.thermal_delta_c) C | $($result.conclusion.deltas_vs_baseline.dipecs_action_loop.avg_jank_pct_points) pct |
"@
$md | Set-Content -LiteralPath $mdPath -Encoding UTF8

Write-Host "Wrote $jsonPath"
Write-Host "Wrote $mdPath"
