<#
Installs the Ragon client on Windows from a release, with nothing compiled.

    irm https://github.com/dreamference/ragon/releases/latest/download/install.ps1 | iex

or, to pick a release:

    & ([scriptblock]::Create((irm https://github.com/dreamference/ragon/releases/latest/download/install.ps1))) -Version 0.9.0

It installs `ragon.exe` and its commands (ragon-search, ragon-fetch, ragon-code, and the
sandbox's helpers when the release carries them) into %LOCALAPPDATA%\Programs\Ragon\bin and puts
that folder on your PATH, with no administrator rights. Then Windows asks once for them, to set up
the sandbox the agent's commands run in (which /airgapped on needs), to let ragon find nodes on
Private networks, and to run the egress audit; -NoSandbox skips that, and declining leaves a working
client without a sandbox. The model runs on a Ragon node on your network (a DGX Spark or another
GB10): `ragon` finds it by itself.

It also installs the desktop app, from the release's Ragon-Setup-<version>-<arm64|x64>.exe
(checked against SHA256SUMS), silently and for you alone, under
%LOCALAPPDATA%\Programs\Ragon\App with a Start menu entry; -NoApp leaves it out. Downloaded
by this script rather than a browser, the installer carries no Mark of the Web, so SmartScreen
does not stop it.

It downloads the same assets, by the same names and with the same checks, as `ragon update`
(ragon-rs/src/update.rs), so a machine installed this way is updated by that command, the desktop
app included: the script records the app it installed (desktop-app, beside the bin folder), and
`ragon update` updates an app it finds recorded, under the same checks.

PREVIEW. Windows builds are not signed yet. Smart App Control, which is on in new Windows 11
installs, blocks unsigned programs outright: it must be off to run Ragon (Windows Security,
App & browser control, Smart App Control settings).

Written for Windows PowerShell 5.1, which every Windows 11 machine has, as well as PowerShell 7,
and kept to ASCII so 5.1 reads it the same whatever the code page
(specs/DREAMFERENCE_RAGON_WINDOWS_ARM.md section 16.4).

RAGON_RELEASE_REPO names another repository (a fork), RAGON_RELEASE_API another API root (a
test's stand-in server), RAGON_INSTALL_DIR another install folder. GH_TOKEN or GITHUB_TOKEN, when
set, raises GitHub's rate limit and reads a private fork.
#>
param(
    [string]$Version = "",
    [switch]$NoPath,
    [switch]$NoSandbox,
    [switch]$NoApp,
    [ValidateSet("client", "node")][string]$Role = "client",
    [switch]$Uninstall,
    [switch]$RemoveData,
    [switch]$NoSshd,
    [int]$Port = 8000,
    [string[]]$EngineArgs = @()
)

$ErrorActionPreference = "Stop"
# Invoke-WebRequest's progress bar slows 5.1's downloads many times over.
$ProgressPreference = "SilentlyContinue"
# .NET Framework's defaults predate TLS 1.2 on some images; GitHub requires it.
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

function Say([string]$Text) { Write-Host $Text }
function Fail([string]$Text) { Write-Host "ERROR: $Text" -ForegroundColor Red; throw $Text }

# -- the node role: functions (specs/DREAMFERENCE_WINDOWS_NODE.md section 6) ---------------------
#
# Everything below takes its folders, tools and hooks as parameters, so tests/windows/
# install-node.Tests.ps1 loads these functions alone (by syntax tree, the script is not run) and
# drives them against a temp prefix with a fake sc.exe, icacls.exe and ragon.exe and hooks that
# record instead of touching accounts, services or the firewall.
#   %ProgramFiles%\Ragon (read-only to users): current\bin, current\engine, previous, staging
#   %ProgramData%\Ragon (ACLs by the table of ragon-node-locator's state module): node, models,
#   advert, jobs, node-id

$NodeServiceName = "ragon-node"
$NodeServiceAccount = "NT SERVICE\ragon-node"
$NodeJobAccount = "RagonJob"
$NodeSandboxAccounts = @("CodexSandboxOffline", "CodexSandboxOnline")
$NodeSshdBegin = "# BEGIN ragon-node (written by install.ps1 -Role node)"
$NodeSshdEnd = "# END ragon-node"
# Owners a folder the node adopts may have: Administrators, SYSTEM, TrustedInstaller (service SIDs,
# S-1-5-80-*, are matched by prefix), plus the installing administrator, passed in.
$NodeTrustedOwners = @("S-1-5-32-544", "S-1-5-18", "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464")

function Get-NodePaths([string]$ProgramFiles, [string]$ProgramData, [string]$LinkRoot = "") {
    $Root = Join-Path $ProgramFiles "Ragon"
    $State = Join-Path $ProgramData "Ragon"
    return @{
        Root = $Root; Current = (Join-Path $Root "current"); Previous = (Join-Path $Root "previous")
        Staging = (Join-Path $Root "staging"); Bin = (Join-Path $Root "current\bin")
        Engine = (Join-Path $Root "current\engine"); Record = (Join-Path $Root "node-install")
        State = $State; NodeState = (Join-Path $State "node"); Models = (Join-Path $State "models")
        Advert = (Join-Path $State "advert"); Jobs = (Join-Path $State "jobs"); NodeId = (Join-Path $State "node-id")
        LinkRoot = $LinkRoot; LinkBin = $(if ($LinkRoot) { Join-Path $LinkRoot "bin" } else { "" })
    }
}

# The state directory's table (ragon_node_locator::state::table): one row per (item, who, access).
# Who: SYSTEM, ADMINS, SERVICE, JOB, SANDBOX. Kept in step by the test that compares it to the
# Rust source and by the CI step that applies and checks the Rust table.
function Get-NodeAclTable {
    $Rows = @()
    foreach ($Item in @("Root", "Models", "NodeId", "Advert", "Jobs")) {
        $Rows += , @($Item, "SYSTEM", "F")
        $Rows += , @($Item, "ADMINS", "F")
        $Rows += , @($Item, "SERVICE", "M")
    }
    $Rows += , @("Root", "JOB", "RX")
    $Rows += , @("Models", "JOB", "RX")
    $Rows += , @("NodeId", "JOB", "R")
    $Rows += , @("NodeId", "SANDBOX", "R")
    $Rows += , @("Advert", "JOB", "R")
    $Rows += , @("Jobs", "JOB", "M")
    $Rows += , @("Jobs", "SANDBOX", "M")
    return $Rows
}

# The icacls argument lists (one per item) that make the state directory match the table.
function Get-NodeAclCommands($Paths, [string[]]$Sandbox, [string]$Service, [string]$Job) {
    $Where = @{ Root = $Paths.State; Models = $Paths.Models; NodeId = $Paths.NodeId; Advert = $Paths.Advert; Jobs = $Paths.Jobs }
    $Table = Get-NodeAclTable
    $Commands = @()
    foreach ($Item in @("Root", "Models", "NodeId", "Advert", "Jobs")) {
        $Cmd = @($Where[$Item], "/inheritance:r")
        $Inherit = if ($Item -eq "NodeId") { "" } else { "(OI)(CI)" }
        foreach ($Row in $Table) {
            if ($Row[0] -ne $Item) { continue }
            $Names = switch ($Row[1]) {
                "SYSTEM" { @("*S-1-5-18") }
                "ADMINS" { @("*S-1-5-32-544") }
                "SERVICE" { @($Service) }
                "JOB" { @($Job) }
                "SANDBOX" { $Sandbox }
            }
            foreach ($Name in $Names) { $Cmd += "/grant:r"; $Cmd += "${Name}:${Inherit}($($Row[2]))" }
        }
        $Commands += , $Cmd
    }
    return $Commands
}

# A zip's entries must all land under the folder it is unpacked into.
function Test-ZipEntriesSafe([string]$Zip, [string]$Destination) {
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $Base = [IO.Path]::GetFullPath($Destination).TrimEnd('\') + '\'
    $Archive = [IO.Compression.ZipFile]::OpenRead($Zip)
    try {
        foreach ($Entry in $Archive.Entries) {
            $Target = [IO.Path]::GetFullPath((Join-Path $Destination $Entry.FullName))
            if (-not $Target.StartsWith($Base, [StringComparison]::OrdinalIgnoreCase)) { return $false }
        }
    } finally { $Archive.Dispose() }
    return $true
}

# Whether the zip has the engine at its top level (Expand-NodeZip puts it at engine\ragon-serve.exe).
function Test-ZipHasEngine([string]$Zip) {
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $Archive = [IO.Compression.ZipFile]::OpenRead($Zip)
    try { return [bool]($Archive.Entries | Where-Object { $_.FullName -eq "ragon-serve.exe" }) } finally { $Archive.Dispose() }
}

function Expand-NodeZip([string]$Zip, [string]$Destination) {
    if (-not (Test-ZipEntriesSafe $Zip $Destination)) { throw "the engine zip has an entry outside its folder; nothing was installed." }
    New-Item -ItemType Directory -Force -Path $Destination | Out-Null
    [IO.Compression.ZipFile]::ExtractToDirectory($Zip, $Destination)
}

# Runs a native tool and returns its exit code. Windows PowerShell 5.1 turns a native command's
# stderr into a terminating error under $ErrorActionPreference = Stop (and 2>&1 does not help), so
# the call runs in a child scope that continues; callers read the exit code.
function Invoke-NodeNative([string]$Tool, [string[]]$Arguments) {
    $Lines = & {
        $ErrorActionPreference = "Continue"
        & $Tool @Arguments 2>&1 | ForEach-Object { "$_" }
    }
    $script:NodeNativeOutput = @($Lines)
    return $LASTEXITCODE
}

# Runs a tool and fails with its output when it exits non-zero.
function Invoke-NodeTool([string]$Tool, [string[]]$Arguments) {
    $Code = Invoke-NodeNative $Tool $Arguments
    if ($Code -ne 0) { throw "$Tool $($Arguments -join ' ') failed ($Code): $($script:NodeNativeOutput -join ' ')" }
    return $script:NodeNativeOutput
}

# The ssh daemon's block for the job account (the text nw-pairing hands over): its own key file,
# no forwarding, no tty. There is no ForceCommand: the first pairing login runs `ragon node id`
# and `ragon node authorize`, after which the key's own command= forces serve-job. It goes above
# the first existing Match block (Match blocks apply in order), or at the end when there is none.
# Idempotent; replaced between its markers, nothing else in the file is touched.
# The edited file is checked with `sshd -t -f` ($Sshd, the daemon's executable) and the original
# bytes are put back when the check fails or sshd cannot be found, so a bad edit never reaches a
# restart.
function Set-NodeSshdBlock([string]$Config, [string]$User, [string]$Sshd) {
    if (-not (Test-Path $Config)) { return $false }
    $Original = [IO.File]::ReadAllBytes($Config)
    $Text = Remove-NodeSshdText ([IO.File]::ReadAllText($Config))
    $Block = (@($NodeSshdBegin, "Match User $User", "    AuthorizedKeysFile .ssh/authorized_keys",
        "    AllowTcpForwarding no", "    AllowStreamLocalForwarding no", "    GatewayPorts no", "    PermitTunnel no",
        "    X11Forwarding no", "    AllowAgentForwarding no", "    PermitTTY no", $NodeSshdEnd) -join "`r`n") + "`r`n"
    $First = [regex]::Match($Text, "(?im)^\s*Match\s")
    if ($First.Success) {
        $Text = $Text.Substring(0, $First.Index) + $Block + $Text.Substring($First.Index)
    } else {
        if (-not $Text.EndsWith("`n")) { $Text += "`r`n" }
        $Text += $Block
    }
    [IO.File]::WriteAllText($Config, $Text)
    $Why = if (-not $Sshd -or -not (Test-Path $Sshd)) { "sshd.exe was not found to check it" } else {
        $Code = Invoke-NodeNative $Sshd @("-t", "-f", $Config)
        if ($Code -ne 0) { "sshd -t rejected it ($Code): $($script:NodeNativeOutput -join ' ')" } else { "" }
    }
    if ($Why) {
        [IO.File]::WriteAllBytes($Config, $Original)
        throw "the ssh daemon's configuration $Config was not changed: $Why."
    }
    return $true
}

function Remove-NodeSshdText([string]$Text) {
    return [regex]::Replace($Text, "(?s)" + [regex]::Escape($NodeSshdBegin) + ".*?" + [regex]::Escape($NodeSshdEnd) + "\r?\n?", "")
}

function Remove-NodeSshdBlock([string]$Config) {
    if (-not (Test-Path $Config)) { return }
    $Text = [IO.File]::ReadAllText($Config)
    if ($Text.Contains($NodeSshdBegin)) { [IO.File]::WriteAllText($Config, (Remove-NodeSshdText $Text)) }
}

# The machine PATH, edited as the registry value itself, unexpanded and of the kind it was (the .NET
# environment calls expand %SystemRoot% and the like on read and would freeze them, as for the
# user PATH below), so the job account, which has no profile, finds ragon.
function Edit-NodeMachinePath([string]$Dir, [bool]$Add) {
    $Key = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey("SYSTEM\CurrentControlSet\Control\Session Manager\Environment", $true)
    try {
        $Now = [string]$Key.GetValue("Path", "", [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
        $Kind = if ($Key.GetValueNames() -contains "Path") { $Key.GetValueKind("Path") } else { [Microsoft.Win32.RegistryValueKind]::ExpandString }
        $Entries = @($Now -split ";" | Where-Object { $_ -and $_ -ne $Dir })
        if ($Add) { $Entries += $Dir }
        $Key.SetValue("Path", ($Entries -join ";"), $Kind)
    } finally { $Key.Close() }
    # Setting and clearing a machine variable broadcasts WM_SETTINGCHANGE.
    [Environment]::SetEnvironmentVariable("RAGON_PATH_CHANGED", "1", "Machine")
    [Environment]::SetEnvironmentVariable("RAGON_PATH_CHANGED", $null, "Machine")
}

# Checks the host before anything is changed; returns the list of problems.
function Test-NodeHost([int]$Build, [string]$Arch, [bool]$Elevated) {
    $Problems = @()
    if (-not $Elevated) { $Problems += "this needs an elevated PowerShell (Run as administrator); over SSH there is no UAC, so start the shell elevated." }
    if ($Build -lt 22000) { $Problems += "Windows 11 (build 22000 or later) is required; this is build $Build." }
    if ($Arch -ne "x86_64") { $Problems += "the node's engine is built for x86_64 (gfx1151); this PC is $Arch." }
    return $Problems
}

function Test-NodeOwnerTrusted([string]$Sid, [string[]]$Also) {
    return [bool]($Sid -and ((($NodeTrustedOwners + $Also) -contains $Sid) -or $Sid -like "S-1-5-80-*"))
}

# What under $Path (itself included) is a link (junction or symbolic link) or is owned by someone
# not trusted: such a folder may have been made by a user to keep a say over the node's files (an
# owner can always rewrite the ACL, and a handle opened before the install keeps its rights).
# Links are not followed. $Skip (the node's own junction, replaced by New-NodeLink) is passed over.
function Find-NodeUntrusted([string]$Path, $Hooks, [string[]]$Also, [string]$Skip = "") {
    $Found = @()
    $Stack = New-Object System.Collections.Stack
    $Stack.Push((Get-Item -Force -LiteralPath $Path))
    while ($Stack.Count -gt 0) {
        $Item = $Stack.Pop()
        if ($Skip -and $Item.FullName.TrimEnd('\') -eq $Skip.TrimEnd('\')) { continue }
        if ($Item.Attributes -band [IO.FileAttributes]::ReparsePoint) { $Found += "$($Item.FullName) is a link"; continue }
        $Sid = & $Hooks.OwnerSid $Item.FullName
        if (-not (Test-NodeOwnerTrusted $Sid $Also)) { $Found += "$($Item.FullName) is owned by $Sid" }
        if ($Item -is [IO.DirectoryInfo]) { foreach ($Child in $Item.EnumerateFileSystemInfos()) { $Stack.Push($Child) } }
    }
    return $Found
}

# Checks, before anything is changed, that the node is not installed yet and that every folder it
# would adopt (C:\Ragon, %ProgramData%\Ragon, whatever is in them) is a plain folder owned by an
# administrator or the system. Returns the list of problems.
function Test-NodePreflight($Paths, $Hooks, [string[]]$Also) {
    $Problems = @()
    if (Test-Path $Paths.Current) { $Problems += "a node is already installed in $($Paths.Root); run install.ps1 -Role node -Uninstall first (weights and data are kept), or update it with the client's ragon update." }
    if ($Paths.LinkBin -and (Test-Path $Paths.LinkBin) -and -not ((Get-Item -Force -LiteralPath $Paths.LinkBin).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        $Problems += "$($Paths.LinkBin) is a folder, not the node's junction; remove it first."
    }
    $Found = @()
    foreach ($Dir in @($Paths.LinkRoot, $Paths.State)) {
        if ($Dir -and (Test-Path -LiteralPath $Dir)) { $Found += @(Find-NodeUntrusted $Dir $Hooks $Also $Paths.LinkBin) }
    }
    if ($Found.Count -gt 0) {
        $Shown = @($Found | Select-Object -First 5) -join "; "
        $More = if ($Found.Count -gt 5) { " (and $($Found.Count - 5) more)" } else { "" }
        $Problems += "these exist already and are not plain folders and files of an administrator: $Shown$More. Remove them, or take them over (takeown /R, icacls /reset /T) if they are yours."
    }
    return $Problems
}

# Takes a folder tree over: Administrators own everything in it, every ACL is reset to inherit,
# and the top gets $Grants alone (no inherited entries), so nothing a user set before stays.
function Set-NodeTree($Path, $Tools, [string[]]$Grants) {
    Invoke-NodeTool $Tools.Icacls @($Path, "/setowner", "*S-1-5-32-544", "/T", "/Q") | Out-Null
    Invoke-NodeTool $Tools.Icacls @($Path, "/reset", "/T", "/Q") | Out-Null
    $Cmd = @($Path, "/inheritance:r")
    foreach ($Grant in $Grants) { $Cmd += "/grant:r"; $Cmd += $Grant }
    Invoke-NodeTool $Tools.Icacls $Cmd | Out-Null
}

# A space-free path to the node's ragon.exe for the job account (the forced command cannot name a
# path with a space, and the primary calls plain `ragon`): C:\Ragon\bin, a junction into
# %ProgramFiles%\Ragon\current\bin. Its parent is closed to users (read and execute only), so
# nobody can swap the junction; the path stays the same when an update renames the trees.
function New-NodeLink($Paths, $Tools) {
    if (-not $Paths.LinkRoot) { return }
    New-Item -ItemType Directory -Force -Path $Paths.LinkRoot | Out-Null
    if (Test-Path $Paths.LinkBin) { [IO.Directory]::Delete($Paths.LinkBin) }
    Set-NodeTree $Paths.LinkRoot $Tools @("*S-1-5-18:(OI)(CI)(F)", "*S-1-5-32-544:(OI)(CI)(F)", "*S-1-5-32-545:(OI)(CI)(RX)")
    New-Item -ItemType Junction -Path $Paths.LinkBin -Target $Paths.Bin | Out-Null
}

# Removes the junction only (never what it points to), and its parent when nothing else is in it.
function Remove-NodeLink($Paths) {
    if (-not $Paths.LinkRoot -or -not (Test-Path $Paths.LinkRoot)) { return }
    if (Test-Path $Paths.LinkBin) { [IO.Directory]::Delete($Paths.LinkBin) }
    if (-not (Get-ChildItem -Force $Paths.LinkRoot)) { Remove-Item -Force $Paths.LinkRoot }
}

# Deletes a folder tree without following the links in it: a junction or symbolic link is
# removed itself, never what it points to. Windows PowerShell 5.1's Remove-Item -Recurse can
# follow a junction into the folder it names, and .NET Framework's Directory.Delete(path, true)
# stops at a junction ("The parameter is incorrect"), so the walk is done here.
function Remove-NodeTree([string]$Path) {
    if (-not (Test-Path -LiteralPath $Path)) { return }
    $Item = Get-Item -Force -LiteralPath $Path
    if ($Item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
        if ($Item -is [IO.DirectoryInfo]) { [IO.Directory]::Delete($Item.FullName) } else { [IO.File]::Delete($Item.FullName) }
        return
    }
    if ($Item -is [IO.DirectoryInfo]) {
        foreach ($Child in @($Item.EnumerateFileSystemInfos())) { Remove-NodeTree $Child.FullName }
        [IO.Directory]::Delete($Item.FullName)
    } else {
        $Item.Attributes = [IO.FileAttributes]::Normal
        [IO.File]::Delete($Item.FullName)
    }
}

# The node id: kept when the file is there, a new GUID otherwise (the file marks the machine as a node).
function Get-NodeId($Paths) {
    if (Test-Path $Paths.NodeId) {
        $Id = (Get-Content -Path $Paths.NodeId -TotalCount 1).Trim()
        if ($Id) { return $Id }
    }
    $Id = [Guid]::NewGuid().ToString()
    [IO.File]::WriteAllText($Paths.NodeId, $Id + "`n")
    return $Id
}

# The placed ragon's server usage: --advertise (the node that advertises on the LAN also gets its
# firewall rules from `server install`) and --jobs-dir (the service runs node jobs from that
# queue; `ragon update` lists it before it moves the node on) are passed only when it names them.
function Get-NodeServerUsage($Tools) {
    Invoke-NodeNative $Tools.Ragon @("server", "--help") | Out-Null
    return ($script:NodeNativeOutput -join " ")
}

function Save-NodeRecord($Paths, [string]$Tag, [string[]]$Created, [int]$Port, [string]$Sshd = "", [string]$Jobs = "") {
    $Lines = @("tag=$Tag", "service=$NodeServiceName", "created=$($Created -join ',')", "port=$Port", "state=$($Paths.NodeState)")
    if ($Paths.LinkBin) { $Lines += "link=$($Paths.LinkBin)" }
    if ($Sshd) { $Lines += "sshd=$Sshd" }
    if ($Jobs) { $Lines += "jobs=$Jobs" }
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Paths.Record) | Out-Null
    Set-Content -Path $Paths.Record -Encoding ASCII -Value $Lines
}

# Installs the node. $Tools: Ragon (the placed ragon.exe), Sc, Icacls, Sshd (paths of executables).
# $Hooks: UserExists, NewUser, RemoveUser, AddPath, RemovePath, RestartSshd, OwnerSid (scriptblocks).
# Called only after every download has been checked. $ClientBinaries are the unpacked executables
# to place next to ragon.exe; $Expect holds the SHA-256 each was checked with (Binaries, by file
# name) and the engine zip's (Engine): both are checked again once copied where users cannot write.
# $TrustedAlso: owner SIDs Test-NodePreflight accepts besides the system's (the installing user).
function Install-RagonNode($Paths, [string]$Tag, [string[]]$ClientBinaries, [string]$EngineZip, $Tools, $Hooks,
                           [string[]]$ServerArgs, [string[]]$EngineArgs, [string]$SshdConfig, [int]$Port, [int]$WaitSeconds,
                           $Expect, [string[]]$TrustedAlso = @()) {
    $Problems = @(Test-NodePreflight $Paths $Hooks $TrustedAlso)
    if ($Problems.Count -gt 0) { throw (($Problems -join " ") + " Nothing was changed.") }
    $Created = @()
    $SshdRecorded = ""
    $JobsDir = ""
    $Save = { Save-NodeRecord $Paths $Tag $Created $Port $SshdRecorded $JobsDir }
    # A clean-up after a failure removes the state folder only when this install made it.
    $NewState = -not (Test-Path $Paths.State)
    try {
        New-Item -ItemType Directory -Force -Path $Paths.Bin, $Paths.State | Out-Null
        # Taken over before anything is written in it; closed to users until the table opens it.
        Set-NodeTree $Paths.State $Tools @("*S-1-5-18:(OI)(CI)(F)", "*S-1-5-32-544:(OI)(CI)(F)")
        New-Item -ItemType Directory -Force -Path $Paths.NodeState | Out-Null
        & $Save
        foreach ($Dir in @($Paths.Models, $Paths.Advert, $Paths.Jobs)) { New-Item -ItemType Directory -Force -Path $Dir | Out-Null }
        foreach ($File in $ClientBinaries) {
            $Name = Split-Path -Leaf $File
            $Placed = Join-Path $Paths.Bin $Name
            Copy-Item -Force -Path $File -Destination $Placed
            if ((Get-FileHash -Algorithm SHA256 -LiteralPath $Placed).Hash.ToLower() -ne $Expect.Binaries[$Name]) {
                throw "$Name changed after it was checked; nothing was installed."
            }
        }
        # The zip is copied where users cannot write and checked again there, then unpacked from it.
        $Zip = Join-Path $Paths.Root "engine.zip"
        Copy-Item -Force -Path $EngineZip -Destination $Zip
        try {
            if ((Get-FileHash -Algorithm SHA256 -LiteralPath $Zip).Hash.ToLower() -ne $Expect.Engine) {
                throw "the engine zip changed after it was checked; nothing was installed."
            }
            Expand-NodeZip $Zip $Paths.Engine
        } finally { Remove-Item -Force -LiteralPath $Zip -ErrorAction SilentlyContinue }
        if (-not (& $Hooks.UserExists $NodeJobAccount)) {
            & $Hooks.NewUser $NodeJobAccount
            $Created += "user:$NodeJobAccount"
            & $Save
        }
        $Engine = Join-Path $Paths.Engine "ragon-serve.exe"
        if (-not (Test-Path $Engine)) { throw "the engine zip has no ragon-serve.exe." }
        if ($Paths.LinkRoot) {
            New-NodeLink $Paths $Tools
            & $Hooks.AddPath $Paths.LinkBin
            $Created += "path:$($Paths.LinkBin)"
            & $Save
        }
        $Usage = Get-NodeServerUsage $Tools
        # `server install` makes the firewall rules of an advertising node: UDP 5353 and the engine's port.
        $Advertise = @()
        if ($Usage -match "--advertise") { $Advertise = @("--advertise", (Get-NodeId $Paths)) }
        # The job runner starts only with --jobs-dir; the record keeps it for `ragon update`.
        $Jobs = @()
        if ($Usage -match "--jobs-dir") { $JobsDir = $Paths.Jobs; $Jobs = @("--jobs-dir", $JobsDir) }
        # --host is where the supervisor probes the engine (a connection to 0.0.0.0 fails on Windows);
        # the address the engine listens on is its own argument, in $EngineArgs (--host 0.0.0.0).
        $Install = @("server", "install", "--name", $NodeServiceName, "--state-dir", $Paths.NodeState, "--host", "127.0.0.1",
            "--port", "$Port") + $Advertise + $Jobs + $ServerArgs + @("--", $Engine) + $EngineArgs
        Invoke-NodeTool $Tools.Ragon $Install | Out-Null
        $Created += "service:$NodeServiceName"
        & $Save
        # The service's virtual account exists once the service does, so the table is applied now.
        $Sandbox = @($NodeSandboxAccounts | Where-Object { & $Hooks.UserExists $_ })
        foreach ($Command in (Get-NodeAclCommands $Paths $Sandbox $NodeServiceAccount $NodeJobAccount)) {
            # The id file is made by `ragon node id` later and inherits the root's entries; the
            # table's rows for it are applied only when it already exists.
            if (-not (Test-Path $Command[0])) { continue }
            Invoke-NodeTool $Tools.Icacls $Command | Out-Null
        }
        if ($SshdConfig -and (Set-NodeSshdBlock $SshdConfig $NodeJobAccount $Tools.Sshd)) {
            $SshdRecorded = $SshdConfig
            & $Save
            & $Hooks.RestartSshd
        }
        Invoke-NodeNative $Tools.Sc @("start", $NodeServiceName) | Out-Null
        $Deadline = (Get-Date).AddSeconds($WaitSeconds)
        $Healthy = $false
        while ((Get-Date) -lt $Deadline) {
            $Code = Invoke-NodeNative $Tools.Ragon @("server", "status", "--name", $NodeServiceName, "--state-dir", $Paths.NodeState, "--port", "$Port")
            if ($Code -eq 0) { $Healthy = $true; break }
            Start-Sleep -Seconds 1
        }
        return $Healthy
    } catch {
        $Message = $_.Exception.Message
        try { Uninstall-RagonNode $Paths $Tools $Hooks $SshdConfig $NewState | Out-Null } catch { $Message += " (and the clean-up failed: $($_.Exception.Message))" }
        throw $Message
    }
}

# Removes what Install-RagonNode made: the service (and its firewall rules, through the supervisor),
# the distribution, the sshd block, the job account if this installer made it. Weights and the
# forge's data stay unless $RemoveData.
function Uninstall-RagonNode($Paths, $Tools, $Hooks, [string]$SshdConfig, [bool]$RemoveData) {
    $Created = ""
    if (Test-Path $Paths.Record) {
        $Line = Get-Content -Path $Paths.Record | Where-Object { $_ -like "created=*" } | Select-Object -First 1
        if ($Line) { $Created = $Line.Substring(8) }
    }
    $Removed = @()
    # The supervisor's own uninstall stops the service and removes it and its firewall rules;
    # sc.exe is the fallback when the binary is gone or that fails.
    $Ragon = Join-Path $Paths.Bin "ragon.exe"
    $Done = $false
    if (Test-Path $Ragon) {
        $Done = ((Invoke-NodeNative $Tools.Ragon @("server", "uninstall", "--name", $NodeServiceName)) -eq 0)
    }
    if (-not $Done) {
        Invoke-NodeNative $Tools.Sc @("stop", $NodeServiceName) | Out-Null
        Start-Sleep -Seconds 2
        Invoke-NodeNative $Tools.Sc @("delete", $NodeServiceName) | Out-Null
    }
    $Removed += "service"
    if ($SshdConfig -and (Test-Path $SshdConfig) -and ([IO.File]::ReadAllText($SshdConfig).Contains($NodeSshdBegin))) {
        Remove-NodeSshdBlock $SshdConfig
        & $Hooks.RestartSshd
    }
    if ($Paths.LinkBin) {
        & $Hooks.RemovePath $Paths.LinkBin
        Remove-NodeLink $Paths
    }
    if (Test-Path $Paths.Root) {
        # The service's files can stay locked for a moment after it stops.
        for ($Try = 1; (Test-Path $Paths.Root) -and $Try -le 8; $Try++) {
            try { Remove-NodeTree $Paths.Root } catch { Start-Sleep -Seconds 2 }
        }
        if (Test-Path $Paths.Root) { throw "could not remove $($Paths.Root); a file in it is still in use." }
        $Removed += "distribution"
    }
    # Kept data passes to Administrators first: files the job account wrote would otherwise be
    # owned by a deleted account's SID, which the next install refuses to adopt.
    if (-not $RemoveData -and (Test-Path $Paths.State)) {
        Invoke-NodeNative $Tools.Icacls @($Paths.State, "/setowner", "*S-1-5-32-544", "/T", "/Q") | Out-Null
    }
    if ($Created -like "*user:$NodeJobAccount*") { & $Hooks.RemoveUser $NodeJobAccount; $Removed += "user" }
    if ($RemoveData -and (Test-Path $Paths.State)) { Remove-NodeTree $Paths.State; $Removed += "data" }
    elseif (Test-Path $Paths.NodeState) { Remove-NodeTree $Paths.NodeState }
    return $Removed
}


$Repo = if ($env:RAGON_RELEASE_REPO) { $env:RAGON_RELEASE_REPO } else { "dreamference/ragon" }
$Api = if ($env:RAGON_RELEASE_API) { $env:RAGON_RELEASE_API } else { "https://api.github.com" }
$InstallDir = if ($env:RAGON_INSTALL_DIR) { $env:RAGON_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "Programs\Ragon\bin" }

# -- which machine ---------------------------------------------------------------------------

# Under x64 emulation on an Arm PC, PROCESSOR_ARCHITECTURE says AMD64 and the real one is in
# PROCESSOR_ARCHITEW6432.
$Machine = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
switch ($Machine) {
    "ARM64" { $Arch = "aarch64" }
    "AMD64" { $Arch = "x86_64" }
    default { Fail "unsupported processor: $Machine" }
}
$Target = "$Arch-pc-windows-msvc"
$Build = [Environment]::OSVersion.Version.Build
if ($Build -lt 22000) {
    Fail "Ragon needs Windows 11 (build 22000 or later); this is build $Build."
}

# -- the node role: where it goes, who does it, and uninstall ---------------------------------

$NodeEngineZip = ""
if ($Role -eq "node") {
    $NodePaths = Get-NodePaths $(if ($env:RAGON_NODE_PROGRAM_FILES) { $env:RAGON_NODE_PROGRAM_FILES } else { $env:ProgramFiles }) `
        $(if ($env:RAGON_NODE_PROGRAM_DATA) { $env:RAGON_NODE_PROGRAM_DATA } else { $env:ProgramData }) `
        $(if ($env:RAGON_NODE_LINK_ROOT) { $env:RAGON_NODE_LINK_ROOT } else { Join-Path $env:SystemDrive "Ragon" })
    $NodeSshdConfig = if ($NoSshd) { "" } elseif ($env:RAGON_NODE_SSHD_CONFIG) { $env:RAGON_NODE_SSHD_CONFIG } else { Join-Path $env:ProgramData "ssh\sshd_config" }
    $NodeTools = @{
        Ragon = (Join-Path $NodePaths.Bin "ragon.exe")
        Sc = (Join-Path $env:SystemRoot "System32\sc.exe")
        Icacls = (Join-Path $env:SystemRoot "System32\icacls.exe")
        Sshd = $(if (Test-Path (Join-Path $env:SystemRoot "System32\OpenSSH\sshd.exe")) { Join-Path $env:SystemRoot "System32\OpenSSH\sshd.exe" }
                 else { (Get-Command sshd.exe -ErrorAction SilentlyContinue).Source })
    }
    $NodeHooks = @{
        UserExists = { param($Name) [bool](Get-LocalUser -Name $Name -ErrorAction SilentlyContinue) }
        # A local account for the node's jobs: not an administrator, a long random password nobody
        # is told (the account is reached by key over ssh, never by password).
        NewUser = {
            param($Name)
            $Bytes = New-Object byte[] 24
            (New-Object Security.Cryptography.RNGCryptoServiceProvider).GetBytes($Bytes)
            $Secret = ConvertTo-SecureString ("aA1!" + [Convert]::ToBase64String($Bytes)) -AsPlainText -Force
            New-LocalUser -Name $Name -Password $Secret -PasswordNeverExpires -UserMayNotChangePassword `
                -AccountNeverExpires -Description "Ragon node jobs" | Out-Null
        }
        RemoveUser = { param($Name) Remove-LocalUser -Name $Name -ErrorAction SilentlyContinue }
        # The machine PATH, so the job account (which has no profile of its own) finds `ragon`.
        AddPath = {
            param($Dir)
            Edit-NodeMachinePath $Dir $true
        }
        RemovePath = {
            param($Dir)
            Edit-NodeMachinePath $Dir $false
        }
        # sshd reads its configuration at start. Restarting it from an ssh session ends that session,
        # so over ssh the script says so instead.
        OwnerSid = { param($Path) (Get-Acl -LiteralPath $Path).GetOwner([Security.Principal.SecurityIdentifier]).Value }
        RestartSshd = {
            if ($env:SSH_CONNECTION) { Say "  sshd: restart it (Restart-Service sshd) from a console for the new block to apply; this session is an ssh login." }
            elseif (Get-Service sshd -ErrorAction SilentlyContinue) { Restart-Service sshd }
        }
    }
    $Elevated = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    if (-not $Elevated) { Fail "the node role needs an elevated PowerShell (Run as administrator); over SSH there is no UAC, so start the shell elevated. Nothing was changed." }
    if ($Uninstall) {
        $Gone = Uninstall-RagonNode $NodePaths $NodeTools $NodeHooks $NodeSshdConfig ([bool]$RemoveData)
        Say "Removed the node: $($Gone -join ', ')."
        if (-not $RemoveData) { Say "Weights and the forge's data under $($NodePaths.State) were kept; -RemoveData removes them too." }
        return
    }
    $Problems = Test-NodeHost $Build $Arch $Elevated
    if ($Problems.Count -gt 0) { Fail ("not a node host: " + ($Problems -join " ") + " Nothing was changed.") }
    # Before anything is downloaded or the client is touched.
    # The installing administrator, and the node's own accounts when they exist (a kept state folder
    # holds the job queue and the node id they wrote).
    $NodeTrustedAlso = @([Security.Principal.WindowsIdentity]::GetCurrent().User.Value) +
        @(@($NodeJobAccount) + $NodeSandboxAccounts | ForEach-Object { (Get-LocalUser -Name $_ -ErrorAction SilentlyContinue).SID.Value } | Where-Object { $_ })
    $Problems = @(Test-NodePreflight $NodePaths $NodeHooks $NodeTrustedAlso)
    if ($Problems.Count -gt 0) { Fail ("the node cannot be installed: " + ($Problems -join " ") + " Nothing was changed.") }
} elseif ($Uninstall -or $RemoveData) {
    Fail "-Uninstall and -RemoveData belong to -Role node."
}

# -- which release ---------------------------------------------------------------------------

$Headers = @{ "Accept" = "application/vnd.github+json"; "User-Agent" = "ragon-install" }
$Token = if ($env:GH_TOKEN) { $env:GH_TOKEN } elseif ($env:GITHUB_TOKEN) { $env:GITHUB_TOKEN } else { "" }
if ($Token) { $Headers["Authorization"] = "Bearer $Token" }

$ReleaseUrl = if ($Version) { "$Api/repos/$Repo/releases/tags/v$($Version.TrimStart('v'))" } else { "$Api/repos/$Repo/releases/latest" }
try {
    $Release = Invoke-RestMethod -Uri $ReleaseUrl -Headers $Headers -UseBasicParsing
} catch {
    $Status = if ($_.Exception.Response) { [int]$_.Exception.Response.StatusCode } else { 0 }
    if (($Status -eq 403 -or $Status -eq 429) -and "$($_.ErrorDetails.Message) $($_.Exception.Message)" -match "rate limit") {
        Fail "GitHub's API rate limit is used up for this network ($ReleaseUrl). Wait a while, or set GH_TOKEN to any GitHub token and run this again."
    }
    Fail "could not read $ReleaseUrl ($($_.Exception.Message)). Drafts and pre-releases are not offered; a private fork needs GH_TOKEN."
}
$Tag = $Release.tag_name
$Assets = @{}
foreach ($Asset in $Release.assets) { $Assets[$Asset.name] = $Asset.url }

$Required = @("ragon", "codex-code-mode-host")
$Optional = @("ragon-search", "ragon-fetch", "ragon-code", "codex-windows-sandbox-setup", "codex-command-runner")
$SumsName = "ragon-$Target.sha256sums"
foreach ($Name in $Required) {
    if (-not $Assets.ContainsKey("$Name-$Target.gz")) { Fail "release $Tag carries no $Name for $Target." }
}
if (-not $Assets.ContainsKey($SumsName)) { Fail "release $Tag has no $SumsName." }

Say "Installing Ragon $Tag for $Target (preview)"

# -- the signature ---------------------------------------------------------------------------

# Ragon's release keys, as `ragon update` compiles them in (ragon-rs/release-signing.pub) and
# install.sh carries them; every release from $SignedSince on signs SHA256SUMS with one of them.
$ReleaseKeys = @('ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAII9boVZe01adX9jSk8Hh/GAJURmqzCznictI3meZX0/i ragon-release-2026-11')
$SignedSince = "0.9.0"

# Whether a tag is $SignedSince or newer. A tag that is not a version is treated as new: a name
# nobody can read is no reason to skip the check (install.sh and `ragon update` decide the same way).
function Test-SigningRequired([string]$Name) {
    if ($Name.TrimStart("v") -notmatch "^(\d+)\.(\d+)\.(\d+)") { return $true }
    $Have = @([long]$Matches[1], [long]$Matches[2], [long]$Matches[3])
    $Since = @($SignedSince.Split(".") | ForEach-Object { [long]$_ })
    for ($I = 0; $I -lt 3; $I++) {
        if ($Have[$I] -gt $Since[$I]) { return $true }
        if ($Have[$I] -lt $Since[$I]) { return $false }
    }
    return $true
}

# Windows 11's own OpenSSH client first (an optional feature, on by default), then one on PATH.
function Find-SshKeygen {
    foreach ($Folder in @("System32", "Sysnative")) {
        $Path = Join-Path $env:SystemRoot "$Folder\OpenSSH\ssh-keygen.exe"
        if (Test-Path -LiteralPath $Path) { return $Path }
    }
    $Found = Get-Command ssh-keygen.exe -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($Found) { return $Found.Path }
    return $null
}

# The allowed-signers lines for public-key lines: `ragon-release <type> <key>`.
function Get-Signers([string[]]$Keys) {
    return @($Keys | ForEach-Object { $_.Trim() } | Where-Object { $_ -and -not $_.StartsWith("#") } |
        ForEach-Object { $Parts = $_ -split "\s+"; "ragon-release $($Parts[0]) $($Parts[1])" })
}

# ssh-keygen -Y verify, the signed file on standard input. Windows's ssh-keygen never sees the end
# of a pipe, and 5.1 re-encodes text it pipes to a program, so the input is the file itself
# (Start-Process hands ssh-keygen the file's handle).
function Test-Signature([string]$Keygen, [string]$Signers, [string]$Namespace, [string]$Signature, [string]$Message) {
    $Folder = Split-Path -Parent $Signers
    $Out = Join-Path $Folder "verify.out"
    $Err = Join-Path $Folder "verify.err"
    $Arguments = "-Y verify -f `"$Signers`" -I ragon-release -n $Namespace -s `"$Signature`""
    $Process = Start-Process -FilePath $Keygen -ArgumentList $Arguments -RedirectStandardInput $Message `
        -RedirectStandardOutput $Out -RedirectStandardError $Err -NoNewWindow -Wait -PassThru
    $script:VerifyOutput = (@(Get-Content -Path $Out, $Err) -join " ").Trim()
    return ($Process.ExitCode -eq 0)
}

function Read-Sums([string]$Path) {
    $Table = @{}
    foreach ($Line in (Get-Content -Path $Path)) {
        $Parts = $Line.Trim() -split "\s+"
        if ($Parts.Count -ge 2) { $Table[$Parts[1].TrimStart("*")] = $Parts[0].ToLower() }
    }
    return $Table
}

# -- download and check everything before installing anything ---------------------------------

$Work = Join-Path ([IO.Path]::GetTempPath()) ("ragon-install-" + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $Work | Out-Null
$DownloadHeaders = @{ "Accept" = "application/octet-stream"; "User-Agent" = "ragon-install" }

# The token goes only to the API root, never to a download URL the release names: a release that was
# tampered with, or an old repository name taken over, would otherwise collect it (as in install.sh).
function Get-Asset([string]$Name) {
    $Path = Join-Path $Work $Name
    Say "  downloading $Name"
    $Sent = $DownloadHeaders.Clone()
    if ($Token -and $Assets[$Name].StartsWith("$Api/", [StringComparison]::Ordinal)) {
        $Sent["Authorization"] = "Bearer $Token"
    }
    Invoke-WebRequest -Uri $Assets[$Name] -Headers $Sent -OutFile $Path -UseBasicParsing
    return $Path
}

function Expand-Gzip([string]$From, [string]$To) {
    $In = [IO.File]::OpenRead($From)
    try {
        $Gzip = New-Object IO.Compression.GZipStream($In, [IO.Compression.CompressionMode]::Decompress)
        $Out = [IO.File]::Create($To)
        try { $Gzip.CopyTo($Out) } finally { $Out.Dispose(); $Gzip.Dispose() }
    } finally { $In.Dispose() }
}

try {
    $SumsFile = Get-Asset $SumsName
    # SHA256SUMS lists every asset of the release, this PC's checksum file and the desktop app's
    # installer among them, and its signature is what ties everything checked below to the release
    # job (specs/DREAMFERENCE_RELEASE_SIGNING.md).
    $Signed = $null
    if ($Assets.ContainsKey("SHA256SUMS.sig")) {
        $Keygen = Find-SshKeygen
        if (-not $Keygen) {
            Fail "ssh-keygen.exe, which checks the release's signature, was not found. Add Windows's OpenSSH Client (Settings > System > Optional features) and run this again; nothing was installed."
        }
        if (-not $Assets.ContainsKey("SHA256SUMS")) { Fail "release $Tag has SHA256SUMS.sig but no SHA256SUMS; nothing was installed." }
        $Signature = Get-Asset "SHA256SUMS.sig"
        $SignedSums = Get-Asset "SHA256SUMS"
        $Signers = Join-Path $Work "allowed_signers"
        $Lines = Get-Signers $ReleaseKeys
        # A key rotation: the release names the new key, signed by a key trusted here under a
        # namespace of its own (specs/DREAMFERENCE_RELEASE_SIGNING.md section 5).
        if ($Assets.ContainsKey("release-key-transition.pub")) {
            if (-not $Assets.ContainsKey("release-key-transition.pub.sig")) {
                Fail "release $Tag names a new release key but carries no endorsement of it; nothing was installed."
            }
            $NewKey = Get-Asset "release-key-transition.pub"
            $NewKeySig = Get-Asset "release-key-transition.pub.sig"
            [IO.File]::WriteAllText($Signers, (($Lines -join "`n") + "`n"))
            if (-not (Test-Signature $Keygen $Signers "ragon-release-key" $NewKeySig $NewKey)) {
                Fail "release $Tag names a new release key that no trusted key has signed; nothing was installed."
            }
            $Lines += Get-Signers (Get-Content -Path $NewKey)
        }
        # LF and no byte-order mark, which ssh-keygen reads as it reads a file written on Linux.
        [IO.File]::WriteAllText($Signers, (($Lines -join "`n") + "`n"))
        if (-not (Test-Signature $Keygen $Signers "ragon-release" $Signature $SignedSums)) {
            Fail "release $Tag failed its signature check: SHA256SUMS is not signed by Ragon's release key ($VerifyOutput). Nothing was installed."
        }
        $Signed = Read-Sums $SignedSums
        if (-not $Signed.ContainsKey($SumsName)) { Fail "SHA256SUMS has no checksum for $SumsName; nothing was installed." }
        if ((Get-FileHash -Algorithm SHA256 -Path $SumsFile).Hash.ToLower() -ne $Signed[$SumsName]) {
            Fail "$SumsName does not match its checksum in SHA256SUMS; nothing was installed."
        }
        Say "  release $Tag is signed by Ragon's release key"
    } elseif (-not $Version) {
        # Asked for the latest release, an install takes whatever is marked latest: the tag of an
        # unsigned one is only a name, and an old-looking one (v0.8.9) must not bypass the
        # signature. A release from before signing is installed only when named with -Version.
        Fail "the latest release ($Tag) is not signed (it has no SHA256SUMS.sig); nothing was installed. Pass -Version to install a release from before $SignedSince on purpose."
    } elseif (Test-SigningRequired $Tag) {
        Fail "release $Tag is not signed (it has no SHA256SUMS.sig); nothing was installed. Every release from $SignedSince on is signed by Ragon's release key, so an unsigned one did not come from its release job."
    } else {
        Say "  release $Tag predates signed releases ($SignedSince); it is checked against its checksums only"
    }

    $Sums = Read-Sums $SumsFile

    $Wanted = @()
    foreach ($Name in $Required) { $Wanted += $Name }
    foreach ($Name in $Optional) {
        if ($Assets.ContainsKey("$Name-$Target.gz")) { $Wanted += $Name } else { Say "  release $Tag carries no $Name; skipping it" }
    }

    $Ready = @()
    $NodeExpect = @{ Binaries = @{}; Engine = "" }
    foreach ($Name in $Wanted) {
        $Asset = "$Name-$Target.gz"
        $Archive = Get-Asset $Asset
        if (-not $Sums.ContainsKey($Asset)) { Fail "$SumsName has no checksum for $Asset; nothing was installed." }
        $Actual = (Get-FileHash -Algorithm SHA256 -Path $Archive).Hash.ToLower()
        if ($Actual -ne $Sums[$Asset]) { Fail "$Asset does not match its checksum; nothing was installed." }
        $Binary = Join-Path $Work "$Name.exe"
        Expand-Gzip $Archive $Binary
        $NodeExpect.Binaries["$Name.exe"] = (Get-FileHash -Algorithm SHA256 -LiteralPath $Binary).Hash.ToLower()
        $Ready += $Name
    }

    # The node's engine: the zip must be named in the signed SHA256SUMS. A node install with no
    # signed sums fails (unlike the desktop app below, which is skipped), because the driver runs
    # whatever code object it is given (spec section 7).
    if ($Role -eq "node") {
        if ($null -eq $Signed) { Fail "the node role needs a signed release; $Tag has no SHA256SUMS.sig, so no engine can be checked. Nothing was installed." }
        $ZipName = "ragon-engine-$($Tag.TrimStart('v'))-windows-gfx1151.zip"
        if (-not $Assets.ContainsKey($ZipName)) { Fail "release $Tag carries no $ZipName; nothing was installed." }
        if (-not $Signed.ContainsKey($ZipName)) { Fail "SHA256SUMS has no checksum for $ZipName; nothing was installed." }
        $ZipFile = Get-Asset $ZipName
        if ((Get-FileHash -Algorithm SHA256 -Path $ZipFile).Hash.ToLower() -ne $Signed[$ZipName]) {
            Fail "$ZipName does not match its checksum in SHA256SUMS; nothing was installed."
        }
        if (-not (Test-ZipEntriesSafe $ZipFile (Join-Path $Work "zip-check"))) { Fail "$ZipName has an entry outside its folder; nothing was installed." }
        if (-not (Test-ZipHasEngine $ZipFile)) { Fail "$ZipName has no ragon-serve.exe; nothing was installed." }
        $NodeExpect.Engine = $Signed[$ZipName]
        # Kept past the clean-up of $Work below: the node is placed after the sandbox step.
        $NodeEngineZip = Join-Path ([IO.Path]::GetTempPath()) ("ragon-engine-" + [Guid]::NewGuid().ToString("N") + ".zip")
        Move-Item -Force -Path $ZipFile -Destination $NodeEngineZip
    }

    # The desktop app: the release's installer for this PC (windows.yml names it
    # Ragon-Setup-<version>-<arm64|x64>.exe), checked against the release-wide SHA256SUMS
    # before anything is installed. A release without one installs the command-line client alone.
    $AppSetup = $null
    if (-not $NoApp) {
        $AppArch = if ($Arch -eq "aarch64") { "arm64" } else { "x64" }
        $SetupName = @($Assets.Keys | Where-Object { $_ -match "^Ragon-Setup-.+-$AppArch\.exe$" }) | Select-Object -First 1
        if (-not $SetupName) {
            Say "  release $Tag carries no desktop app for $Target; installing the command-line client only"
        } elseif (-not $Assets.ContainsKey("SHA256SUMS")) {
            Say "  release $Tag has no SHA256SUMS to check $SetupName against; leaving the desktop app out"
        } else {
            # The signed SHA256SUMS; only a release from before signing began gets here without one.
            $AllSums = $Signed
            if ($null -eq $AllSums) { $AllSums = Read-Sums (Get-Asset "SHA256SUMS") }
            if (-not $AllSums.ContainsKey($SetupName)) { Fail "SHA256SUMS has no checksum for $SetupName; nothing was installed." }
            $AppSetup = Get-Asset $SetupName
            $Actual = (Get-FileHash -Algorithm SHA256 -Path $AppSetup).Hash.ToLower()
            if ($Actual -ne $AllSums[$SetupName]) { Fail "$SetupName does not match its checksum; nothing was installed." }
            # SmartScreen's first-run check is for files marked as coming from the internet, which
            # browsers mark; make sure this one carries no such mark.
            Unblock-File -Path $AppSetup
        }
    }

    # -- install ---------------------------------------------------------------------------------

    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    foreach ($Name in $Ready) {
        $Destination = Join-Path $InstallDir "$Name.exe"
        # A running .exe cannot be overwritten, but it can be renamed: move it aside, as
        # `ragon update` does; ragon deletes the leftovers at its next start.
        if (Test-Path $Destination) {
            $Aside = "$Destination.old"
            $Copy = 1
            while (Test-Path $Aside) {
                try { Remove-Item -Force $Aside } catch { $Copy += 1; $Aside = "$Destination.$Copy.old" }
            }
            Move-Item -Force -Path $Destination -Destination $Aside
        }
        Move-Item -Force -Path (Join-Path $Work "$Name.exe") -Destination $Destination
    }

    # The NSIS installer, silent (/S): no administrator, no window, and the app is not started.
    # It closes a running copy of the app before it replaces it.
    $AppResult = ""
    if ($AppSetup) {
        Say "  installing the desktop app"
        try {
            $Process = Start-Process -FilePath $AppSetup -ArgumentList "/S" -PassThru -Wait
            $AppResult = if ($Process.ExitCode -eq 0) { "ok" } else { "the installer exited with $($Process.ExitCode)" }
            if ($AppResult -eq "ok") {
                # What `ragon update` reads to update the app too (update.rs, APP_RECORD).
                $AppKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Ragon"
                $AppDir = (Get-ItemProperty -Path $AppKey -ErrorAction SilentlyContinue).InstallLocation
                if (-not $AppDir) { $AppDir = Join-Path $env:LOCALAPPDATA "Programs\Ragon\App" }
                $Record = Join-Path (Split-Path -Parent $InstallDir) "desktop-app"
                Set-Content -Path $Record -Encoding UTF8 -Value @("path=$AppDir", "version=$Tag")
            }
        } catch {
            $AppResult = $_.Exception.Message
        }
    }
} finally {
    Remove-Item -Recurse -Force -Path $Work -ErrorAction SilentlyContinue
}

# -- PATH ------------------------------------------------------------------------------------

if (-not $NoPath) {
    # The registry value itself, unexpanded, and written back as the kind it was: the .NET
    # environment calls expand %USERPROFILE% and the like on read and write REG_SZ, which would
    # freeze every entry that depends on another variable.
    $Key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey("Environment")
    $UserPath = [string]$Key.GetValue("Path", "", [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    $Kind = if ($Key.GetValueNames() -contains "Path") { $Key.GetValueKind("Path") } else { [Microsoft.Win32.RegistryValueKind]::ExpandString }
    # Always an array: with one entry (a fresh profile has just WindowsApps) PowerShell returns a
    # string, and + then glued the install folder onto it (`...\WindowsAppsC:\...`).
    $Entries = @($UserPath -split ";" | Where-Object { $_ })
    if (-not ($Entries -contains $InstallDir)) {
        $Key.SetValue("Path", (($Entries + $InstallDir) -join ";"), $Kind)
        # Setting and clearing a user variable through .NET broadcasts WM_SETTINGCHANGE, so new
        # terminals see the PATH just written.
        [Environment]::SetEnvironmentVariable("RAGON_PATH_CHANGED", "1", "User")
        [Environment]::SetEnvironmentVariable("RAGON_PATH_CHANGED", $null, "User")
        Say "Added $InstallDir to your PATH (new terminals see it)."
    }
    $Key.Close()
    if (-not (($env:Path -split ";") -contains $InstallDir)) { $env:Path = "$env:Path;$InstallDir" }
}

# -- the sandbox, the firewall and the audit: one Administrator prompt ------------------------
#
# Codex's Windows sandbox runs commands as two local accounts that only an administrator can
# create; the offline one is what takes a command's network away at /airgapped on. One elevation
# sets them up for this user, lets ragon hear node announcements (mDNS, UDP 5353) on Private and
# Domain networks, and runs the egress audit once (section 7.1, 10.2, 14 of the spec). Declining
# leaves a working client whose commands run without a sandbox; ragon says so at every start.
#
# The firewall rules are machine-wide, so there is one per program path, named after it:
# "Ragon-mDNS-" and the first 8 hex digits of the SHA-256 of the full path, lowercased, in
# UTF-8 (section 16.4; `ragon uninstall` names and removes them the same way). An install removes
# only the rules whose program is in its own folders, so another user's install keeps its rules.

# Whether Codex's elevated sandbox is set up for a CODEX_HOME, read as Codex reads it
# (sandbox_setup_is_complete in codex-rs/windows-sandbox-rs): its setup marker and its accounts
# file, both at Codex's setup version (SETUP_VERSION in windows-sandbox-rs/src/setup.rs).
$SandboxSetupVersion = 5
function Test-SandboxSetUp([string]$CodexDir) {
    foreach ($File in @(".sandbox\setup_marker.json", ".sandbox-secrets\sandbox_users.json")) {
        try {
            $Version = (Get-Content -Raw -Path (Join-Path $CodexDir $File) -ErrorAction Stop | ConvertFrom-Json).version
        } catch {
            return $false
        }
        if ($Version -ne $SandboxSetupVersion) { return $false }
    }
    return $true
}

$Ragon = Join-Path $InstallDir "ragon.exe"
if ($NoSandbox) {
    Say ""
    Say "Skipped the Windows sandbox (-NoSandbox): commands will run without one."
} elseif (-not (Test-Path (Join-Path $InstallDir "codex-windows-sandbox-setup.exe"))) {
    Say ""
    Say "Release $Tag carries no sandbox setup for ${Target}: commands will run without a sandbox."
} else {
    # Named now, as the user who ran this: with an administrator's credentials typed over the
    # shoulder of a standard user, the elevated process runs as that administrator instead, with
    # the administrator's environment and profile. So everything that is this user's is decided
    # here and handed over: the account, CODEX_HOME, and what picks the node the audit judges (the
    # remembered node in CODEX_HOME, the configuration under the home folder, and the variables).
    # The account's own name: $env:USERDOMAIN is the workgroup over OpenSSH, and the sandbox's
    # setup then fails with 1332 (no mapping between account names and security IDs).
    $User = [Security.Principal.WindowsIdentity]::GetCurrent().Name
    $CodexHome = if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path $env:USERPROFILE ".ragon" }
    $UserHome = if ($env:HOME) { $env:HOME } else { $env:USERPROFILE }
    New-Item -ItemType Directory -Force -Path $CodexHome | Out-Null
    # The desktop app's installer (desktop/electron/windows/installer.nsi) is per user, without
    # administrator rights, so it cannot add firewall rules: the app's two programs get theirs here
    # whenever it is present, the copy this run installed above included.
    $AppDir = ""
    try {
        $AppDir = (Get-ItemProperty -Path "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Ragon" -Name InstallLocation -ErrorAction Stop).InstallLocation
    } catch { }
    if (-not $AppDir) { $AppDir = Join-Path $env:LOCALAPPDATA "Programs\Ragon\App" }
    $Programs = @($Ragon)
    if (Test-Path (Join-Path $AppDir "Ragon.exe")) {
        $Programs += Join-Path $AppDir "Ragon.exe"
        $Programs += Join-Path $AppDir "resources\ragon.exe"
    }
    $Step = Join-Path ([IO.Path]::GetTempPath()) ("ragon-elevated-" + [Guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Path $Step | Out-Null
    $Script = Join-Path $Step "elevated.ps1"
    $Log = Join-Path $Step "elevated.log"
    Set-Content -Path $Script -Encoding ASCII -Value @'
param([string]$Ragon, [string]$User, [string]$CodexHome, [string]$UserHome, [string]$VllmHost, [string]$Node,
      [string]$ConfigPath, [string]$InstallDir, [string]$AppDir, [string]$Programs, [string]$Log)
$ErrorActionPreference = "Continue"
function Note([string]$Text) { Add-Content -Path $Log -Value $Text -Encoding UTF8 }
# The rule of one program (section 16.4): the first 8 hex digits of the SHA-256 of its full path,
# lowercased, in UTF-8.
function RuleName([string]$Program) {
    $Sha = [Security.Cryptography.SHA256]::Create()
    try { $Hash = $Sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($Program.ToLowerInvariant())) } finally { $Sha.Dispose() }
    return "Ragon-mDNS-" + (($Hash[0..3] | ForEach-Object { $_.ToString("x2") }) -join "")
}
& $Ragon sandbox setup --elevated --user $User --codex-home $CodexHome 2>&1 | ForEach-Object { Note "$_" }
Note "sandbox-setup-exit=$LASTEXITCODE"
try {
    # Only this install's rules go: those whose program is in its folders, whatever their name; a
    # rule that points at another user's install stays.
    $Mine = @($InstallDir, $AppDir) | Where-Object { $_ } | ForEach-Object { $_.TrimEnd("\") + "\" }
    Get-NetFirewallRule | Where-Object { $_.Name -like "Ragon-mDNS-*" } | ForEach-Object {
        $Program = [string]($_ | Get-NetFirewallApplicationFilter).Program
        $Ours = $false
        foreach ($Dir in $Mine) { if ($Program.StartsWith($Dir, [StringComparison]::OrdinalIgnoreCase)) { $Ours = $true } }
        if ($Ours) { $_ | Remove-NetFirewallRule -ErrorAction Stop }
    }
    foreach ($Program in ($Programs -split "\|" | Where-Object { $_ })) {
        New-NetFirewallRule -Name (RuleName $Program) -DisplayName "Ragon node discovery (mDNS, $Program)" -Direction Inbound -Action Allow -Protocol UDP -LocalPort 5353 -Program $Program -Profile Private, Domain -ErrorAction Stop | Out-Null
    }
    Note "firewall=ok"
} catch {
    Note "firewall=failed: $($_.Exception.Message)"
}
# The audit as the user would run it: their CODEX_HOME (the remembered node), their home folder
# (the configuration file), and their variables, never the administrator's.
$env:CODEX_HOME = $CodexHome
if ($UserHome) { $env:HOME = $UserHome }
foreach ($Pair in @(@("DREAMFERENCE_VLLM_HOST", $VllmHost), @("RAGON_NODE", $Node), @("DREAMFERENCE_CONFIG_PATH", $ConfigPath))) {
    if ($Pair[1]) { Set-Item -Path "Env:$($Pair[0])" -Value $Pair[1] } else { Remove-Item -Path "Env:$($Pair[0])" -ErrorAction SilentlyContinue }
}
& $Ragon audit egress 2>&1 | ForEach-Object { Note "$_" }
Note "audit-exit=$LASTEXITCODE"
'@
    # Each value quoted; an empty one is left out, since Windows PowerShell 5.1 can drop an empty
    # quoted argument and take the next parameter's name for its value. A trailing backslash is
    # doubled, so it does not escape the closing quote.
    $Pass = [ordered]@{
        Ragon = $Ragon; User = $User; CodexHome = $CodexHome; UserHome = $UserHome
        VllmHost = $env:DREAMFERENCE_VLLM_HOST; Node = $env:RAGON_NODE; ConfigPath = $env:DREAMFERENCE_CONFIG_PATH
        InstallDir = $InstallDir; AppDir = $AppDir; Programs = ($Programs -join "|"); Log = $Log
    }
    $Arguments = "-NoProfile -ExecutionPolicy Bypass -File `"$Script`""
    foreach ($Key in $Pass.Keys) {
        $Value = [string]$Pass[$Key]
        if ($Value.EndsWith("\")) { $Value += "\" }
        if ($Value) { $Arguments += " -$Key `"$Value`"" }
    }
    $Principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
    $NoDesktop = $false
    Say ""
    try {
        if ($Principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
            Say "Setting up the Windows sandbox and the firewall rule (already running as Administrator)..."
            Start-Process -FilePath "powershell.exe" -ArgumentList $Arguments -Wait -NoNewWindow
        } elseif (-not [Environment]::UserInteractive) {
            # Over SSH, or from a service, the session has no desktop (session 0, a window station
            # that is not visible): Windows cannot show the Administrator prompt there, and the
            # attempt fails with "requires an interactive window station", which reads as a decline.
            $NoDesktop = $true
            Say "Skipped the Administrator step: this session (SSH, or a service) has no desktop to show the prompt on."
        } else {
            Say "Windows will ask once for Administrator rights: to set up the sandbox that runs the agent's"
            Say "commands, to let ragon find nodes on Private networks, and to run the egress audit."
            Start-Process -FilePath "powershell.exe" -ArgumentList $Arguments -Verb RunAs -Wait -WindowStyle Hidden
        }
    } catch {
        Say "The Administrator prompt was declined or failed ($($_.Exception.Message))."
    }
    $Lines = @()
    if (Test-Path $Log) { $Lines = @(Get-Content -Path $Log -Encoding UTF8) }
    Remove-Item -Recurse -Force -Path $Step -ErrorAction SilentlyContinue
    if ($Lines -contains "sandbox-setup-exit=0") {
        Say "Windows sandbox: set up for $User."
    } elseif (Test-SandboxSetUp $CodexHome) {
        # A reinstall: the setup of an earlier install is still in place, and still what Codex uses.
        Say "Windows sandbox: set up for $User (by an earlier install; still in place)."
    } else {
        Say "Windows sandbox: NOT set up. Commands will run without one, and /airgapped on is refused."
        foreach ($Line in $Lines) { if ($Line -notmatch "^(firewall|audit-exit)=" -and $Line -notmatch "^sandbox-setup-exit=") { Say "  $Line" } }
        if ($NoDesktop) {
            Say "  The sandbox setup needs a desktop session. At this PC's screen, run install.ps1 again and accept"
            Say "  the Administrator prompt; or ask an administrator to run, in an Administrator terminal:"
            Say "    & `"$Ragon`" sandbox setup --elevated --user `"$User`" --codex-home `"$CodexHome`""
        }
    }
    if ($Lines -contains "firewall=ok") {
        Say "Firewall: ragon may hear node announcements (UDP 5353) on Private and Domain networks."
    } elseif ($Lines.Count -gt 0) {
        Say "Firewall: the mDNS rule was not added; discovery may find nothing (ragon node use <address> still works)."
    } elseif ($NoDesktop) {
        Say "Firewall: the mDNS rule needs the same Administrator step; until then ragon finds nodes by asking"
        Say "  (ragon node list), but does not hear their announcements."
    }
    $AuditExit = $Lines | Where-Object { $_ -match "^audit-exit=" } | Select-Object -First 1
    if ($AuditExit) {
        Say "Egress audit:"
        $InAudit = $false
        foreach ($Line in $Lines) {
            if ($Line -match "^firewall=") { $InAudit = $true; continue }
            if ($Line -match "^audit-exit=") { break }
            if ($InAudit) { Say "  $Line" }
        }
        if ($AuditExit -ne "audit-exit=0") {
            Say "  (Run it again once ragon uses a node: ragon audit egress, in an Administrator terminal; as another"
            Say "  administrator, set CODEX_HOME=$CodexHome in that terminal first, so it judges your node.)"
        }
    }
}

if ($Role -eq "node") {
    Say ""
    Say "Installing the node under $($NodePaths.Root) (state under $($NodePaths.State))"
    try {
        $Binaries = @($Ready | ForEach-Object { Join-Path $InstallDir "$_.exe" })
        $Healthy = Install-RagonNode $NodePaths $Tag $Binaries $NodeEngineZip $NodeTools $NodeHooks @() $EngineArgs $NodeSshdConfig $Port 60 $NodeExpect $NodeTrustedAlso
    } finally {
        if ($NodeEngineZip) { Remove-Item -Force -Path $NodeEngineZip -ErrorAction SilentlyContinue }
    }
    if ($Healthy) { Say "The ragon-node service is running and healthy." }
    else { Say "The ragon-node service is installed but not healthy yet: ragon server status --name ragon-node" }
    Say "Weights: ragon model pull, or sync-model from another node, into $($NodePaths.Models)."
    # C:\Ragon\bin stays on the machine PATH: the job account has no profile and its first pairing
    # login runs plain `ragon`. The machine PATH comes before the user's, so new shells here find
    # the node's copy, which refuses `ragon update`.
    Say "On this PC, ragon in a new shell is the node's copy ($($NodePaths.LinkBin)); update with the client's own:"
    Say "  & `"$InstallDir\ragon.exe`" update   (elevated; it updates the client and the node)"
}

Say ""
Say "Done. Ragon $Tag is in $InstallDir."
if ($AppResult -eq "ok") {
    Say "The desktop app is installed: open Ragon from the Start menu."
} elseif ($AppResult) {
    Say "The desktop app was NOT installed ($AppResult). If Windows blocked it, Smart App Control is on."
}
Say ""
Say "PREVIEW: these Windows builds are not signed. If Windows blocks ragon.exe, Smart App Control"
Say "is on; turn it off in Windows Security > App & browser control > Smart App Control settings."
Say ""
Say "Next:"
Say "  ragon node list          # the Ragon nodes (DGX Spark or another GB10) on your network"
Say "  cd <your project>; ragon # start working; it finds the node by itself"
Say "  ragon node use <address> # if your network is marked Public and discovery finds nothing"
