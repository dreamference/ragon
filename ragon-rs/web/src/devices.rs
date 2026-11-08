//! The Devices page (specs/DREAMFERENCE_RAGON_ASK.md §4.3, "Settings → Devices"): paired
//! devices with a Revoke button, and a pairing code with a QR code a phone scans to open the
//! pairing page on this server, prefilled.
//!
//! Only this machine's own session may use it: a session comes from `ragon web open`, whose link is
//! for loopback, so a paired device cannot mint more devices or revoke others. What the QR code
//! carries is the pairing link (qr.rs): the server's address and a code that works once, for ten
//! minutes. Served by the server itself, with no script: the forms post back to it, so the page
//! needs nothing beyond the CSP every page has.

use crate::auth::Device;
use crate::qr;

const STYLE: &str = r#"<style>
main { max-width: 560px; }
.qr { background: #ffffff; padding: 8px; border-radius: 8px; width: min(320px, 100%); margin: 0 0 0.75rem; }
.qr svg { display: block; width: 100%; height: auto; }
.code { font-size: 2rem; font-weight: 700; letter-spacing: 0.15em; font-variant-numeric: tabular-nums; margin: 0 0 0.25rem; color: var(--fg); }
.link { word-break: break-all; font-size: 0.9rem; }
ul { list-style: none; padding: 0; margin: 0 0 1.5rem; }
li { display: flex; align-items: center; gap: 0.75rem; padding: 0.5rem 0; border-bottom: 1px solid var(--field); }
li span { flex: 1; min-width: 0; }
li small { display: block; color: var(--muted); }
pre { white-space: pre-wrap; word-break: break-all; font-size: 0.75rem; }
li form button { width: auto; padding: 0.4rem 0.8rem; background: var(--field); color: var(--fg); }
details { margin: 0 0 1.5rem; }
details .qr { width: min(220px, 100%); }
section { margin: 0 0 2rem; }
h2 { font-size: 1.1rem; margin: 0 0 0.5rem; }
a { color: var(--accent); }
</style>"#;

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

pub(crate) fn page(title: &str, body: &str) -> String {
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
<title>{}</title>\n<link rel=\"stylesheet\" href=\"/pair.css\">\n{STYLE}\n</head>\n<body>\n<main>\n{body}\n</main>\n</body>\n</html>\n",
        escape(title)
    )
}

/// How long ago a time was, for people.
pub fn ago(seconds: u64, now: u64) -> String {
    let elapsed = now.saturating_sub(seconds);
    match elapsed {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{} min ago", elapsed / 60),
        3600..=86_399 => format!("{} h ago", elapsed / 3600),
        _ => format!("{} days ago", elapsed / 86_400),
    }
}

/// A pairing code just issued, and where devices reach this server.
pub struct Pairing<'a> {
    pub code: &'a str,
    pub port: u16,
    pub lan_names: &'a [String],
}

fn pairing_section(pairing: &Pairing) -> String {
    let code = pairing.code;
    let spaced = format!("{} {}", &code[..code.len().min(4)], &code[code.len().min(4)..]);
    let names = qr::ordered(pairing.lan_names);
    let Some((first, others)) = names.split_first() else {
        return format!("<section><h2>Pairing code</h2><p class=\"code\">{}</p></section>", escape(&spaced));
    };
    let label = |name: &str| {
        if qr::is_tailscale(name) { format!("{} (Tailscale)", escape(name)) } else { escape(name) }
    };
    let link = qr::pairing_link(first, pairing.port, code);
    let image = qr::svg(&link).unwrap_or_default();
    let mut html = format!(
        "<section><h2>Scan this with the other device</h2>\
<div class=\"qr\">{image}</div>\
<p class=\"code\">{}</p>\
<p>Or open <span class=\"link\">{}</span> on it and type the code. It works once, for ten minutes, for one device; the device stays paired until you revoke it here.</p>\
<p>This code is for {}.</p>",
        escape(&spaced),
        escape(&link.replace(&format!("?code={code}"), "")),
        label(first)
    );
    if !others.is_empty() {
        html.push_str("<details><summary>The other device is on another network? Other addresses of this machine</summary>");
        for name in others {
            let link = qr::pairing_link(name, pairing.port, code);
            html.push_str(&format!(
                "<p>{}</p><div class=\"qr\">{}</div>",
                label(name),
                qr::svg(&link).unwrap_or_default()
            ));
        }
        html.push_str("</details>");
    }
    html.push_str("</section>");
    html
}

/// The page for this machine's own session.
pub fn owner_page(devices: &[Device], lan_names: &[String], pairing: Option<&Pairing>, notice: Option<&str>, now: u64) -> String {
    owner_page_with_phone(devices, lan_names, pairing, notice, now, &Phone::Hidden)
}

/// What the "Phone on the overlay" section shows (REMOTE_ACCESS §4.2).
pub enum Phone<'a> {
    /// This node is not a remote-access peer: the section is not shown.
    Hidden,
    /// Remote access is set up: the steps and a button that mints a one-use setup key.
    Offer,
    /// A key was minted: the QR code and the values it carries.
    Bundle(&'a PhoneBundle),
    /// The key could not be minted; the reason, for the owner.
    Failed(&'a str),
}

/// What `ragon-admin remote code --bundle-file` wrote, the parts a phone needs.
pub struct PhoneBundle {
    pub management_url: String,
    pub setup_key: String,
    pub overlay_name: String,
    /// The node CA (PEM) when the control plane uses it; None when its certificate is public.
    pub ca: Option<String>,
}

impl PhoneBundle {
    /// Reads the bundle's JSON; None when a value the page needs is missing.
    pub fn parse(text: &str) -> Option<PhoneBundle> {
        let value: serde_json::Value = serde_json::from_str(text).ok()?;
        let field = |name: &str| value.get(name).and_then(|v| v.as_str()).map(str::to_string).filter(|v| !v.is_empty());
        Some(PhoneBundle {
            management_url: field("management_url")?,
            setup_key: field("setup_key")?,
            overlay_name: field("overlay_name")?,
            ca: field("ca"),
        })
    }

    /// The text the QR code carries: the management URL and the setup key, one per line.
    fn qr_text(&self) -> String {
        format!("{}\n{}", self.management_url, self.setup_key)
    }
}

fn phone_section(phone: &Phone) -> String {
    let steps = |overlay: &str| {
        format!(
            "<ol>\n<li>Install NetBird's mobile app and Element X.</li>\n\
<li>In NetBird choose <em>Self-hosted</em> and enter the management URL and the setup key shown here.</li>\n\
<li>Once NetBird shows Connected, run <code>ragon-admin matrix start</code> on this machine if Matrix is not running yet.</li>\n\
<li>In Element X choose <em>Change account provider</em> and enter <code>{}</code>, then create the account.</li>\n</ol>\n",
            escape(overlay)
        )
    };
    let section = |inner: &str| format!("<section><h2>Phone on the overlay</h2>\n{inner}</section>\n");
    match phone {
        Phone::Hidden => String::new(),
        Phone::Offer => section(&format!(
            "<p>Enrols a phone in this node's overlay and shows the Matrix steps, with no terminal. The setup key works once, for ten minutes.</p>\n{}\
<form method=\"post\" action=\"/devices/phone\"><button type=\"submit\">Show the phone's QR code</button></form>",
            steps("the node's overlay name")
        )),
        Phone::Failed(reason) => section(&format!("<p class=\"error\">{}</p>", escape(reason))),
        Phone::Bundle(bundle) => {
            let qr = qr::svg(&bundle.qr_text()).unwrap_or_default();
            let ca = match &bundle.ca {
                Some(ca) => format!(
                    "<details><summary>The node's certificate authority</summary><p>Install it on the phone first (iOS: as a profile, then Certificate Trust Settings).</p><pre>{}</pre></details>\n",
                    escape(ca)
                ),
                None => "<p>No certificate to install: the control plane's certificate is public.</p>\n".to_string(),
            };
            section(&format!(
                "<div class=\"qr\">{qr}</div>\n<p class=\"link\">Management URL: <code>{}</code><br>Setup key: <code>{}</code></p>\n{ca}{}",
                escape(&bundle.management_url),
                escape(&bundle.setup_key),
                steps(&bundle.overlay_name)
            ))
        }
    }
}

/// The page for this machine's own session, with the phone section.
pub fn owner_page_with_phone(
    devices: &[Device],
    lan_names: &[String],
    pairing: Option<&Pairing>,
    notice: Option<&str>,
    now: u64,
    phone: &Phone,
) -> String {
    let mut body = String::from("<h1>Devices</h1>\n<p><a href=\"/\">Back to Ragon</a></p>\n");
    if let Some(notice) = notice {
        body.push_str(&format!("<p class=\"error\">{}</p>\n", escape(notice)));
    }
    if lan_names.is_empty() {
        body.push_str(
            "<section><h2>Pair a phone or another device</h2><p>This server answers on loopback only, so no other device can reach it. \
On an advertised node, <code>ragon-admin node enable</code> serves the web UI to the local network (and over Tailscale), \
paired devices only; then come back here.</p></section>\n",
        );
    } else if let Some(pairing) = pairing {
        body.push_str(&pairing_section(pairing));
    } else {
        body.push_str(
            "<section><h2>Pair a phone or another device</h2>\
<p>Shows a QR code the other device scans to open its pairing page here, with a code that works once, for ten minutes.</p>\
<form method=\"post\" action=\"/devices/pair\"><button type=\"submit\">Show a pairing code</button></form></section>\n",
        );
    }
    body.push_str(&phone_section(phone));
    body.push_str("<section><h2>Paired devices</h2>\n");
    if devices.is_empty() {
        body.push_str("<p>None yet.</p>");
    } else {
        body.push_str("<ul>");
        for device in devices {
            // Every pairing may control other devices' threads (SYNC §5.1); one can be narrowed to
            // watching, and widened again.
            let (kind, scope, other, label) = match device.scope {
                crate::auth::Scope::Control => (device.kind, "can control threads", "watch", "Watch only"),
                crate::auth::Scope::Watch => (device.kind, "watches only", "control", "Allow control"),
            };
            let kind = if kind == crate::auth::DeviceKind::Sync { "sync agent, " } else { "" };
            body.push_str(&format!(
                "<li><span>{}<small>{kind}{scope}; paired {}, last used {}</small></span>\
<form method=\"post\" action=\"/devices/scope\"><input type=\"hidden\" name=\"device\" value=\"{}\"><input type=\"hidden\" name=\"scope\" value=\"{other}\"><button type=\"submit\">{label}</button></form>\
<form method=\"post\" action=\"/devices/revoke\"><input type=\"hidden\" name=\"device\" value=\"{}\"><button type=\"submit\">Revoke</button></form></li>",
                escape(&device.name),
                ago(device.created, now),
                ago(device.last_used, now),
                escape(&device.id),
                escape(&device.id)
            ));
        }
        body.push_str("</ul>");
    }
    body.push_str("</section>");
    page("Devices", &body)
}

/// What a paired device sees instead: pairing is done from the Ragon machine itself.
pub fn device_page() -> String {
    page(
        "Devices",
        "<h1>Devices</h1>\n<p>Pair another device, or revoke one, on the Ragon machine itself: open this page there \
(<code>ragon web open</code>), or run <code>ragon web pair</code>.</p>\n<p><a href=\"/\">Back to Ragon</a></p>",
    )
}

/// Whether a code is the pairing kind, the only kind the pairing page may show back.
pub fn is_pairing_code(code: &str) -> bool {
    code.len() == 8 && code.chars().all(|c| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(name: &str) -> Device {
        Device {
            id: "ab12cd34".to_string(),
            name: name.to_string(),
            token_sha256: "secret-hash".to_string(),
            created: 0,
            last_used: 0,
            kind: crate::auth::DeviceKind::Browser,
            scope: crate::auth::Scope::Control,
        }
    }

    #[test]
    fn the_page_escapes_names_and_never_shows_a_token_hash() {
        let page = owner_page(&[device("<script>")], &["192.168.50.105".to_string()], None, None, 100);
        assert!(page.contains("&lt;script&gt;") && !page.contains("<script>"));
        assert!(!page.contains("secret-hash"));
        assert!(page.contains("action=\"/devices/pair\"") && page.contains("action=\"/devices/revoke\""));
    }

    #[test]
    fn a_loopback_server_offers_no_code() {
        let page = owner_page(&[], &[], None, None, 0);
        assert!(page.contains("loopback only") && !page.contains("/devices/pair"));
    }

    #[test]
    fn a_pairing_shows_the_lan_address_first_and_the_others_folded() {
        let names = vec!["172.17.0.1".to_string(), "100.101.102.103".to_string(), "192.168.50.105".to_string()];
        let pairing = Pairing { code: "01234567", port: 3100, lan_names: &names };
        let page = owner_page(&[], &names, Some(&pairing), None, 0);
        assert!(page.contains("0123 4567"));
        let first = page.find("<svg").unwrap();
        let folded = page.find("<details>").unwrap();
        assert!(first < folded);
        assert!(page.contains("This code is for 192.168.50.105."));
        assert!(page.contains("100.101.102.103 (Tailscale)"));
        assert_eq!(page.matches("<svg").count(), 3);
    }

    #[test]
    fn the_phone_section_needs_every_value_and_escapes_them() {
        assert!(PhoneBundle::parse("{\"management_url\":\"https://x\"}").is_none());
        let bundle = PhoneBundle::parse(
            "{\"management_url\":\"https://x\",\"setup_key\":\"<k>\",\"overlay_name\":\"ragon-a.b\"}",
        )
        .unwrap();
        assert!(bundle.ca.is_none());
        let html = phone_section(&Phone::Bundle(&bundle));
        assert!(html.contains("<svg") && html.contains("&lt;k&gt;") && !html.contains("<k>"));
        assert!(html.contains("public") && html.contains("<code>ragon-a.b</code>"));
        assert!(phone_section(&Phone::Hidden).is_empty());
    }

    #[test]
    fn only_eight_digits_count_as_a_pairing_code() {
        assert!(is_pairing_code("01234567"));
        for code in ["0123456", "012345678", "0123456a", "\"><b>", ""] {
            assert!(!is_pairing_code(code), "{code}");
        }
    }
}
