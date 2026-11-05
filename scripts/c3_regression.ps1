# Task C.3: Regression Verification & Performance Benchmarking (PowerShell)
#
# Runs, from the repository root:
#   1. `cargo test --release` in gardelc/ (all tests must pass)
#   2. `gardelc --stdlib --strict`        (0 errors, 0 warnings)
#   3. `gardelc examples/<x>.gdl --strict` for every example (0 errors, 0 warnings)
#   4. native C11 runs: greeter and seq_ops (compiled from Gardel source), then the SEQ
#      benchmark twice: examples/seq_bench.gdl compiled from Gardel, and the HAND-WRITTEN
#      reference runtime gardelc/runtime/reference/seq.c (not compiled from Gardel)
# and writes a Markdown report with wall-clock timings to build/c3_report.md.
# Exits non-zero if any regression check fails. Timings are informational.
param(
    [int[]]$SeqSizes = @(100000, 1000000, 4000000),
    [int]$Repeat = 3,
    [int[]]$Seeds = @(0, 1, 42),
    [switch]$SkipTests,
    [switch]$SkipMultiSeed
)

# "Continue": Windows PowerShell 5 turns native stderr (cargo, gcc) into terminating
# errors under "Stop". Every native call checks $LASTEXITCODE explicitly instead.
$ErrorActionPreference = "Continue"
# Windows PowerShell 5 prefixes text piped to native programs with the console input
# encoding's byte-order mark (U+FEFF); the programs under test would (correctly) reject it
# as input. Both encodings must be BOM-less UTF-8.
$NoBom = New-Object System.Text.UTF8Encoding $false
[Console]::InputEncoding = $NoBom
$OutputEncoding = $NoBom
$Root = Resolve-Path "$PSScriptRoot\.."
Set-Location $Root
$Gardelc = Join-Path $Root "gardelc\target\release\gardelc.exe"
$Failures = New-Object System.Collections.Generic.List[string]
$Report = New-Object System.Collections.Generic.List[string]

function Step($msg) { Write-Host "=== $msg" -ForegroundColor Cyan }
function Fail($msg) { Write-Host "[FAIL] $msg" -ForegroundColor Red; $Failures.Add($msg) }
function Ok($msg) { Write-Host "[OK] $msg" -ForegroundColor Green }

# Runs gardelc and returns @{ Errors; Warnings; Seconds; Exit; Output }.
function Invoke-Gardelc([string[]]$GardelArgs) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $out = & $Gardelc @GardelArgs 2>&1 | Out-String
    $sw.Stop()
    $code = $LASTEXITCODE
    $e = -1; $w = -1
    if ($out -match '(\d+) errors?, (\d+) warnings?') { $e = [int]$Matches[1]; $w = [int]$Matches[2] }
    @{ Errors = $e; Warnings = $w; Seconds = $sw.Elapsed.TotalSeconds; Exit = $code; Output = $out }
}

$Report.Add("# Task C.3 Regression & Benchmark Report")
$Report.Add("")
$Report.Add("- Date: $(Get-Date -Format 'yyyy-MM-dd HH:mm')")
$Report.Add("- Commit: $(git rev-parse --short HEAD 2>$null) (dirty: $([bool](git status --porcelain 2>$null)))")
$Report.Add("- CPU: $((Get-CimInstance Win32_Processor | Select-Object -First 1).Name.Trim()) ($([Environment]::ProcessorCount) logical)")
$Report.Add("- Tools: $(& boogie /version 2>$null | Select-Object -First 1); $(& gcc --version 2>$null | Select-Object -First 1)")
$Report.Add("")

Step "Build gardelc (release)"
Push-Location gardelc
cargo build --release 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) { Pop-Location; throw "cargo build failed" }

# 1. Unit tests
if (-not $SkipTests) {
    Step "cargo test --release"
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $testOut = cargo test --release 2>&1 | Out-String
    $sw.Stop()
    if ($testOut -match 'test result: \w+\. (\d+) passed; (\d+) failed') {
        $passed = [int]$Matches[1]; $failed = [int]$Matches[2]
        if ($LASTEXITCODE -eq 0 -and $failed -eq 0) { Ok "$passed passed" } else { Fail "cargo test: $failed failed" }
        $Report.Add("## 1. Unit tests")
        $Report.Add("")
        $Report.Add("``cargo test --release``: **$passed passed, $failed failed** in $([math]::Round($sw.Elapsed.TotalSeconds, 1)) s")
        $Report.Add("")
    } else { Fail "cargo test: no result line" }
}
Pop-Location

# 2 + 3. Verification under --strict (also the SMT benchmark)
$Report.Add("## 2. Verification under ``--strict`` (Boogie/Z3 wall clock, best of $Repeat)")
$Report.Add("")
$Report.Add("| Target | Errors | Warnings | Best (s) | Mean (s) |")
$Report.Add("| :--- | ---: | ---: | ---: | ---: |")
$targets = @(@{ Name = "stdlib (13 files)"; Args = @("--stdlib", "--strict") })
foreach ($f in Get-ChildItem examples -Filter *.gdl | Sort-Object Name) {
    $targets += @{ Name = "examples/$($f.Name)"; Args = @("examples/$($f.Name)", "--strict") }
}
foreach ($t in $targets) {
    Step "gardelc $($t.Args -join ' ')"
    $runs = @()
    for ($i = 0; $i -lt $Repeat; $i++) { $runs += Invoke-Gardelc $t.Args }
    $last = $runs[-1]
    $times = $runs | ForEach-Object { $_.Seconds }
    $best = [math]::Round(($times | Measure-Object -Minimum).Minimum, 2)
    $mean = [math]::Round(($times | Measure-Object -Average).Average, 2)
    if ($last.Exit -eq 0 -and $last.Errors -eq 0 -and $last.Warnings -eq 0) {
        Ok "$($t.Name): 0/0 (best $best s)"
    } else {
        Fail "$($t.Name): exit $($last.Exit), $($last.Errors) errors, $($last.Warnings) warnings"
        Write-Host $last.Output
    }
    $Report.Add("| $($t.Name) | $($last.Errors) | $($last.Warnings) | $best | $mean |")
}
$Report.Add("")

# 2b. Multi-seed proof stability
if (-not $SkipMultiSeed) {
    Step "Multi-seed proof stability (seeds: $($Seeds -join ', '))"
    $Report.Add("## 2b. Multi-seed proof stability")
    $Report.Add("")
    $Report.Add("Proofs checked across multiple Z3 seeds to verify stability against solver search variations (§16.4).")
    $Report.Add("")
    $seedHeaders = ($Seeds | ForEach-Object { "Seed $_" }) -join ' | '
    $seedAlign = ($Seeds | ForEach-Object { "---:" }) -join ' | '
    $Report.Add("| Target | $seedHeaders |")
    $Report.Add("| :--- | $seedAlign |")

    foreach ($t in $targets) {
        $rowCells = @()
        $targetOk = $true
        foreach ($seed in $Seeds) {
            $seedArgs = $t.Args + @("--z3-seed", "$seed")
            $res = Invoke-Gardelc $seedArgs
            if ($res.Exit -eq 0 -and $res.Errors -eq 0 -and $res.Warnings -eq 0) {
                $rowCells += "$([math]::Round($res.Seconds, 2)) s"
            } else {
                $targetOk = $false
                $rowCells += "**FAIL** (exit $($res.Exit), $($res.Errors)e)"
                Fail "$($t.Name) [seed $seed]: exit $($res.Exit), $($res.Errors) errors, $($res.Warnings) warnings"
            }
        }
        if ($targetOk) {
            Ok "$($t.Name): stable across all $($Seeds.Count) seeds"
        }
        $Report.Add("| $($t.Name) | $($rowCells -join ' | ') |")
    }
    $Report.Add("")
}

# 4a. Native greeter (built by `gardelc examples/greeter.gdl` above)
$Report.Add("## 3. Native C11 runs")
$Report.Add("")
Step "native greeter"
$greeter = Join-Path $Root "build\greeter_bin.exe"
if (Test-Path $greeter) {
    $sw = [Diagnostics.Stopwatch]::StartNew()
    $gOut = "World" | & $greeter | Out-String
    $sw.Stop()
    if ($LASTEXITCODE -eq 0 -and [string]::Equals($gOut.TrimEnd([char]13, [char]10), "Hello, World", [StringComparison]::Ordinal)) {
        Ok "greeter: '$($gOut.Trim())'"
        $Report.Add("- ``greeter_bin.exe`` with stdin ``World``: printed ``Hello, World``, exit 0, $([math]::Round($sw.Elapsed.TotalMilliseconds)) ms")
    } else { Fail "greeter: exit $LASTEXITCODE, output '$($gOut.Trim())'" }
} else { Fail "greeter: $greeter was not built (is gcc on PATH?)" }

# 4b. SEQ compiled from Gardel source: every SEQ routine against run-time checks.
Step "native seq_ops (stdlib SEQ compiled from source)"
$seqOps = Join-Path $Root "build\seq_ops_bin.exe"
if (Test-Path $seqOps) {
    & $seqOps | Out-Null
    if ($LASTEXITCODE -eq 0) {
        Ok "seq_ops: all checks passed"
        $Report.Add("- ``seq_ops_bin.exe`` (``examples/seq_ops.gdl``, stdlib ``SEQ`` compiled from source): all run-time checks passed")
    } else { Fail "seq_ops: check $LASTEXITCODE failed" }
} else { Fail "seq_ops: $seqOps was not built" }
$Report.Add("")

# 4c. SEQ benchmarks. Times are wall clock including process start, best of $Repeat.
function Invoke-SeqBench([string]$Label, [string]$Exe, [scriptblock]$RunOnce) {
    $Report.Add("$Label")
    $Report.Add("")
    $Report.Add("| n | 1 thread (ms) | $([Environment]::ProcessorCount) threads (ms) | Speedup |")
    $Report.Add("| ---: | ---: | ---: | ---: |")
    foreach ($n in $SeqSizes) {
        $best = @{}
        foreach ($threads in @(1, [Environment]::ProcessorCount)) {
            $env:OMP_NUM_THREADS = "$threads"
            $ms = @()
            for ($i = 0; $i -lt $Repeat; $i++) {
                $sw = [Diagnostics.Stopwatch]::StartNew()
                $code = & $RunOnce $Exe $n
                $sw.Stop()
                if ($code -ne 0) { Fail "${Exe} n=${n}: exit $code (wrong sum or bad input)" }
                $ms += $sw.Elapsed.TotalMilliseconds
            }
            $best[$threads] = ($ms | Measure-Object -Minimum).Minimum
        }
        Remove-Item Env:OMP_NUM_THREADS
        $one = $best[1]; $many = $best[[Environment]::ProcessorCount]
        $speed = [math]::Round($one / $many, 2)
        Ok "$(Split-Path $Exe -Leaf) n=${n}: 1t $([math]::Round($one)) ms, Nt $([math]::Round($many)) ms (x$speed)"
        $Report.Add("| $n | $([math]::Round($one)) | $([math]::Round($many)) | x$speed |")
    }
    $Report.Add("")
}

Step "SEQ benchmark: compiled from Gardel (examples/seq_bench.gdl)"
$benchExe = Join-Path $Root "build\seq_bench_bin.exe"
if (Test-Path $benchExe) {
    Invoke-SeqBench "- **Compiled from Gardel** by gardelc: ``examples/seq_bench.gdl`` (verified 0/0) with the stdlib ``SEQ`` lowered from ``stdlib/collections/seq.gdl`` (``gcc -O3 -fopenmp``). The build (``from_range``: ``SEQ.singleton`` + ``concat``) and the sum (a structural recursion on SEQ's constructors) fork their halves as OpenMP tasks. Memory is reference counted: the tree is freed before the program exits. ``n`` is read from stdin." $benchExe {
        param($exe, $n)
        "$n" | & $exe | Out-Null
        $LASTEXITCODE
    }
} else { Fail "seq_bench: $benchExe was not built" }

Step "SEQ benchmark: hand-written reference runtime (not compiled from Gardel)"
$seqC = Join-Path $Root "gardelc\runtime\reference\seq.c"
$seqExe = Join-Path $Root "build\seq_reference_bench.exe"
New-Item -ItemType Directory -Force -Path build | Out-Null
gcc -std=gnu11 -O3 -fopenmp $seqC -o $seqExe 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) {
    Fail "SEQ: gardelc/runtime/reference/seq.c failed to compile with -fopenmp"
} else {
    Invoke-SeqBench "- **Hand-written reference runtime** (not compiled from Gardel, not covered by verification): ``gardelc/runtime/reference/seq.c`` (``gcc -O3 -fopenmp``): ``seq_from_range(1, n)`` then a parallel ``seq_parallel_sum``; it frees nothing. ``n`` is an argument." $seqExe {
        param($exe, $n)
        & $exe $n | Out-Null
        $LASTEXITCODE
    }
}

$Report.Add("## Result")
$Report.Add("")
if ($Failures.Count -eq 0) { $Report.Add("**PASS**: all regression checks passed.") }
else { $Report.Add("**FAIL**:"); $Failures | ForEach-Object { $Report.Add("- $_") } }

New-Item -ItemType Directory -Force -Path build | Out-Null
$Report | Set-Content -Encoding UTF8 build\c3_report.md
Write-Host "Report: build\c3_report.md"
if ($Failures.Count -ne 0) { exit 1 }
