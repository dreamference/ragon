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
    if ($Statement -is [Management.Automation.Language.FunctionDefinitionAst] -and $Statement.Name -match '^(Get-Node|Test-Zip|Expand-Node|Invoke-Node|Set-Node|Remove-Node|Test-Node|Find-Node|Save-Node|New-Node|Install-RagonNode|Uninstall-RagonNode)') {
        $Pieces += $Statement.Extent.Text
    } elseif ($Statement -is [Management.Automation.Language.AssignmentStatementAst] -and $Statement.Left.Extent.Text -match '^\$Node(Service|Job|Sandbox|Sshd|Trusted)') {
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
        Set-Content -Path $Path -Encoding ASCII -Value @("@echo off", "echo $Name %* >> `"%RAGON_FAKE_LOG%`"", "if defined FAKE_STDERR echo noise 1>&2", "if `"%2`"==`"--help`" if not defined FAKE_NO_ADVERTISE echo usage server --advertise NODE_ID", "if `"%2`"==`"--help`" if not defined FAKE_NO_JOBS echo usage server [--jobs-dir D]", "if defined $ExitVar exit /b %$ExitVar%", "exit /b 0")
        return $Path
    }
    $Tools = @{ Sc = (New-Fake "sc" "FAKE_SC_EXIT"); Icacls = (New-Fake "icacls" "FAKE_ICACLS_EXIT"); Ragon = (New-Fake "ragon" "FAKE_RAGON_EXIT"); Sshd = (New-Fake "sshd" "FAKE_SSHD_EXIT") }
    $Calls = New-Object Collections.ArrayList
    $Foreign = New-Object Collections.ArrayList
    $Users = New-Object Collections.ArrayList
    [void]$Users.Add("CodexSandboxOffline"); [void]$Users.Add("CodexSandboxOnline")
    $Hooks = @{
        UserExists = { param($Name) $Users -contains $Name }.GetNewClosure()
        NewUser = { param($Name) [void]$Users.Add($Name); [void]$Calls.Add("newuser $Name") }.GetNewClosure()
        RemoveUser = { param($Name) $Users.Remove($Name); [void]$Calls.Add("removeuser $Name") }.GetNewClosure()
        AddPath = { param($Dir) [void]$Calls.Add("addpath $Dir") }.GetNewClosure()
        RemovePath = { param($Dir) [void]$Calls.Add("removepath $Dir") }.GetNewClosure()
        RestartSshd = { [void]$Calls.Add("restartsshd") }.GetNewClosure()
        # Everything is owned by Administrators, except paths matching a pattern in $Foreign.
        OwnerSid = { param($Path) if (@($Foreign | Where-Object { $Path -like $_ }).Count) { "S-1-5-21-1-2-3-1001" } else { "S-1-5-32-544" } }.GetNewClosure()
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

    function Get-Sha([string]$File) { (Get-FileHash -Algorithm SHA256 -LiteralPath $File).Hash.ToLower() }
    $BinaryHashes = @{ "ragon.exe" = (Get-Sha $Binaries[0]); "codex-code-mode-host.exe" = (Get-Sha $Binaries[1]) }
    function Expect([string]$ZipFile) { return @{ Engine = (Get-Sha $ZipFile); Binaries = $BinaryHashes } }

    function New-Prefix([string]$Name) {
        $Base = Join-Path $Temp $Name
        return Get-NodePaths (Join-Path $Base "pf") (Join-Path $Base "pd") (Join-Path $Base "link")
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
    Check "a block is written" (Set-NodeSshdBlock $Sshd "RagonJob" $Tools.Sshd) $true
    $Written = [IO.File]::ReadAllText($Sshd)
    Check "block names the job user" ($Written -match "Match User RagonJob") $true
    Check "block gives the account its own key file" ($Written -match "AuthorizedKeysFile .ssh/authorized_keys") $true
    Check "block forbids tty and forwarding" (($Written -match "PermitTTY no") -and ($Written -match "AllowTcpForwarding no") -and ($Written -match "GatewayPorts no")) $true
    Check "block has no ForceCommand (the first pairing login runs ragon node authorize)" ($Written -match "ForceCommand") $false
    Set-NodeSshdBlock $Sshd "RagonJob" $Tools.Sshd | Out-Null
    Check "a second write leaves one block" ([regex]::Matches([IO.File]::ReadAllText($Sshd), "BEGIN ragon-node").Count) 1
    Remove-NodeSshdBlock $Sshd
    Check "removing the block restores the file" ([IO.File]::ReadAllText($Sshd)) $SshdOriginal
    $WithMatch = $SshdOriginal + "Match Group administrators`r`n    AuthorizedKeysFile __PROGRAMDATA__/ssh/administrators_authorized_keys`r`n"
    $SshdM = Join-Path $Temp "sshd_config_match"
    [IO.File]::WriteAllText($SshdM, $WithMatch)
    Set-NodeSshdBlock $SshdM "RagonJob" $Tools.Sshd | Out-Null
    $M = [IO.File]::ReadAllText($SshdM)
    Check "the block goes above the first existing Match block" ($M.IndexOf("Match User RagonJob") -lt $M.IndexOf("Match Group administrators")) $true
    Remove-NodeSshdBlock $SshdM
    Check "and removing it restores that file too" ([IO.File]::ReadAllText($SshdM)) $WithMatch
    Check "a missing config is skipped" (Set-NodeSshdBlock (Join-Path $Temp "none") "RagonJob" $Tools.Sshd) $false
    Reset-Log
    Set-NodeSshdBlock $Sshd "RagonJob" $Tools.Sshd | Out-Null
    Check "the edited config is checked with sshd -t" (@(Read-Log | Where-Object { $_ -eq "sshd -t -f $Sshd " }).Count) 1
    Remove-NodeSshdBlock $Sshd
    $env:FAKE_SSHD_EXIT = "1"
    Throws "a config sshd -t rejects is refused" { Set-NodeSshdBlock $Sshd "RagonJob" $Tools.Sshd } "was not changed: sshd -t rejected it \(1\)"
    Check "and the original bytes are back" ([IO.File]::ReadAllText($Sshd)) $SshdOriginal
    Remove-Item Env:\FAKE_SSHD_EXIT
    Throws "without sshd.exe to check it, the config is not changed" { Set-NodeSshdBlock $Sshd "RagonJob" (Join-Path $Temp "no-sshd.exe") } "not found"
    Check "and stays as it was" ([IO.File]::ReadAllText($Sshd)) $SshdOriginal

    # --- the zip ---
    Check "a zip with an escaping entry is unsafe" (Test-ZipEntriesSafe $Slip (Join-Path $Temp "dest")) $false
    Check "an ordinary zip is safe" (Test-ZipEntriesSafe $Zip (Join-Path $Temp "dest")) $true
    Check "the verify step finds the engine in the zip" (Test-ZipHasEngine $Zip) $true
    Check "and refuses a zip without it before anything is installed" (Test-ZipHasEngine $BadZip) $false

    # --- owners ---
    Check "Administrators, SYSTEM and service SIDs are trusted owners" ((Test-NodeOwnerTrusted "S-1-5-32-544" @()) -and (Test-NodeOwnerTrusted "S-1-5-18" @()) -and (Test-NodeOwnerTrusted "S-1-5-80-1-2-3" @())) $true
    Check "a user is not, unless passed in" ((Test-NodeOwnerTrusted "S-1-5-21-1-2-3-1001" @()), (Test-NodeOwnerTrusted "S-1-5-21-1-2-3-1001" @("S-1-5-21-1-2-3-1001"))) "False True"

    # --- install ---
    $P = New-Prefix "ok"
    Reset-Log
    $Healthy = Install-RagonNode $P "v0.9.10" $Binaries $Zip $Tools $Hooks @() @("--model", "D:\m") $Sshd 8000 5 (Expect $Zip)
    Check "install reports healthy" $Healthy $true
    Check "ragon.exe is placed under the program folder" (Test-Path (Join-Path $P.Bin "ragon.exe")) $true
    Check "the engine is unpacked beside it" (Test-Path (Join-Path $P.Engine "kernels\a.co")) $true
    Check "the job account was made" ($Calls -contains "newuser RagonJob") $true
    $Lines = Read-Log
    $InstallLine = $Lines | Where-Object { $_ -like "ragon server install*" } | Select-Object -First 1
    Check "the service is made by ragon server install" ($InstallLine -match "^ragon server install --name ragon-node --state-dir .*node --host 127.0.0.1 --port 8000 --advertise [0-9a-f-]{36} --jobs-dir .*\\jobs -- .*ragon-serve.exe --model D:.m") $true
    Check "a node id was written, one line" ((Get-Content $P.NodeId).Count) 1
    Check "the junction gives the job account a space-free ragon.exe" (Test-Path (Join-Path $P.LinkBin "ragon.exe")) $true
    Check "and its path is on the machine PATH" ($Calls -contains "addpath $($P.LinkBin)") $true
    Check "sshd was restarted for the block" ($Calls -contains "restartsshd") $true
    Check "icacls took over the link folder and the state folder (owner, reset, ACL) and set the five state items" (@($Lines | Where-Object { $_ -like "icacls*" }).Count) 11
    Check "the state folder's owner is set before anything is written in it" (@($Lines | Where-Object { $_ -like "icacls*" })[0]) "icacls $($P.State) /setowner *S-1-5-32-544 /T /Q "
    Check "and its ACL is reset, then closed" ((@($Lines | Where-Object { $_ -like "icacls*" })[1..2]) -join "|") "icacls $($P.State) /reset /T /Q |icacls $($P.State) /inheritance:r /grant:r *S-1-5-18:(OI)(CI)(F) /grant:r *S-1-5-32-544:(OI)(CI)(F) "
    Check "the service was started" (@($Lines | Where-Object { $_ -eq "sc start ragon-node " }).Count -ge 1) $true
    Check "the service's start comes after its ACLs" ([array]::IndexOf($Lines, ($Lines | Where-Object { $_ -like "sc start*" } | Select-Object -First 1)) -gt [array]::IndexOf($Lines, ($Lines | Where-Object { $_ -like "icacls*" } | Select-Object -Last 1))) $true
    Check "the record names what was created" ((Get-Content $P.Record) -join ";") "tag=v0.9.10;service=ragon-node;created=user:RagonJob,path:$($P.LinkBin),service:ragon-node;port=8000;state=$($P.NodeState);link=$($P.LinkBin);sshd=$Sshd;jobs=$($P.Jobs)"
    Check "the sshd block is in the config" ([IO.File]::ReadAllText($Sshd).Contains("BEGIN ragon-node")) $true
    Throws "a second install is refused" { Install-RagonNode $P "v0.9.10" $Binaries $Zip $Tools $Hooks @() @() "" 8000 1 (Expect $Zip) } "already installed"

    # --- a tool that writes to stderr is not an error (Windows PowerShell 5.1 under Stop) ---
    $env:FAKE_STDERR = "1"
    $PS = New-Prefix "stderr"
    Check "install succeeds when the tools print to stderr" (Install-RagonNode $PS "v0.9.10" $Binaries $Zip $Tools $Hooks @() @() "" 8000 5 (Expect $Zip)) $true
    Check "and so does uninstall" ((Uninstall-RagonNode $PS $Tools $Hooks "" $true) -join ",") "service,distribution,data"
    Remove-Item Env:\FAKE_STDERR
    Reset-Log

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
    Check "the junction and its folder are gone, the program folder with it" ((Test-Path $P.LinkRoot) -or (Test-Path $P.Bin)) $false
    Check "the PATH entry was removed" ($Calls -contains "removepath $($P.LinkBin)") $true
    Uninstall-RagonNode $P $Tools $Hooks $Sshd $true | Out-Null
    Check "-RemoveData removes the state folder" (Test-Path $P.State) $false

    # --- a supervisor without --advertise is installed without it ---
    $env:FAKE_NO_ADVERTISE = "1"
    $PA = New-Prefix "noadv"
    Reset-Log
    Install-RagonNode $PA "v0.9.10" $Binaries $Zip $Tools $Hooks @() @() "" 8000 5 (Expect $Zip) | Out-Null
    Check "no --advertise is passed to one that does not know it" ((Read-Log | Where-Object { $_ -like "ragon server install*" }) -match "--advertise") $false
    Uninstall-RagonNode $PA $Tools $Hooks "" $true | Out-Null
    Remove-Item Env:\FAKE_NO_ADVERTISE
    Reset-Log

    # --- a supervisor without node jobs gets no --jobs-dir and no jobs= in the record ---
    $env:FAKE_NO_JOBS = "1"
    $PJ = New-Prefix "nojobs"
    Install-RagonNode $PJ "v0.9.10" $Binaries $Zip $Tools $Hooks @() @() "" 8000 5 (Expect $Zip) | Out-Null
    Check "no --jobs-dir for one without node jobs" ((Read-Log | Where-Object { $_ -like "ragon server install*" }) -match "--jobs-dir") $false
    Check "and the record has no jobs= line" ((Get-Content $PJ.Record) -match "^jobs=").Count 0
    Uninstall-RagonNode $PJ $Tools $Hooks "" $true | Out-Null
    Remove-Item Env:\FAKE_NO_JOBS
    Reset-Log

    # --- folders that exist before the install are adopted only when an administrator owns them ---
    $PF = New-Prefix "foreign"
    New-Item -ItemType Directory -Force -Path $PF.Models | Out-Null
    Set-Content -Path (Join-Path $PF.Models "planted.bin") -Value "x"
    [void]$Foreign.Add("*planted*")
    Throws "a file in the state folder owned by a user is refused" { Install-RagonNode $PF "v0.9.10" $Binaries $Zip $Tools $Hooks @() @() "" 8000 1 (Expect $Zip) } "owned by S-1-5-21-1-2-3-1001.*Nothing was changed"
    Check "before anything was changed" ((Test-Path $PF.Root), @(Read-Log).Count, $Calls.Count) "False 0 0"
    Check "the installing user may own them" (Test-NodePreflight $PF $Hooks @("S-1-5-21-1-2-3-1001")).Count 0
    $Foreign.Clear()
    [void]$Foreign.Add("*\link")
    New-Item -ItemType Directory -Force -Path $PF.LinkRoot | Out-Null
    Throws "a link folder (C:\Ragon) a user made is refused" { Install-RagonNode $PF "v0.9.10" $Binaries $Zip $Tools $Hooks @() @() "" 8000 1 (Expect $Zip) } "link is owned by"
    $Foreign.Clear()
    $Outside = Join-Path $Temp "outside"; New-Item -ItemType Directory -Force -Path $Outside | Out-Null
    New-Item -ItemType Junction -Path (Join-Path $PF.Models "sneaky") -Target $Outside | Out-Null
    Throws "a junction in the state folder is refused" { Install-RagonNode $PF "v0.9.10" $Binaries $Zip $Tools $Hooks @() @() "" 8000 1 (Expect $Zip) } "sneaky is a link"
    [IO.Directory]::Delete((Join-Path $PF.Models "sneaky"))
    New-Item -ItemType Directory -Force -Path $PF.LinkBin | Out-Null
    Check "a plain folder where the junction goes is refused" ((Test-NodePreflight $PF $Hooks @()) -join " ") "$($PF.LinkBin) is a folder, not the node's junction; remove it first."
    Remove-Item -Force $PF.LinkBin
    Reset-Log
    Check "an administrator's folders are adopted" (Install-RagonNode $PF "v0.9.10" $Binaries $Zip $Tools $Hooks @() @() "" 8000 5 (Expect $Zip)) $true
    $Acls = @(Read-Log | Where-Object { $_ -like "icacls*" })
    Check "taken over: owner and reset on the whole link folder" (($Acls -contains "icacls $($PF.LinkRoot) /setowner *S-1-5-32-544 /T /Q ") -and ($Acls -contains "icacls $($PF.LinkRoot) /reset /T /Q ")) $true
    Check "and on the whole state folder" (($Acls -contains "icacls $($PF.State) /setowner *S-1-5-32-544 /T /Q ") -and ($Acls -contains "icacls $($PF.State) /reset /T /Q ")) $true
    Reset-Log
    Uninstall-RagonNode $PF $Tools $Hooks "" $false | Out-Null
    Check "kept data passes to Administrators before the job account goes" ((Read-Log) -contains "icacls $($PF.State) /setowner *S-1-5-32-544 /T /Q ") $true

    # --- a failed install over kept data leaves that data ---
    $env:FAKE_RAGON_EXIT = "5"
    Throws "the install fails" { Install-RagonNode $PF "v0.9.10" $Binaries $Zip $Tools $Hooks @() @() "" 8000 1 (Expect $Zip) } "failed \(5\)"
    Remove-Item Env:\FAKE_RAGON_EXIT
    Check "the state folder it did not make, and the weights in it, are kept" (Test-Path (Join-Path $PF.Models "planted.bin")) $true
    Check "the program folder is gone" (Test-Path $PF.Root) $false
    Uninstall-RagonNode $PF $Tools $Hooks "" $true | Out-Null
    Reset-Log

    # --- the hashes are checked again where users cannot write ---
    $PH = New-Prefix "hash"
    $Wrong = @{ Engine = (Get-Sha $Zip); Binaries = @{ "ragon.exe" = "00"; "codex-code-mode-host.exe" = $BinaryHashes["codex-code-mode-host.exe"] } }
    Throws "a client binary that changed after its check is refused" { Install-RagonNode $PH "v0.9.10" $Binaries $Zip $Tools $Hooks @() @() "" 8000 1 $Wrong } "ragon.exe changed after it was checked"
    Check "and nothing is left" (Test-Path $PH.Root) $false
    Throws "an engine zip that changed after its check is refused" { Install-RagonNode $PH "v0.9.10" $Binaries $Zip $Tools $Hooks @() @() "" 8000 1 (Expect $BadZip) } "engine zip changed"
    Check "and nothing is left either" (Test-Path $PH.Root) $false
    Reset-Log

    # --- removing a tree does not follow a junction out of it ---
    $PR = New-Prefix "junction"
    Install-RagonNode $PR "v0.9.10" $Binaries $Zip $Tools $Hooks @() @() "" 8000 5 (Expect $Zip) | Out-Null
    $Precious = Join-Path $Temp "precious"; New-Item -ItemType Directory -Force -Path $Precious | Out-Null
    Set-Content -Path (Join-Path $Precious "keep.txt") -Value "keep"
    New-Item -ItemType Junction -Path (Join-Path $PR.NodeState "out") -Target $Precious | Out-Null
    New-Item -ItemType Junction -Path (Join-Path $PR.Models "out") -Target $Precious | Out-Null
    New-Item -ItemType Junction -Path (Join-Path $PR.Bin "out") -Target $Precious | Out-Null
    Uninstall-RagonNode $PR $Tools $Hooks "" $false | Out-Null
    Uninstall-RagonNode $PR $Tools $Hooks "" $true | Out-Null
    Check "the state and program folders are gone" ((Test-Path $PR.State) -or (Test-Path $PR.Root)) $false
    Check "what their junctions pointed to is not" (Test-Path (Join-Path $Precious "keep.txt")) $true
    Reset-Log

    # --- a failed install cleans up after itself ---
    $P2 = New-Prefix "bad"
    Reset-Log
    Throws "a zip without the engine fails" { Install-RagonNode $P2 "v0.9.10" $Binaries $BadZip $Tools $Hooks @() @() "" 8000 1 (Expect $BadZip) } "no ragon-serve.exe"
    Check "nothing is left after it" (Test-Path $P2.Root) $false
    Check "the job account made for it is removed" ($Users -contains "RagonJob") $false
    $P3 = New-Prefix "slip"
    Throws "a zip with an escaping entry fails" { Install-RagonNode $P3 "v0.9.10" $Binaries $Slip $Tools $Hooks @() @() "" 8000 1 (Expect $Slip) } "outside its folder"
    Check "nothing is left after the slip" (Test-Path $P3.Root) $false
    $env:FAKE_RAGON_EXIT = "5"
    $P4 = New-Prefix "failinstall"
    Throws "a failing server install is reported with its exit code" { Install-RagonNode $P4 "v0.9.10" $Binaries $Zip $Tools $Hooks @() @() "" 8000 1 (Expect $Zip) } "failed \(5\)"
    Check "and leaves nothing" (Test-Path $P4.Root) $false
    Remove-Item Env:\FAKE_RAGON_EXIT
    $env:FAKE_ICACLS_EXIT = "5"
    $P5 = New-Prefix "failacl"
    Throws "a failing icacls is reported" { Install-RagonNode $P5 "v0.9.10" $Binaries $Zip $Tools $Hooks @() @() "" 8000 1 (Expect $Zip) } "icacls"
    Check "and leaves nothing behind either" (Test-Path $P5.Root) $false
    Remove-Item Env:\FAKE_ICACLS_EXIT
} finally {
    Remove-Item Env:\RAGON_FAKE_LOG -ErrorAction SilentlyContinue
    Remove-Item -Recurse -Force -Path $Temp -ErrorAction SilentlyContinue
}
Write-Host "$($Count - $Failures) of $Count checks passed"
if ($Failures) { exit 1 }
