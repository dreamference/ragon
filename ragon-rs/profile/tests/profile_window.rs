//! The §5.1 window against a fake server and fake commands (shell one-liners that log their name).
use ragon_profile::window::{self, Refusal, WindowConfig};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// The interrupt flag is process-wide, so the tests take turns.
static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());
static N: AtomicUsize = AtomicUsize::new(0);

fn tmp() -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "ragon-window-test-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn server(running: u32) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap().to_string();
    std::thread::spawn(move || {
        for c in l.incoming() {
            let Ok(mut c) = c else { continue };
            std::thread::spawn(move || {
                let mut buf = [0u8; 2048];
                let n = c.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let body = if req.contains("/v1/models") {
                    "{\"data\":[{\"id\":\"prod-model\"}]}".to_string()
                } else {
                    format!(
                        "sglang:num_running_reqs {running}\nsglang:num_queue_reqs 0\nvllm:num_requests_running {running}\nvllm:num_requests_waiting 0\n"
                    )
                };
                write!(
                    c,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .ok();
            });
        }
    });
    format!("http://{addr}")
}

fn sh(script: &str) -> Vec<String> {
    vec!["sh".into(), "-c".into(), script.into()]
}

fn cfg(dir: &Path, running: u32) -> WindowConfig {
    let calls = dir.join("calls");
    let say = |n: &str, code: i32| sh(&format!("echo {n} >> {}; exit {code}", calls.display()));
    WindowConfig {
        url: server(running),
        model: Some("prod-model".into()),
        lock_path: dir.join("gpu.lock"),
        gate_run: Some(dir.join("run.json")),
        night_lock: Some(dir.join("runner.lock")),
        open_marker: dir.join("window.open"),
        windows_log: dir.join("windows.log"),
        label: "fanout-32".into(),
        units_cmd: sh("true"),
        safety_cmd: say("safety", 0),
        stop_cmd: say("stop", 0),
        config_start_cmd: say("config", 0),
        restore_cmd: say("restore", 0),
        gpu_idle_cmd: sh("true"),
        alert_cmd: say("alert", 0),
        check_port: false,
        timeout: Duration::from_secs(5),
        max_len: window::WINDOW_MAX,
    }
}

fn calls(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("calls"))
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect()
}

fn nothing_stopped(dir: &Path) {
    assert!(!calls(dir).iter().any(|c| c == "stop"), "{:?}", calls(dir));
    assert!(!dir.join("windows.log").exists());
}

#[test]
fn it_refuses_once_for_each_reason_and_stops_nothing() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let d = tmp();
    let mut c = cfg(&d, 0);
    c.units_cmd = sh("echo ragon-swe-run.service loaded active");
    assert!(
        matches!(window::run_window(&c, |_| Ok(())), Err(Refusal::SweUnit(u)) if u == "ragon-swe-run.service")
    );
    nothing_stopped(&d);

    let mut c = cfg(&d, 0);
    std::fs::write(d.join("run.json"), "{}").unwrap();
    assert_eq!(
        window::run_window(&c, |_| Ok(())).err(),
        Some(Refusal::GateHeld)
    );
    nothing_stopped(&d);
    std::fs::remove_file(d.join("run.json")).unwrap();

    let night = std::fs::File::create(d.join("runner.lock")).unwrap();
    night.lock().unwrap();
    assert_eq!(
        window::run_window(&c, |_| Ok(())).err(),
        Some(Refusal::NightRun)
    );
    drop(night);
    nothing_stopped(&d);

    let gpu = std::fs::File::create(d.join("gpu.lock")).unwrap();
    gpu.lock().unwrap();
    assert_eq!(
        window::run_window(&c, |_| Ok(())).err(),
        Some(Refusal::LockHeld)
    );
    drop(gpu);
    nothing_stopped(&d);

    std::fs::write(d.join("window.open"), "x").unwrap();
    assert_eq!(
        window::run_window(&c, |_| Ok(())).err(),
        Some(Refusal::WindowOpen)
    );
    std::fs::remove_file(d.join("window.open")).unwrap();
    nothing_stopped(&d);

    c.safety_cmd = sh("exit 1");
    assert_eq!(
        window::run_window(&c, |_| Ok(())).err(),
        Some(Refusal::Unsafe)
    );
    nothing_stopped(&d);

    let busy = cfg(&d, 2);
    assert!(matches!(
        window::run_window(&busy, |_| Ok(())),
        Err(Refusal::Busy(_))
    ));
    nothing_stopped(&d);
    let msg = format!("{}", Refusal::GateHeld);
    assert_eq!(msg.lines().count(), 1);
}

#[test]
fn a_clean_window_stops_starts_the_configuration_restores_and_logs() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let d = tmp();
    let c = cfg(&d, 0);
    let out = window::run_window(&c, |ctl| {
        ctl.start_config()?;
        assert!(ctl.fits(Duration::from_secs(60)));
        assert!(!ctl.fits(Duration::from_secs(41 * 60)));
        Ok(())
    })
    .unwrap();
    assert_eq!(out.exit_code(), 0);
    assert_eq!(calls(&d), ["safety", "stop", "config", "stop", "restore"]);
    let log = std::fs::read_to_string(d.join("windows.log")).unwrap();
    let lines: Vec<&str> = log.lines().collect();
    assert_eq!(lines.len(), 2);
    let end: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(end["restored_healthy"], true);
    assert_eq!(end["production_stopped"], true);
    assert_eq!(end["label"], "fanout-32");
    assert!(end["start"].as_str().unwrap().ends_with('Z'));
    assert!(!d.join("window.open").exists());
    // the lock is free again
    let f = std::fs::File::create(d.join("gpu.lock")).unwrap();
    assert!(f.try_lock().is_ok());
}

#[test]
fn it_restores_after_an_error_and_after_a_panic() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let d = tmp();
    let out = window::run_window(&cfg(&d, 0), |_| Err("boom".into())).unwrap();
    assert!(out.restored && !out.steps_ok);
    assert_eq!(out.exit_code(), 1);
    assert_eq!(calls(&d).last().unwrap(), "restore");

    let d = tmp();
    let out = window::run_window(&cfg(&d, 0), |_| -> Result<(), String> { panic!("x") }).unwrap();
    assert_eq!(out.exit_code(), 1);
    assert_eq!(calls(&d).last().unwrap(), "restore");
}

#[test]
fn it_restores_after_an_interrupt() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let d = tmp();
    let out = window::run_window(&cfg(&d, 0), |_| {
        window::interrupt();
        Ok(())
    })
    .unwrap();
    assert_eq!(out.exit_code(), 1);
    assert_eq!(calls(&d).last().unwrap(), "restore");
    assert!(out.record["note"].as_str().unwrap().contains("interrupted"));
}

#[test]
fn a_failed_restore_exits_two_logs_not_restored_alerts_and_does_not_retry() {
    let _turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let d = tmp();
    let mut c = cfg(&d, 0);
    c.restore_cmd = sh(&format!(
        "echo restore >> {}; exit 1",
        d.join("calls").display()
    ));
    let out = window::run_window(&c, |_| Ok(())).unwrap();
    assert_eq!(out.exit_code(), 2);
    let log = std::fs::read_to_string(d.join("windows.log")).unwrap();
    assert!(log.contains("not restored"));
    assert!(log.contains("\"restored_healthy\":false"));
    assert_eq!(calls(&d).iter().filter(|c| *c == "restore").count(), 1);
    assert!(calls(&d).contains(&"alert".to_string()));
    let f = std::fs::File::create(d.join("gpu.lock")).unwrap();
    assert!(f.try_lock().is_ok());
}
