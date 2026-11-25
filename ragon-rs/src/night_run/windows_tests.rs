//! The night run on Windows itself: the Task Scheduler entry as `schtasks` registers it. The
//! registration is real, so it runs only when asked (`cargo test -- --ignored`), on a box where a
//! `Ragon Night Shift` entry may be created and deleted again.

use std::path::Path;

use super::*;

fn schtasks_ok(args: &[String]) -> String {
    let output = std::process::Command::new("schtasks").args(args).output().expect("schtasks runs");
    let text = String::from_utf8_lossy(&output.stdout).to_string() + &String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "schtasks {args:?}: {text}");
    text
}

/// Deletes the entry when the test ends, whichever way it ends.
struct Unregister;

impl Drop for Unregister {
    fn drop(&mut self) {
        let _ = std::process::Command::new("schtasks").args(schtasks_delete()).output();
    }
}

#[test]
#[ignore = "registers a real Task Scheduler entry"]
fn the_schedule_registers_and_unregisters_with_task_scheduler() {
    let exe = Path::new(r"C:\Program Files\Ragon\ragon.exe");
    let entry = Unregister;
    schtasks_ok(&schtasks_create(exe, "01:00"));
    let query = ["/Query", "/TN", SCHEDULED_TASK, "/XML"].map(String::from);
    let xml = schtasks_ok(&query);
    schtasks_ok(&schtasks_delete());
    std::mem::forget(entry);
    // The quotes `/TR` needs around a path with spaces stay in the stored command.
    assert!(xml.contains(r#"<Command>"C:\Program Files\Ragon\ragon.exe"</Command>"#), "{xml}");
    assert!(xml.contains("<Arguments>night run</Arguments>"), "{xml}");
    assert!(xml.contains("T01:00:00</StartBoundary>"), "{xml}");
    assert!(xml.contains("<DaysInterval>1</DaysInterval>"), "{xml}");
    // `/RL LIMITED` is the default run level, which the stored XML leaves out.
    assert!(!xml.contains("HighestAvailable"), "{xml}");
    let gone = std::process::Command::new("schtasks").args(&query).output().expect("schtasks runs");
    assert!(!gone.status.success(), "the entry is gone after the delete");
}
