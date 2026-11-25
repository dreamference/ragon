//! The firewall rules of an advertising node, made by `server install` (elevated) and removed by
//! `server uninstall`: UDP 5353 for the program that runs the responder (the supervisor, so the
//! mDNS queries reach it) and TCP for the model server's port for the engine program, on the
//! Private and Domain profiles only. They are the supervisor's own, named
//! `Ragon-Node-<service>-mDNS` and `Ragon-Node-<service>-TCP`, so they never touch the
//! installer's per-user client rules (`Ragon-mDNS-*`, install.ps1). The scripts are plain text,
//! built and tested everywhere; only [`apply`] and [`remove`] run PowerShell, and only on Windows.

/// A single-quoted PowerShell string.
fn ps(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

pub fn mdns_rule_name(service: &str) -> String {
    format!("Ragon-Node-{service}-mDNS")
}

pub fn tcp_rule_name(service: &str) -> String {
    format!("Ragon-Node-{service}-TCP")
}

/// The script that (re)creates the two rules.
pub fn install_script(service: &str, supervisor: &str, engine: &str, port: u16) -> String {
    let rule = |name: String, display: String, proto: &str, local_port: u16, program: &str| {
        format!(
            "Get-NetFirewallRule -Name {n} -ErrorAction SilentlyContinue | Remove-NetFirewallRule; \
             New-NetFirewallRule -Name {n} -DisplayName {d} -Direction Inbound -Action Allow -Protocol {proto} \
             -LocalPort {local_port} -Program {p} -Profile Private,Domain -ErrorAction Stop | Out-Null",
            n = ps(&name),
            d = ps(&display),
            p = ps(program),
        )
    };
    format!(
        "$ErrorActionPreference = 'Stop'; {}; {}",
        rule(mdns_rule_name(service), format!("Ragon node discovery (mDNS, {service})"), "UDP", 5353, supervisor),
        rule(tcp_rule_name(service), format!("Ragon node model server (TCP {port}, {service})"), "TCP", port, engine),
    )
}

/// The script that removes them (no error when they are not there).
pub fn remove_script(service: &str) -> String {
    format!(
        "Get-NetFirewallRule -Name {},{} -ErrorAction SilentlyContinue | Remove-NetFirewallRule",
        ps(&mdns_rule_name(service)),
        ps(&tcp_rule_name(service))
    )
}

#[cfg(windows)]
fn powershell(script: &str) -> Result<(), String> {
    let out = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script])
        .output()
        .map_err(|e| format!("powershell: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("firewall rules: {}", String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// Creates the rules (administrator).
#[cfg(windows)]
pub fn apply(service: &str, supervisor: &str, engine: &str, port: u16) -> Result<(), String> {
    powershell(&install_script(service, supervisor, engine, port))
}

/// Removes the rules (administrator).
#[cfg(windows)]
pub fn remove(service: &str) -> Result<(), String> {
    powershell(&remove_script(service))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rules_are_private_and_domain_only_and_name_their_program_and_port() {
        let s = install_script("ragon-node", r"C:\Ragon\ragon.exe", r"C:\Ragon\ragon-serve.exe", 8000);
        assert!(s.contains("-Profile Private,Domain") && !s.contains("Public"));
        assert!(s.contains("-Protocol UDP -LocalPort 5353 -Program 'C:\\Ragon\\ragon.exe'"));
        assert!(s.contains("-Protocol TCP -LocalPort 8000 -Program 'C:\\Ragon\\ragon-serve.exe'"));
        assert!(s.contains("'Ragon-Node-ragon-node-mDNS'") && s.contains("'Ragon-Node-ragon-node-TCP'"));
    }

    #[test]
    fn a_quote_in_a_path_cannot_end_the_string() {
        assert_eq!(ps("C:\\o'brien\\x.exe"), "'C:\\o''brien\\x.exe'");
    }

    #[test]
    fn removal_names_only_its_own_rules() {
        let s = remove_script("n");
        assert!(s.contains("'Ragon-Node-n-mDNS','Ragon-Node-n-TCP'") && !s.contains("Ragon-mDNS-"));
    }
}
