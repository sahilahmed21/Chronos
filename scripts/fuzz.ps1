# P6 swarm batch. Usage: powershell -File scripts/fuzz.ps1 [N]
# P9: N=50 on PR, N=1000 nightly. 10k is a stretch goal, not a start gate.
# P7 hunt starts at N=100.
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$n = if ($args.Count -ge 1) { $args[0] } else { "100" }
Set-Location $root
$out = Join-Path $root "traces"
New-Item -ItemType Directory -Force -Path $out | Out-Null
& cargo run -p chronos-sim -- --seeds $n --start 0 --out $out
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}
