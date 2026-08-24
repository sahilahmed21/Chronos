# P7: run one swarm seed twice. Exit 2 if digest or check lines differ.
# Usage: powershell -File scripts/repro.ps1 <seed>
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
if ($args.Count -lt 1) {
    Write-Error "usage: scripts/repro.ps1 <seed>"
}
$seed = $args[0]
Set-Location $root

function Invoke-SeedLine {
    param([string]$Seed)
    $out = & cargo run -q -p chronos-sim -- --seed $Seed 2>&1 | Out-String
    $line = ($out -split "`r?`n" | Where-Object { $_ -match '^(ok|FAIL|ABORT) seed=' } | Select-Object -Last 1)
    if (-not $line) {
        Write-Output $out
        throw "no swarm status line for seed $Seed"
    }
    $line
}

$a = Invoke-SeedLine $seed
$b = Invoke-SeedLine $seed
Write-Output $a
Write-Output $b
if ($a -ne $b) {
    Write-Error "MISMATCH seed $seed"
    exit 2
}
if ($a -match '^ok ') {
    exit 0
}
exit 1
