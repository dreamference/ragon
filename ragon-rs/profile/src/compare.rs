//! `ragon profile compare <a> <b>` and `baseline [set|show]` (specs/DREAMFERENCE_PROFILING.md O5, §7.1, §7.2).
//! Runs are read from `<root>/runs/<name>/result.json`; a baseline is a pointer file
//! `<root>/baselines/<node>--<shape>` holding the run directory's name. Nothing here runs a model.
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Noise floor and the least difference that can count, in percent (O5).
pub const NOISE_FLOOR_PCT: f64 = 0.5;
pub const MIN_REGRESSION_PCT: f64 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    NoChange,
    Regression,
    Faster,
    Unclear,
}

impl Verdict {
    pub fn label(self) -> &'static str {
        match self {
            Verdict::NoChange => "no change",
            Verdict::Regression => "regression",
            Verdict::Faster => "faster",
            Verdict::Unclear => "unclear: one more ABBA pair",
        }
    }
}

/// `worse_pct` is positive when the candidate is worse than the base. Noise is the larger of the two
/// spreads, at least 0.5 %; a difference counts when it exceeds max(2 × noise, 2 %).
pub fn judge(worse_pct: f64, spread_a: f64, spread_b: f64) -> (Verdict, f64) {
    let noise = spread_a.max(spread_b).max(NOISE_FLOOR_PCT);
    let bar = (2.0 * noise).max(MIN_REGRESSION_PCT);
    let v = if worse_pct.abs() <= noise {
        Verdict::NoChange
    } else if worse_pct > bar {
        Verdict::Regression
    } else if -worse_pct > bar {
        Verdict::Faster
    } else {
        Verdict::Unclear
    };
    (v, noise)
}

#[derive(Debug, Clone)]
pub struct Line {
    pub name: String,
    pub base: f64,
    pub cand: f64,
    pub worse_pct: f64,
    pub noise_pct: f64,
    pub verdict: Verdict,
}

fn figure(name: &str, a: &Value, b: &Value, higher_better: bool) -> Option<Line> {
    // `a`/`b` are {median, spread_pct} objects, or plain numbers (no spread recorded: the floor applies).
    let get = |v: &Value| -> Option<(f64, f64)> {
        match v.get("median") {
            Some(m) => Some((m.as_f64()?, v["spread_pct"].as_f64().unwrap_or(0.0))),
            None => Some((v.as_f64()?, 0.0)),
        }
    };
    let ((base, sa), (cand, sb)) = (get(a)?, get(b)?);
    if base == 0.0 {
        return None;
    }
    let delta = (cand - base) / base * 100.0;
    let worse = if higher_better { -delta } else { delta };
    let (verdict, noise_pct) = judge(worse, sa, sb);
    Some(Line {
        name: name.into(),
        base,
        cand,
        worse_pct: worse,
        noise_pct,
        verdict,
    })
}

/// The decisive figures of §7.2 that this build measures: decode and TTFT for one-stream, span tok/s
/// for the concurrent shape.
pub fn lines(a: &Value, b: &Value) -> Vec<Line> {
    let (fa, fb) = (&a["figures"], &b["figures"]);
    let mut out = vec![];
    if fa.get("aggregate").is_some() || fb.get("aggregate").is_some() {
        out.extend(figure(
            "span tok/s",
            &fa["aggregate"]["span_tok_s"],
            &fb["aggregate"]["span_tok_s"],
            true,
        ));
    } else {
        out.extend(figure(
            "decode tok/s",
            &fa["all"]["decode_tok_s"],
            &fb["all"]["decode_tok_s"],
            true,
        ));
        for k in ["prose", "code", "json", "long"] {
            out.extend(figure(
                &format!("TTFT s ({k})"),
                &fa[k]["ttft_s"],
                &fb[k]["ttft_s"],
                false,
            ));
        }
    }
    out
}

pub struct Outcome {
    pub verdict: Verdict,
    pub lines: Vec<Line>,
    pub text: String,
}

fn state_of(r: &Value) -> &str {
    r["state"].as_str().unwrap_or("unknown")
}

/// Compares two parsed results. `Err` when they are not comparable (different shape, workload or node).
pub fn compare(a: &Value, b: &Value, a_name: &str, b_name: &str) -> Result<Outcome, String> {
    for (what, x, y) in [
        ("shape", &a["shape"]["name"], &b["shape"]["name"]),
        ("workload", &a["workload_sha256"], &b["workload_sha256"]),
        ("node", &a["machine"]["node"], &b["machine"]["node"]),
    ] {
        if x != y {
            return Err(format!("❌ Not comparable: the {what} differs ({x} vs {y})."));
        }
    }
    let shape = a["shape"]["name"].as_str().unwrap_or("n/a");
    for (n, r) in [(a_name, a), (b_name, b)] {
        if state_of(r) != "clean" {
            let text = format!(
                "no verdict: {n} has state \"{}\" (only clean runs are judged, §7.2)",
                state_of(r)
            );
            return Ok(Outcome {
                verdict: Verdict::Unclear,
                lines: vec![],
                text,
            });
        }
    }
    let ls = lines(a, b);
    if ls.is_empty() {
        return Err("❌ No decisive figure found in both results.".into());
    }
    let any = |v: Verdict| ls.iter().any(|l| l.verdict == v);
    let verdict = if any(Verdict::Regression) {
        Verdict::Regression
    } else if any(Verdict::Unclear) {
        Verdict::Unclear
    } else if any(Verdict::Faster) {
        Verdict::Faster
    } else {
        Verdict::NoChange
    };
    let mut text = format!("base {a_name}  candidate {b_name}  shape {shape}\n");
    for l in &ls {
        text += &format!(
            "{}: {:.3} -> {:.3}, {:+.1}% {}, noise {:.1}%: {}\n",
            l.name,
            l.base,
            l.cand,
            l.worse_pct.abs(),
            if l.worse_pct >= 0.0 { "worse" } else { "better" },
            l.noise_pct,
            l.verdict.label()
        );
    }
    text += &format!("verdict: {}\n", verdict.label());
    if verdict == Verdict::Unclear {
        text += &format!(
            "next: run one more ABBA pair of this shape on the same node, base then candidate then candidate then base:\n  ragon profile run {shape}   (base build, A)\n  ragon profile run {shape}   (candidate build, B)\n  ragon profile run {shape}   (candidate build, B)\n  ragon profile run {shape}   (base build, A)\nthen compare the new pair. If it is still unclear the result is \"slower (or faster), within 2%\": not a regression and not a gain to claim.\n"
        );
    }
    Ok(Outcome {
        verdict,
        lines: ls,
        text,
    })
}

fn runs_dir(root: &Path) -> PathBuf {
    root.join("runs")
}

fn baseline_path(root: &Path, node: &str, shape: &str) -> PathBuf {
    root.join("baselines").join(format!("{node}--{shape}"))
}

/// A run named by directory name or by path (a directory or its result.json).
fn locate(root: &Path, arg: &str) -> PathBuf {
    let p = Path::new(arg);
    let dir = if root.join("runs").join(arg).is_dir() {
        runs_dir(root).join(arg)
    } else if p.file_name().is_some_and(|f| f == "result.json") {
        p.parent().unwrap_or(p).to_path_buf()
    } else {
        p.to_path_buf()
    };
    dir.join("result.json")
}

fn load(root: &Path, arg: &str) -> Result<(Value, String), String> {
    let f = locate(root, arg);
    let text = std::fs::read_to_string(&f).map_err(|e| format!("❌ Cannot read {arg}: {e}"))?;
    let v: Value = serde_json::from_str(&text).map_err(|e| format!("❌ {arg} is not a result: {e}"))?;
    let name = f
        .parent()
        .and_then(|d| d.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| arg.to_string());
    Ok((v, name))
}

/// The run the baseline of this node and shape points at, by directory name.
pub fn baseline_get(root: &Path, node: &str, shape: &str) -> Option<String> {
    let s = std::fs::read_to_string(baseline_path(root, node, shape)).ok()?;
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

/// Records a clean run as the baseline of its node and shape (§7.1).
pub fn baseline_set(root: &Path, arg: &str) -> Result<String, String> {
    let (r, name) = load(root, arg)?;
    if state_of(&r) != "clean" {
        return Err(format!(
            "❌ A baseline is only set from a clean run; {name} is \"{}\".",
            state_of(&r)
        ));
    }
    let node = r["machine"]["node"].as_str().unwrap_or("unknown");
    let shape = r["shape"]["name"].as_str().unwrap_or("run");
    if !runs_dir(root).join(&name).join("result.json").is_file() {
        return Err(format!("❌ {name} is not in the results directory {}.", runs_dir(root).display()));
    }
    let path = baseline_path(root, node, shape);
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| format!("❌ {e}"))?;
    std::fs::write(&path, format!("{name}\n")).map_err(|e| format!("❌ {e}"))?;
    Ok(format!("baseline for {node} / {shape}: {name}"))
}

pub fn baseline_show(root: &Path, node: &str, shape: Option<&str>) -> String {
    let shapes: Vec<&str> = match shape {
        Some(s) => vec![s],
        None => vec!["one-stream", "four-streams"],
    };
    let mut out = String::new();
    for s in shapes {
        match baseline_get(root, node, s) {
            Some(n) => out += &format!("{node} / {s}: {n}\n"),
            None => out += &format!("{node} / {s}: none\n"),
        }
    }
    out
}

const USAGE: &str = "usage: ragon profile compare <base> <candidate> [--out DIR]   (a name under runs/, a run folder, or `baseline`)\n       ragon profile baseline set <run> [--out DIR]\n       ragon profile baseline show [--node ID] [--shape SHAPE] [--out DIR]";

/// `compare …` and `baseline …`; exit 0 for no change / faster / a baseline written, 1 for a regression
/// or an error, 2 for unclear (one more ABBA pair).
pub fn run(args: &[String]) -> i32 {
    let mut pos: Vec<&str> = vec![];
    let mut out = crate::output::default_root();
    let (mut node, mut shape) = (std::env::var("RAGON_NODE_ID").unwrap_or_else(|_| "unknown".into()), None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = || it.next().cloned();
        match a.as_str() {
            "--out" => out = val().map(PathBuf::from),
            "--node" => match val() {
                Some(v) => node = v,
                None => return usage(),
            },
            "--shape" => shape = val(),
            s if s.starts_with("--") => return usage(),
            s => pos.push(s),
        }
    }
    let Some(root) = out else {
        eprintln!("❌ No home directory to keep results in; pass --out DIR.");
        return 1;
    };
    match pos.as_slice() {
        ["compare", a, b] => {
            let resolve = |x: &str, other: &str| -> Result<(Value, String), String> {
                if x != "baseline" {
                    return load(&root, x);
                }
                let (o, _) = load(&root, other)?;
                let (n, s) = (
                    o["machine"]["node"].as_str().unwrap_or("unknown").to_string(),
                    o["shape"]["name"].as_str().unwrap_or("run").to_string(),
                );
                let name = baseline_get(&root, &n, &s)
                    .ok_or_else(|| format!("❌ No baseline for {n} / {s}; set one with `baseline set <run>`."))?;
                load(&root, &name)
            };
            let res = resolve(a, b).and_then(|x| resolve(b, a).map(|y| (x, y)));
            match res.and_then(|((ra, na), (rb, nb))| compare(&ra, &rb, &na, &nb)) {
                Ok(o) => {
                    print!("{}", crate::output::normalise(&o.text));
                    match o.verdict {
                        Verdict::Regression => 1,
                        Verdict::Unclear => 2,
                        _ => 0,
                    }
                }
                Err(e) => {
                    eprintln!("{e}");
                    1
                }
            }
        }
        ["baseline", "set", r] => match baseline_set(&root, r) {
            Ok(t) => {
                println!("{t}");
                0
            }
            Err(e) => {
                eprintln!("{e}");
                1
            }
        },
        ["baseline"] | ["baseline", "show"] => {
            print!("{}", baseline_show(&root, &node, shape.as_deref()));
            0
        }
        _ => usage(),
    }
}

fn usage() -> i32 {
    eprintln!("{USAGE}");
    1
}
