# Benchmark comparison between Gardel (generated C11+OpenMP), C (idiomatic C11+OpenMP), and Rust (idiomatic Rust+Rayon)
param(
    [int[]]$Sizes = @(100000, 1000000, 4000000),
    [int]$Repeat = 3
)

$ErrorActionPreference = "Continue"
$Root = Resolve-Path "$PSScriptRoot\.."
Set-Location $Root

$LogicalCores = [Environment]::ProcessorCount
Write-Host "Running Benchmark on $LogicalCores logical cores (Repeat=$Repeat)" -ForegroundColor Cyan

$GardelExe = Join-Path $Root "build\gardel_bench.exe"
$CExe = Join-Path $Root "build\c_bench.exe"
$RustExe = Join-Path $Root "benchmarks\seq_comparison\rust\target\release\seq_bench_rust.exe"

function Run-Target([string]$Name, [string]$ExePath, [int]$N, [int]$Threads) {
    if ($Name -eq "Rust") {
        $env:RAYON_NUM_THREADS = "$Threads"
    } else {
        $env:OMP_NUM_THREADS = "$Threads"
    }

    $bestTotal = [double]::MaxValue
    $bestBuild = 0
    $bestSum = 0

    for ($i = 0; $i -lt $Repeat; $i++) {
        $out = & $ExePath $N 2>&1 | Out-String
        if ($LASTEXITCODE -ne 0) {
            Write-Host "Error running $Name ($N, $Threads): $out" -ForegroundColor Red
            return $null
        }
        if ($out -match 'build_ms=([\d\.]+)\s+sum_ms=([\d\.]+)\s+total_ms=([\d\.]+)') {
            $b = [double]$Matches[1]
            $s = [double]$Matches[2]
            $tot = [double]$Matches[3]
            if ($tot -lt $bestTotal) {
                $bestTotal = $tot
                $bestBuild = $b
                $bestSum = $s
            }
        }
    }

    Remove-Item Env:OMP_NUM_THREADS -ErrorAction SilentlyContinue
    Remove-Item Env:RAYON_NUM_THREADS -ErrorAction SilentlyContinue

    return @{
        Language = $Name
        N = $N
        Threads = $Threads
        BuildMs = [math]::Round($bestBuild, 2)
        SumMs = [math]::Round($bestSum, 2)
        TotalMs = [math]::Round($bestTotal, 2)
    }
}

$results = @()
$targets = @(
    @{ Name = "Gardel"; Exe = $GardelExe },
    @{ Name = "C";      Exe = $CExe },
    @{ Name = "Rust";   Exe = $RustExe }
)

foreach ($n in $Sizes) {
    Write-Host "`n=== Benchmark for N = $n ===" -ForegroundColor Yellow
    foreach ($threads in @(1, $LogicalCores)) {
        Write-Host "--- Threads: $threads ---" -ForegroundColor Gray
        foreach ($t in $targets) {
            $res = Run-Target $t.Name $t.Exe $n $threads
            if ($res) {
                $thStr = if ($threads -eq 1) { "1t" } else { "${threads}t" }
                Write-Host ("[{0,-6}] ({1}) Total: {2,7} ms (build: {3,6} ms, sum: {4,6} ms)" -f $res.Language, $thStr, $res.TotalMs, $res.BuildMs, $res.SumMs) -ForegroundColor Green
                $results += $res
            }
        }
    }
}

# Export results to JSON
$results | ConvertTo-Json | Set-Content (Join-Path $Root "build\benchmark_comparison.json")
Write-Host "`nSaved results to build\benchmark_comparison.json" -ForegroundColor Cyan

