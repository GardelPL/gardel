# Gardel Verification Tools Setup (PowerShell)
param(
    [string]$DafnyVersion = "4.9.1",
    [string]$TargetDir = "$PSScriptRoot\..\tools\dafny"
)

$ErrorActionPreference = "Stop"

Write-Host "=== Setting up Gardel Verification Tools ===" -ForegroundColor Cyan

# 1. Setup Dafny & bundled Z3
if (Test-Path "$TargetDir\Dafny.exe") {
    Write-Host "[OK] Dafny already installed at $TargetDir" -ForegroundColor Green
} else {
    Write-Host "[*] Downloading Dafny v$DafnyVersion for Windows x64..." -ForegroundColor Yellow
    $zipUrl = "https://github.com/dafny-lang/dafny/releases/download/v${DafnyVersion}/dafny-${DafnyVersion}-x64-windows.zip"
    $tempZip = Join-Path $env:TEMP "dafny-${DafnyVersion}-windows.zip"

    Invoke-WebRequest -Uri $zipUrl -OutFile $tempZip
    Write-Host "[*] Extracting to $TargetDir..." -ForegroundColor Yellow

    $parentDir = Split-Path -Parent $TargetDir
    if (-not (Test-Path $parentDir)) {
        New-Item -ItemType Directory -Path $parentDir -Force | Out-Null
    }

    Expand-Archive -Path $tempZip -DestinationPath $parentDir -Force
    Remove-Item $tempZip -Force
    Write-Host "[OK] Dafny installed to $TargetDir" -ForegroundColor Green
}

# 2. Add Dafny and Z3 to current session PATH
$dafnyResolved = (Resolve-Path $TargetDir).Path
$z3Path = Join-Path $dafnyResolved "z3\bin"

if ($env:PATH -notlike "*$dafnyResolved*") {
    $env:PATH = "$dafnyResolved;$z3Path;$env:PATH"
    Write-Host "[*] Added Dafny and Z3 to current session PATH" -ForegroundColor DarkGray
}

# 3. Setup Boogie via dotnet
Write-Host "[*] Checking Boogie installation..." -ForegroundColor Yellow
if (Get-Command "boogie" -ErrorAction SilentlyContinue) {
    Write-Host "[OK] Boogie is already available in PATH" -ForegroundColor Green
} elseif (Get-Command "dotnet" -ErrorAction SilentlyContinue) {
    Write-Host "[*] Installing Boogie via dotnet tool..." -ForegroundColor Yellow
    try {
        & dotnet tool install --global Boogie
    } catch {
        # Fall back to net7.0-compatible Boogie 3.1.6 if dotnet is v7
        Write-Host "[*] Retrying with .NET 7 compatible version (Boogie 3.1.6)..." -ForegroundColor Yellow
        & dotnet tool install --global Boogie --version 3.1.6
    }
    Write-Host "[OK] Boogie installed via dotnet" -ForegroundColor Green
} else {
    Write-Warning ".NET SDK (dotnet) not found. Boogie requires .NET SDK: https://dotnet.microsoft.com/download"
}

# 4. Summary Verification
Write-Host ""
Write-Host "=== Verification Summary ===" -ForegroundColor Cyan
if (Get-Command "dafny" -ErrorAction SilentlyContinue) {
    $v = (dafny --version 2>&1) | Out-String
    Write-Host "[OK] Dafny: $($v.Trim())" -ForegroundColor Green
} else {
    Write-Host "[INFO] Dafny executable located at: $dafnyResolved\Dafny.exe" -ForegroundColor Yellow
}

$z3Exe = Get-ChildItem -Path "$z3Path" -Filter "z3*.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
if ($z3Exe) {
    Write-Host "[OK] Z3 (bundled with Dafny): $($z3Exe.FullName)" -ForegroundColor Green
}

if (Get-Command "boogie" -ErrorAction SilentlyContinue) {
    Write-Host "[OK] Boogie: available" -ForegroundColor Green
} else {
    $dotnetTools = Join-Path $env:USERPROFILE ".dotnet\tools"
    Write-Host "[INFO] Boogie tool path: $dotnetTools" -ForegroundColor Yellow
}

Write-Host ""
Write-Host "Ready to run: cd gardelc; cargo run -- --stdlib --run" -ForegroundColor Cyan
