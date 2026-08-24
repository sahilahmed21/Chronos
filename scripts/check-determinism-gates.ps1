# D6 / D16: protocol and sim source must not use HashMap/HashSet or host I/O types.
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$script:gateFailed = $false

function Check-Dir {
    param([string]$Dir, [string[]]$Needles)
    Get-ChildItem -Path $Dir -Filter *.rs -Recurse | ForEach-Object {
        $file = $_.FullName
        $lineNo = 0
        Get-Content $file | ForEach-Object {
            $lineNo++
            $line = $_
            $trim = $line.TrimStart()
            if ($trim.StartsWith("//")) { return }
            foreach ($n in $Needles) {
                if ($line.Contains($n)) {
                    Write-Output ("{0}:{1}: {2}" -f $file, $lineNo, $n)
                    $script:gateFailed = $true
                }
            }
        }
    }
}

Check-Dir "$root\crates\chronos-protocol\src" @(
    "std::collections::HashMap",
    "std::collections::HashSet",
    "std::fs",
    "std::net::",
    "std::thread::",
    "std::time::Instant",
    "std::time::SystemTime"
)
Check-Dir "$root\crates\chronos-sim\src" @(
    "std::collections::HashMap",
    "std::collections::HashSet"
)

if ($script:gateFailed) {
    Write-Output "determinism gate failed"
    exit 1
}
Write-Output "determinism gates ok"
