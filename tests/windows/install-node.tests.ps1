# install.ps1 -Role node, the install, update-record and uninstall logic, against a temp prefix.
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File tests\windows\install-node.tests.ps1
#
# The functions are taken out of install.ps1 by syntax tree (the script itself is not run). The
# service manager, icacls and ragon are fake .cmd files that append their arguments to a log; the
# accounts are scriptblocks that record. Nothing here touches a real service, account, firewall
# rule or ACL. Plain PowerShell 5.1, no Pester (the box has 3.x).
param([string]$Script = (Join-Path $PSScriptRoot "..\..\install.ps1"))
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.IO.Compression.FileSystem

$Tokens = $null; $Errors = $null
$Tree = [Management.Automation.Language.Parser]::ParseFile((Resolve-Path $Script).Path, [ref]$Tokens, [ref]$Errors)
if ($Errors.Count) { throw "install.ps1 does not parse: $($Errors[0].Message)" }
$Pieces = @()
foreach ($Statement in $Tree.EndBlock.Statements) {
    if ($Statement -is [Management.Automation.Language.FunctionDefinitionAst] -and $Statement.Name -match '^(Get-Node|Test-Zip|Expand-Node|Invoke-Node|Set-Node|Remove-Node|Test-Node|Save-Node|Install-RagonNode|Uninstall-RagonNode)') {
        $Pieces += $Statement.Extent.Text
    } elseif ($Statement -is [Management.Automation.Language.AssignmentStatementAst] -and $Statement.Left.Extent.Text -match '^\$Node(Service|Job|Sandbox|Sshd)') {
        $Pieces += $Statement.Extent.Text
    }
}
Invoke-Expression ($Pieces -join "`n")

$Failures = 0
$Count = 0
function Check([string]$Name, $Actual, $Expected) {
    $script:Count++
    if ("$Actual" -ne "$Expected") { $script:Failures++; Write-Host "FAIL $Name`n   expected: $Expected`n   actual:   $Actual" -ForegroundColor Red }
    else { Write-Host "ok   $Name" }
}
function Throws([string]$Name, [scriptblock]$Body, [string]$Pattern) {
    $script:Count++
    try { & $Body | Out-Null; $script:Failures++; Write-Host "FAIL $Name (no error)" -ForegroundColor Red }
    catch {
        if ($_.Exception.Message -match $Pattern) { Write-Host "ok   $Name" }
        else { $script:Failures++; Write-Host "FAIL $Name`n   error: $($_.Exception.Message)" -ForegroundColor Red }
    }
}

$Temp = Join-Path ([IO.Path]::GetTempPath()) ("ragon-node-test-" + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $Temp | Out-Null
try {
    $Log = Join-Path $Temp "calls.log"
    $env:RAGON_FAKE_LOG = $Log
    function New-Fake([string]$Name, [string]$ExitVar) {
        $Path = Join-Path $Temp "$Name.cmd"
        Set-Content -Path $Path -Encoding ASCII -Value @("@echo off", "echo $Name %* >> `"%RAGON_FAKE_LOG%`"", "if defined $ExitVar exit /b %$ExitVar%", "exit /b 0")
        return $Path
    }
    $Tools = @{ Sc = (New-Fake "sc" "FAKE_SC_EXIT"); Icacls = (New-Fake "icacls" "FAKE_ICACLS_EXIT"); Ragon = (New-Fake "ragon" "FAKE_RAGON_EXIT") }
    $Calls = New-Object Collections.ArrayList
    $Users = New-Object Collections.ArrayList
    [void]$Users.Add("CodexSandboxOffline"); [void]$Users.Add("CodexSandboxOnline")
    $Hooks = @{
        UserExists = { param($Name) $Users -contains $Name }.GetNewClosure()
        NewUser = { param($Name) [void]$Users.Add($Name); [void]$Calls.Add("newuser $Name") }.GetNewClosure()
        RemoveUser = { param($Name) $Users.Remove($Name); [void]$Calls.Add("removeuser $Name") }.GetNewClosure()
    }
    function Read-Log { if (Test-Path $Log) { @(Get-Content $Log) } else { @() } }
    function Reset-Log { Remove-Item -Force $Log -ErrorAction SilentlyContinue; $Calls.Clear() }

    # A client binary set and an engine zip.
    $Client = Join-Path $Temp "client"
    New-Item -ItemType Directory -Path $Client | Out-Null
    Set-Content -Path (Join-Path $Client "ragon.exe") -Value "fake ragon"
    Set-Content -Path (Join-Path $Client "codex-code-mode-host.exe") -Value "fake host"
    $Binaries = @((Join-Path $Client "ragon.exe"), (Join-Path $Client "codex-code-mode-host.exe"))
    $EngineDir = Join-Path $Temp "enginesrc"
    New-Item -ItemType Directory -Path (Join-Path $EngineDir "kernels") | Out-Null
    Set-Content -Path (Join-Path $EngineDir "ragon-serve.exe") -Value "fake engine"
    Set-Content -Path (Join-Path $EngineDir "kernels\a.co") -Value "code object"
    $Zip = Join-Path $Temp "engine.zip"
    [IO.Compression.ZipFile]::CreateFromDirectory($EngineDir, $Zip)
    $EmptySrc = Join-Path $Temp "emptysrc"; New-Item -ItemType Directory -Path $EmptySrc | Out-Null
    Set-Content -Path (Join-Path $EmptySrc "readme.txt") -Value "no engine"
    $BadZip = Join-Path $Temp "noengine.zip"
    [IO.Compression.ZipFile]::CreateFromDirectory($EmptySrc, $BadZip)
    $Slip = Join-Path $Temp "slip.zip"
    $Stream = [IO.File]::Create($Slip)
    $Archive = New-Object IO.Compression.ZipArchive($Stream, [IO.Compression.ZipArchiveMode]::Create)
    $Entry = $Archive.CreateEntry("..\..\evil.txt"); $Writer = New-Object IO.StreamWriter($Entry.Open()); $Writer.Write("x"); $Writer.Dispose()
    $Archive.Dispose(); $Stream.Dispose()

    function New-Prefix([string]$Name) {
        $Base = Join-Path $Temp $Name
        return Get-NodePaths (Join-Path $Base "pf") (Join-Path $Base "pd")
    }
    $Sshd = Join-Path $Temp "sshd_config"
    $SshdOriginal = "Port 22`r`nSubsystem sftp sftp-server.exe`r`n"
    [IO.File]::WriteAllText($Sshd, $SshdOriginal)

    # --- the table ---
    $Paths = New-Prefix "acl"
    $Acl = Get-NodeAclCommands $Paths @("CodexSandboxOffline", "CodexSandboxOnline") 'NT SERVICE\ragon-node' "RagonJob"
    Check "five ACL commands" $Acl.Count 5
    Check "root entries" ($Acl[0] -join " ") ("$($Paths.State) /inheritance:r /grant:r *S-1-5-18:(OI)(CI)(F) /grant:r *S-1-5-32-544:(OI)(CI)(F) /grant:r NT SERVICE\ragon-node:(OI)(CI)(M) /grant:r RagonJob:(OI)(CI)(RX)")
    Check "node id is a file: no inheritance flags, sandbox may read" ($Acl[2] -join " ") ("$($Paths.NodeId) /inheritance:r /grant:r *S-1-5-18:(F) /grant:r *S-1-5-32-544:(F) /grant:r NT SERVICE\ragon-node:(M) /grant:r RagonJob:(R) /grant:r CodexSandboxOffline:(R) /grant:r CodexSandboxOnline:(R)")
    Check "jobs: the job and both sandbox accounts modify" (($Acl[4] -join " ") -match "RagonJob:\(OI\)\(CI\)\(M\).*CodexSandboxOffline:\(OI\)\(CI\)\(M\).*CodexSandboxOnline:\(OI\)\(CI\)\(M\)") $true
    Check "models: the sandbox accounts never appear" (($Acl[1] -join " ") -match "Sandbox") $false

    # --- the host check ---
    Check "elevated Windows 11 x64 has no problem" (Test-NodeHost 26100 "x86_64" $true).Count 0
    Check "three problems found at once" (Test-NodeHost 19045 "aarch64" $false).Count 3

    # --- the sshd block ---
    Check "a block is written" (Set-NodeSshdBlock $Sshd "RagonJob") $true
    $Written = [IO.File]::ReadAllText($Sshd)
    Check "block names the job user" ($Written -match "Match User RagonJob") $true
    Check "block forbids tty and forwarding" (($Written -match "PermitTTY no") -and ($Written -match "AllowTcpForwarding no")) $true
    Set-NodeSshdBlock $Sshd "RagonJob" | Out-Null
    Check "a second write leaves one block" ([regex]::Matches([IO.File]::ReadAllText($Sshd), "BEGIN ragon-node").Count) 1
    Remove-NodeSshdBlock $Sshd
    Check "removing the block restores the file" ([IO.File]::ReadAllText($Sshd)) $SshdOriginal
    Check "a missing config is skipped" (Set-NodeSshdBlock (Join-Path $Temp "none") "RagonJob") $false

    # --- the zip ---
    Check "a zip with an escaping entry is unsafe" (Test-ZipEntriesSafe $Slip (Join-Path $Temp "dest")) $false
    Check "an ordinary zip is safe" (Test-ZipEntriesSafe $Zip (Join-Path $Temp "dest")) $true

    # --- install ---
    $P = New-Prefix "ok"
    Reset-Log
    $Healthy = Install-RagonNode $P "v0.9.10" $Binaries $Zip $Tools $Hooks @() @("--model", "D:\m") $Sshd 8000 5
    Check "install reports healthy" $Healthy $true
    Check "ragon.exe is placed under the program folder" (Test-Path (Join-Path $P.Bin "ragon.exe")) $true
    Check "the engine is unpacked beside it" (Test-Path (Join-Path $P.Engine "kernels\a.co")) $true
    Check "the job account was made" ($Calls -contains "newuser RagonJob") $true
    $Lines = Read-Log
    Check "the service is made by ragon server install" ($Lines[0] -match "^ragon server install --name ragon-node --state-dir .*node --host 0.0.0.0 --port 8000 -- .*ragon-serve.exe --model D:.m") $true
    Check "icacls ran for the root, models, advert, jobs (not the absent id file)" (@($Lines | Where-Object { $_ -like "icacls*" }).Count) 4
    Check "the service was started" (@($Lines | Where-Object { $_ -eq "sc start ragon-node " }).Count -ge 1) $true
    Check "the service's start comes after its ACLs" ([array]::IndexOf($Lines, ($Lines | Where-Object { $_ -like "sc start*" } | Select-Object -First 1)) -gt [array]::IndexOf($Lines, ($Lines | Where-Object { $_ -like "icacls*" } | Select-Object -Last 1))) $true
    Check "the record names what was created" ((Get-Content $P.Record) -join ";") "tag=v0.9.10;service=ragon-node;created=user:RagonJob,service:ragon-node;port=8000;state=$($P.NodeState);sshd=$Sshd"
    Check "the sshd block is in the config" ([IO.File]::ReadAllText($Sshd).Contains("BEGIN ragon-node")) $true
    Throws "a second install is refused" { Install-RagonNode $P "v0.9.10" $Binaries $Zip $Tools $Hooks @() @() "" 8000 1 } "already installed"

    # --- uninstall leaves nothing ---
    Set-Content -Path (Join-Path $P.Models "weights.bin") -Value "w"
    Reset-Log
    $Gone = Uninstall-RagonNode $P $Tools $Hooks $Sshd $false
    Check "uninstall removed the service, the files and the user" ($Gone -join ",") "service,distribution,user"
    Check "the program folder is gone" (Test-Path $P.Root) $false
    Check "the node state is gone" (Test-Path $P.NodeState) $false
    Check "weights are kept without -RemoveData" (Test-Path (Join-Path $P.Models "weights.bin")) $true
    Check "the sshd block is gone" ([IO.File]::ReadAllText($Sshd)) $SshdOriginal
    Check "the supervisor's uninstall ran" (@(Read-Log | Where-Object { $_ -like "ragon server uninstall --name ragon-node*" }).Count) 1
    Check "the job account was removed" ($Users -contains "RagonJob") $false
    Uninstall-RagonNode $P $Tools $Hooks $Sshd $true | Out-Null
    Check "-RemoveData removes the state folder" (Test-Path $P.State) $false

    # --- a failed install cleans up after itself ---
    $P2 = New-Prefix "bad"
    Reset-Log
    Throws "a zip without the engine fails" { Install-RagonNode $P2 "v0.9.10" $Binaries $BadZip $Tools $Hooks @() @() "" 8000 1 } "no ragon-serve.exe"
    Check "nothing is left after it" (Test-Path $P2.Root) $false
    Check "the job account made for it is removed" ($Users -contains "RagonJob") $false
    $P3 = New-Prefix "slip"
    Throws "a zip with an escaping entry fails" { Install-RagonNode $P3 "v0.9.10" $Binaries $Slip $Tools $Hooks @() @() "" 8000 1 } "outside its folder"
    Check "nothing is left after the slip" (Test-Path $P3.Root) $false
    $env:FAKE_RAGON_EXIT = "5"
    $P4 = New-Prefix "failinstall"
    Throws "a failing server install is reported with its exit code" { Install-RagonNode $P4 "v0.9.10" $Binaries $Zip $Tools $Hooks @() @() "" 8000 1 } "failed \(5\)"
    Check "and leaves nothing" (Test-Path $P4.Root) $false
    Remove-Item Env:\FAKE_RAGON_EXIT
    $env:FAKE_ICACLS_EXIT = "5"
    $P5 = New-Prefix "failacl"
    Throws "a failing icacls is reported" { Install-RagonNode $P5 "v0.9.10" $Binaries $Zip $Tools $Hooks @() @() "" 8000 1 } "icacls"
    Check "and leaves nothing behind either" (Test-Path $P5.Root) $false
    Remove-Item Env:\FAKE_ICACLS_EXIT
} finally {
    Remove-Item Env:\RAGON_FAKE_LOG -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force -Path $Temp -ErrorAction SilentlyContinue
}
Write-Host "$($Count - $Failures) of $Count checks passed"
if ($Failures) { exit 1 }
