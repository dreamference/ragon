<#
.SYNOPSIS
Packages the Windows HIP engine as the release asset ragon-engine-<version>-windows-gfx1151.zip
and appends its line to SHA256SUMS.

.DESCRIPTION
The zip is flat: the engine executable, every DLL it loads that is not part of Windows, and
ENGINE-MANIFEST.txt (version, engine commit, SHA-256 of each file). The node unpacks it into
%ProgramFiles%\Ragon\engine; Windows then finds the DLLs beside the executable, so no ROCm or
TheRock install is needed on PATH.

How the DLLs are found:
- The executable's import and delay-import tables are read (a small PE parser below), then
  those of every DLL found, recursively.
- A DLL is shipped when it sits in -BuildDir or one of -RuntimeDirs (TheRock's bin by default).
  Anything else (System32, the API sets) is part of Windows and is not shipped; a name found
  nowhere is reported and fails the run unless it is an api-ms-*/ext-ms-* set.
- Libraries HIP loads at run time by name (amd_comgr*, hiprtc*) are not in any import table;
  -Extra adds them by wildcard (the default covers both).

Verification: with -VerifyArgs the staged executable is run from the staging directory with
PATH reduced to System32 and HIP_PATH/ROCM_PATH removed, so a missing DLL fails here and not on
a node. The exit code must be 0.

Reproducible: entries are sorted by name and stamped 2000-01-01 00:00, so the same inputs give
the same zip and the same SHA-256.

.EXAMPLE
pwsh scripts/package_windows_engine.ps1 -BuildDir C:\build\bin -Version 0.9.4 `
  -Commit b60634e -OutDir C:\out -VerifyArgs '--help'
#>
param(
    [Parameter(Mandatory = $true)][string]$BuildDir,
    [Parameter(Mandatory = $true)][string]$Version,
    [Parameter(Mandatory = $true)][string]$OutDir,
    [string]$Exe = 'ragon-serve.exe',
    [string]$Commit = 'unknown',
    [string[]]$RuntimeDirs = @('C:\TheRock\build\bin'),
    [string[]]$Extra = @('amd_comgr*.dll', 'hiprtc*.dll'),
    [string]$VerifyArgs = '',
    [string]$Arch = 'gfx1151'
)
$ErrorActionPreference = 'Stop'

function Get-PeImports([string]$Path) {
    $b = [System.IO.File]::ReadAllBytes($Path)
    $pe = [BitConverter]::ToInt32($b, 0x3C)
    if ([BitConverter]::ToUInt32($b, $pe) -ne 0x4550) { throw "$Path is not a PE file" }
    $coff = $pe + 4
    $nsec = [BitConverter]::ToUInt16($b, $coff + 2)
    $optSize = [BitConverter]::ToUInt16($b, $coff + 16)
    $opt = $coff + 20
    $magic = [BitConverter]::ToUInt16($b, $opt)
    $dirs = if ($magic -eq 0x20b) { $opt + 112 } else { $opt + 96 }
    $sections = for ($i = 0; $i -lt $nsec; $i++) {
        $s = $opt + $optSize + 40 * $i
        [pscustomobject]@{
            Va   = [BitConverter]::ToUInt32($b, $s + 12)
            Size = [Math]::Max([BitConverter]::ToUInt32($b, $s + 8), [BitConverter]::ToUInt32($b, $s + 16))
            Raw  = [BitConverter]::ToUInt32($b, $s + 20)
        }
    }
    $off = {
        param([uint32]$rva)
        foreach ($s in $sections) {
            if ($rva -ge $s.Va -and $rva -lt $s.Va + $s.Size) { return [int]($rva - $s.Va + $s.Raw) }
        }
        return -1
    }
    $str = {
        param([int]$o)
        $e = $o
        while ($b[$e] -ne 0) { $e++ }
        [System.Text.Encoding]::ASCII.GetString($b, $o, $e - $o)
    }
    $names = New-Object System.Collections.Generic.List[string]
    # Directory 1: imports (20-byte descriptors, name RVA at +12, ends with a zero descriptor).
    $rva = [BitConverter]::ToUInt32($b, $dirs + 8)
    if ($rva -ne 0) {
        $o = & $off $rva
        while ($o -ge 0) {
            $name = [BitConverter]::ToUInt32($b, $o + 12)
            if ($name -eq 0) { break }
            $names.Add((& $str (& $off $name)))
            $o += 20
        }
    }
    # Directory 13: delay imports (32-byte descriptors, name RVA at +4).
    $rva = [BitConverter]::ToUInt32($b, $dirs + 13 * 8)
    if ($rva -ne 0) {
        $o = & $off $rva
        while ($o -ge 0) {
            $name = [BitConverter]::ToUInt32($b, $o + 4)
            if ($name -eq 0) { break }
            $names.Add((& $str (& $off $name)))
            $o += 32
        }
    }
    return $names
}

$exePath = Join-Path $BuildDir $Exe
if (-not (Test-Path $exePath)) { throw "no $exePath" }
$searchDirs = @($BuildDir) + @($RuntimeDirs | Where-Object { Test-Path $_ })
$system = Join-Path $env:SystemRoot 'System32'

# Walk the import closure.
$ship = [ordered]@{}            # lower-case file name -> full path
$seen = @{}
$missing = New-Object System.Collections.Generic.List[string]
$queue = New-Object System.Collections.Generic.Queue[string]
$queue.Enqueue($exePath)
foreach ($dir in $searchDirs) {
    foreach ($pattern in $Extra) {
        Get-ChildItem -Path $dir -Filter $pattern -File -ErrorAction SilentlyContinue | ForEach-Object {
            if (-not $ship.Contains($_.Name.ToLower())) { $ship[$_.Name.ToLower()] = $_.FullName; $queue.Enqueue($_.FullName) }
        }
    }
}
while ($queue.Count -gt 0) {
    $file = $queue.Dequeue()
    foreach ($dll in Get-PeImports $file) {
        $key = $dll.ToLower()
        if ($seen.ContainsKey($key)) { continue }
        $seen[$key] = $true
        $found = $null
        foreach ($dir in $searchDirs) {
            $p = Join-Path $dir $dll
            if (Test-Path $p) { $found = (Resolve-Path $p).Path; break }
        }
        if ($found) {
            if (-not $ship.Contains($key)) { $ship[$key] = $found; $queue.Enqueue($found) }
        } elseif ($key -match '^(api|ext)-ms-') {
            continue
        } elseif (-not (Test-Path (Join-Path $system $dll))) {
            $missing.Add("$dll (imported by $(Split-Path $file -Leaf))")
        }
    }
}
if ($missing.Count -gt 0) { throw "DLLs found neither in the build, the runtime dirs nor System32: $($missing -join '; ')" }

# Stage flat.
$name = "ragon-engine-$Version-windows-$Arch"
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$stage = Join-Path $OutDir $name
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Path $stage | Out-Null
Copy-Item $exePath (Join-Path $stage $Exe)
foreach ($path in $ship.Values) { Copy-Item $path (Join-Path $stage (Split-Path $path -Leaf)) }
$files = Get-ChildItem -File $stage | Sort-Object Name
$manifest = @("ragon-engine $Version windows $Arch", "engine commit $Commit", '')
foreach ($f in $files) { $manifest += "$((Get-FileHash -Algorithm SHA256 $f.FullName).Hash.ToLower())  $($f.Name)" }
[System.IO.File]::WriteAllText((Join-Path $stage 'ENGINE-MANIFEST.txt'), (($manifest -join "`n") + "`n"))

# Verify with no SDK on PATH.
if ($VerifyArgs -ne '') {
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = Join-Path $stage $Exe
    $psi.Arguments = $VerifyArgs
    $psi.WorkingDirectory = $stage
    $psi.UseShellExecute = $false
    $psi.EnvironmentVariables['PATH'] = "$system;$env:SystemRoot"
    foreach ($v in 'HIP_PATH', 'ROCM_PATH', 'HIP_PATH_64') { $psi.EnvironmentVariables.Remove($v) }
    $proc = [System.Diagnostics.Process]::Start($psi)
    $proc.WaitForExit()
    if ($proc.ExitCode -ne 0) { throw "$Exe $VerifyArgs exited $($proc.ExitCode) with PATH reduced to System32" }
    Write-Host "verified: $Exe $VerifyArgs exits 0 with PATH reduced to System32"
}

# Zip reproducibly.
Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
$zip = Join-Path $OutDir "$name.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
$stamp = [DateTimeOffset]::new(2000, 1, 1, 0, 0, 0, [TimeSpan]::Zero)
$fs = [System.IO.File]::Open($zip, [System.IO.FileMode]::CreateNew)
$archive = New-Object System.IO.Compression.ZipArchive($fs, [System.IO.Compression.ZipArchiveMode]::Create)
foreach ($f in (Get-ChildItem -File $stage | Sort-Object { $_.Name } -Culture '')) {
    $entry = $archive.CreateEntry($f.Name, [System.IO.Compression.CompressionLevel]::Optimal)
    $entry.LastWriteTime = $stamp
    $out = $entry.Open()
    $in = [System.IO.File]::OpenRead($f.FullName)
    $in.CopyTo($out)
    $in.Dispose(); $out.Dispose()
}
$archive.Dispose(); $fs.Dispose()

$sha = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLower()
$line = "$sha  $name.zip"
$sums = Join-Path $OutDir 'SHA256SUMS'
$kept = @()
if (Test-Path $sums) { $kept = @(Get-Content $sums | Where-Object { $_ -notmatch "  $([regex]::Escape("$name.zip"))$" }) }
[System.IO.File]::WriteAllText($sums, ((@($kept) + $line) -join "`n") + "`n")
Write-Host "files: $($files.Count) ($(($ship.Keys | Sort-Object) -join ', '))"
Write-Host $line
