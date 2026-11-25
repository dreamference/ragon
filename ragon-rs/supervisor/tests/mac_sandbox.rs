//! The Seatbelt profile on a real Mac (specs/DREAMFERENCE_MAC_NODE.md §4.5): the stand-in job runs
//! under `sandbox-exec` with the profile src/seatbelt.rs generates and probes what it may touch.
//! Run as a normal user, no root needed. The build's target folder must be outside /Users (the
//! profile hides every home, so a stand-in under one could not be started): the drill sets
//! CARGO_TARGET_DIR under /private/tmp.
//!
//! The GPU probe asks Metal for a device; on a Mac without one (a VM) it has nothing to deny and
//! says so.
#![cfg(target_os = "macos")]

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;

use ragon_supervisor::seatbelt::{profile, wrap, JobSandbox};

const STANDIN: &str = env!("CARGO_BIN_EXE_ragon-standin-job");

struct Job {
    dir: PathBuf,
    sandbox: JobSandbox,
}

impl Drop for Job {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn job(name: &str, network: bool) -> Job {
    let dir = PathBuf::from("/private/tmp").join(format!("rsj-sb-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("tree/.git")).unwrap();
    std::fs::create_dir_all(dir.join("tmp")).unwrap();
    let home = PathBuf::from(std::env::var("HOME").unwrap()).canonicalize().unwrap();
    let sandbox = JobSandbox { tree: dir.join("tree"), tmp: dir.join("tmp"), home, readable: Vec::new(), network };
    Job { dir, sandbox }
}

/// Runs the stand-in under the profile; returns (exit code, output).
fn probe(job: &Job, args: &[&str]) -> (i32, String) {
    let path = job.dir.join("sandbox.sb");
    std::fs::write(&path, profile(&job.sandbox)).unwrap();
    let mut command = vec![STANDIN.to_string()];
    command.extend(args.iter().map(|a| a.to_string()));
    let argv = wrap(&path, &command);
    let out = Command::new(&argv[0]).args(&argv[1..]).current_dir(&job.sandbox.tree).output().unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    println!("{args:?} -> {:?}: {}", out.status.code(), text.trim());
    (out.status.code().unwrap_or(-1), text)
}

fn allowed(job: &Job, args: &[&str]) {
    let (code, text) = probe(job, args);
    assert_eq!(code, 0, "{args:?} should be allowed: {text}");
}

/// Denied by the sandbox: EPERM, not a missing file or an unreachable host.
fn denied(job: &Job, args: &[&str]) {
    let (code, text) = probe(job, args);
    assert_eq!(code, 1, "{args:?} should be denied: {text}");
    assert!(text.contains("denied:") && text.contains("Operation not permitted"), "{text}");
}

/// The same probe without the sandbox succeeds, so a denial under it is the profile's doing.
fn works_unsandboxed(args: &[&str]) -> bool {
    Command::new(STANDIN).args(args).output().is_ok_and(|o| o.status.success())
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

#[test]
fn the_job_works_in_its_tree_and_tmp_only() {
    let j = job("tree", false);
    allowed(&j, &["print", "hello"]);
    allowed(&j, &["write", &s(&j.sandbox.tree.join("out.txt"))]);
    allowed(&j, &["read", &s(&j.sandbox.tree.join("out.txt"))]);
    allowed(&j, &["write", &s(&j.sandbox.tmp.join("scratch"))]);
    // Readable system files: the toolchain must work.
    allowed(&j, &["read", "/usr/bin/true"]);
    allowed(&j, &["read", "/private/etc/hosts"]);
    denied(&j, &["write", &s(&j.sandbox.tree.join(".git/config"))]);
    denied(&j, &["write", "/private/tmp/rsj-outside-the-tree"]);
    denied(&j, &["write", "/usr/local/rsj-outside-the-tree"]);
}

#[test]
fn the_homes_and_the_ssh_keys_are_denied() {
    let j = job("home", false);
    let home = j.sandbox.home.clone();
    denied(&j, &["list", &s(&home)]);
    denied(&j, &["list", "/Users"]);
    let ssh = home.join(".ssh");
    if ssh.exists() {
        denied(&j, &["list", &s(&ssh)]);
        for f in ["known_hosts", "config", "authorized_keys", "id_ed25519", "id_ed25519.pub", "id_rsa"] {
            if ssh.join(f).exists() {
                denied(&j, &["read", &s(&ssh.join(f))]);
            }
        }
    }
    denied(&j, &["write", &s(&home.join("rsj-written-by-a-job"))]);
    assert!(!home.join("rsj-written-by-a-job").exists());
    denied(&j, &["list", "/private/etc/ssh"]);
    denied(&j, &["read", "/private/etc/ssh/ssh_config"]);
    denied(&j, &["read", "/etc/ssh/sshd_config"]);
}

#[test]
fn airgapped_on_denies_the_network_and_name_lookups() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let j = job("airgap", false);
    denied(&j, &["connect", &addr]);
    denied(&j, &["connect", "1.1.1.1:443"]);
    denied(&j, &["connect-unix", "/private/var/run/mDNSResponder"]);
    // A lookup fails without EPERM (the resolver reports "nodename nor servname"), so the
    // control run proves the name resolves outside the sandbox.
    assert!(works_unsandboxed(&["resolve", "example.com"]), "no name resolution on this box: the probe proves nothing");
    let (code, text) = probe(&j, &["resolve", "example.com"]);
    assert_eq!(code, 1, "{text}");
    host_sockets_denied(&j);
}

#[test]
fn airgapped_off_allows_the_network() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let j = job("net", true);
    allowed(&j, &["connect", &addr]);
    if works_unsandboxed(&["resolve", "example.com"]) {
        allowed(&j, &["resolve", "example.com"]);
    }
    // The host's control sockets stay closed with the network on.
    host_sockets_denied(&j);
}

/// Every host control socket and launchd listener folder this Mac has, probed only where the
/// unsandboxed stand-in reaches it (so a denial is the profile's).
fn host_sockets_denied(j: &Job) {
    let mut probed = 0;
    for socket in ragon_supervisor::seatbelt::HOST_CONTROL_SOCKETS {
        if works_unsandboxed(&["connect-unix", socket]) {
            denied(j, &["connect-unix", socket]);
            probed += 1;
        }
    }
    for parent in ["/private/tmp", "/private/var/run"] {
        for entry in std::fs::read_dir(parent).unwrap().flatten() {
            if !entry.file_name().to_string_lossy().starts_with("com.apple.launchd.") {
                continue;
            }
            let path = s(&entry.path());
            if works_unsandboxed(&["list", &path]) {
                denied(j, &["list", &path]);
                probed += 1;
            }
        }
    }
    if let Ok(agent) = std::env::var("SSH_AUTH_SOCK") {
        if works_unsandboxed(&["connect-unix", &agent]) {
            denied(j, &["connect-unix", &agent]);
            probed += 1;
        }
    }
    assert!(probed > 0, "no host control socket to probe on this Mac");
}

/// Whether this Mac's GPU is Apple's own (an `AGXAccelerator` in the IOKit registry), the one whose
/// user-client classes the profile names; a VM's paravirtual GPU is denied by the deny default.
fn gpu_is_apple_silicon() -> bool {
    Command::new("/usr/sbin/ioreg")
        .args(["-r", "-c", "IOAccelerator", "-d", "1"])
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("AGXAccelerator"))
}

#[test]
fn the_gpu_user_clients_are_denied() {
    if !works_unsandboxed(&["metal"]) {
        println!("no Metal device on this Mac: nothing for the profile to deny");
        return;
    }
    // The job profile, with the network on (the most it allows): no device.
    let j = job("gpu", true);
    let (code, text) = probe(&j, &["metal"]);
    assert_eq!(code, 1, "a job got a Metal device: {text}");
    // The profile's GPU lines alone over (allow default), so the denial is theirs and not the deny
    // default's; (allow default) without them must keep the device.
    let gpu_lines: String = profile(&j.sandbox)
        .lines()
        .filter(|l| l.contains("iokit-user-client-class"))
        .map(|l| format!("{l}\n"))
        .collect();
    assert!(!gpu_lines.is_empty());
    let run = |extra: &str| {
        let path = j.dir.join("gpu.sb");
        std::fs::write(&path, format!("(version 1)\n(allow default)\n{extra}")).unwrap();
        let argv = wrap(&path, &[STANDIN.to_string(), "metal".to_string()]);
        let out = Command::new(&argv[0]).args(&argv[1..]).output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        println!("gpu lines {} -> {:?}: {}", !extra.is_empty(), out.status.code(), text.trim());
        out.status.code().unwrap_or(-1)
    };
    assert_eq!(run(""), 0, "(allow default) alone took the device away: the probe proves nothing");
    if gpu_is_apple_silicon() {
        assert_eq!(run(&gpu_lines), 1, "the GPU user-client lines left a job a Metal device");
    } else {
        println!("not Apple's GPU: the user-client lines are not this GPU's, the deny default covers it");
    }
}
