//! Chooser v1 (`night_shift_chooser.py`): the regression filter, then a pairwise judge tournament
//! among the attempts it kept. The pairing order is drawn from Python's `random.Random(task_id)`,
//! reproduced here bit for bit ([`PyRandom`]) so a task pairs its attempts the same way on either
//! runner.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::time::Duration;

use serde_json::Value;
use serde_json::json;
use sha2::Digest;
use sha2::Sha512;

const MAX_PATCH_CHARS: usize = 40_000;
const MAX_ISSUE_CHARS: usize = 20_000;
const MIN_COMPARISONS: u32 = 2;
const SWISS_WINDOW: usize = 8;
const WEIGHT_FLOOR: f64 = 0.1;
const JUDGE_MAX_TOKENS: u32 = 16_384;
const JUDGE_TEMPERATURE: f64 = 0.6;
const JUDGE_TOP_P: f64 = 0.95;
const SYSTEM_PROMPT: &str = "You are an expert code reviewer. Compare two solutions to a programming problem and rate their correctness. The problem is a GitHub issue in a Python repository; each solution is a patch (a diff) written to fix it. Neither patch has been run. A correct patch fixes the behaviour the issue describes and breaks no existing behaviour. If both look correct, prefer the more robust and fault-tolerant one. Ignore style, comments and the tests a patch adds.";
const USER_TEMPLATE: &str = "<issue>\n{issue}\n</issue>\n\n<solution_A>\n{a}\n</solution_A>\n\n<solution_B>\n{b}\n</solution_B>\n\nRate the correctness of each solution from 1 (wrong) to 10 (certainly correct). End your answer with exactly these two lines:\nRATING_A: <1-10>\nRATING_B: <1-10>";

/// One attempt the chooser weighs (`chooser_candidate.py`).
#[derive(Clone, Debug)]
pub struct Candidate {
    pub id: String,
    pub patch: String,
    pub applied: bool,
    pub p2p_failed: Option<u32>,
}

impl Candidate {
    pub fn breaks(&self) -> bool {
        !self.applied || self.p2p_failed.is_some_and(|failed| failed > 0)
    }
}

/// A pairwise judgement: the two ratings, or `None` for no judgement (a tie at the floor weight).
pub type Compare<'a> = dyn FnMut(&Candidate, &Candidate) -> Option<(f64, f64)> + 'a;

/// What [`choose`] decided.
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    pub stage: String,
    pub survivors: Vec<String>,
    /// Points by id, in id order (Python's `dict.fromkeys(sorted(ids))`).
    pub points: Vec<(String, f64)>,
    pub chosen: Vec<String>,
}

impl Choice {
    pub fn points_json(&self) -> Value {
        let mut map = serde_json::Map::new();
        for (id, points) in &self.points {
            map.insert(id.clone(), json!(points));
        }
        Value::Object(map)
    }
}

/// Python's `random.Random(seed_string)`: MT19937 seeded by `init_by_array` with the 32-bit words
/// of `int.from_bytes(seed + sha512(seed), "big")`, least significant first; `random()` is the
/// 53-bit draw of `_randommodule.c`.
pub struct PyRandom {
    state: [u32; 624],
    index: usize,
}

impl PyRandom {
    pub fn from_str_seed(seed: &str) -> Self {
        let mut bytes = seed.as_bytes().to_vec();
        bytes.extend_from_slice(&Sha512::digest(seed.as_bytes()));
        let mut words = Vec::new();
        let mut end = bytes.len();
        while end > 0 {
            let start = end.saturating_sub(4);
            let mut word = 0u32;
            for byte in &bytes[start..end] {
                word = (word << 8) | u32::from(*byte);
            }
            words.push(word);
            end = start;
        }
        while words.len() > 1 && words.last() == Some(&0) {
            words.pop();
        }
        if words.is_empty() {
            words.push(0);
        }
        Self::from_key(&words)
    }

    fn from_key(key: &[u32]) -> Self {
        const N: usize = 624;
        let mut mt = [0u32; N];
        mt[0] = 19_650_218;
        for i in 1..N {
            mt[i] = 1_812_433_253u32.wrapping_mul(mt[i - 1] ^ (mt[i - 1] >> 30)).wrapping_add(i as u32);
        }
        let (mut i, mut j) = (1usize, 0usize);
        for _ in 0..N.max(key.len()) {
            mt[i] = (mt[i] ^ (mt[i - 1] ^ (mt[i - 1] >> 30)).wrapping_mul(1_664_525))
                .wrapping_add(key[j])
                .wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i >= N {
                mt[0] = mt[N - 1];
                i = 1;
            }
            if j >= key.len() {
                j = 0;
            }
        }
        for _ in 0..N - 1 {
            mt[i] = (mt[i] ^ (mt[i - 1] ^ (mt[i - 1] >> 30)).wrapping_mul(1_566_083_941)).wrapping_sub(i as u32);
            i += 1;
            if i >= N {
                mt[0] = mt[N - 1];
                i = 1;
            }
        }
        mt[0] = 0x8000_0000;
        PyRandom { state: mt, index: N }
    }

    fn next_u32(&mut self) -> u32 {
        const N: usize = 624;
        const M: usize = 397;
        if self.index >= N {
            for k in 0..N {
                let y = (self.state[k] & 0x8000_0000) | (self.state[(k + 1) % N] & 0x7fff_ffff);
                let mut next = self.state[(k + M) % N] ^ (y >> 1);
                if y & 1 != 0 {
                    next ^= 0x9908_b0df;
                }
                self.state[k] = next;
            }
            self.index = 0;
        }
        let mut y = self.state[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^ (y >> 18)
    }

    pub fn random(&mut self) -> f64 {
        let a = f64::from(self.next_u32() >> 5);
        let b = f64::from(self.next_u32() >> 6);
        (a * 67_108_864.0 + b) * (1.0 / 9_007_199_254_740_992.0)
    }
}

pub fn clip(text: &str, limit: usize) -> String {
    let count = text.chars().count();
    if count <= limit {
        return text.to_string();
    }
    let head: String = text.chars().take(limit).collect();
    format!("{head}\n[... cut: {} more characters]", count - limit)
}

/// `RATING_A: n` and `RATING_B: n` from a judge's answer, clamped to 1..=10; the last of each wins.
pub fn ratings(answer: &str) -> Option<(f64, f64)> {
    let pattern = regex_lite::Regex::new(r"RATING_([AB]):\s*\**\s*(\d+(?:\.\d+)?)").ok()?;
    let (mut a, mut b) = (None, None);
    for found in pattern.captures_iter(answer) {
        let value: f64 = found[2].parse().ok()?;
        let value = value.clamp(1.0, 10.0);
        if &found[1] == "A" {
            a = Some(value);
        } else {
            b = Some(value);
        }
    }
    Some((a?, b?))
}

pub fn messages(issue: &str, a: &str, b: &str) -> Value {
    let user = USER_TEMPLATE
        .replace("{issue}", &clip(issue, MAX_ISSUE_CHARS))
        .replace("{a}", &clip(a, MAX_PATCH_CHARS))
        .replace("{b}", &clip(b, MAX_PATCH_CHARS));
    json!([{"role": "system", "content": SYSTEM_PROMPT}, {"role": "user", "content": user}])
}

fn pair_key(a: &str, b: &str) -> (String, String) {
    if a <= b { (a.to_string(), b.to_string()) } else { (b.to_string(), a.to_string()) }
}

pub fn pairings(
    ids: &[String],
    scores: &BTreeMap<String, f64>,
    played: &BTreeSet<(String, String)>,
    counts: &BTreeMap<String, u32>,
    rng: &mut PyRandom,
) -> Vec<(String, String)> {
    let unplayed = |a: &str, b: &str| !played.contains(&pair_key(a, b));
    if ids.iter().any(|id| counts[id] < MIN_COMPARISONS) {
        let mut keyed: Vec<(u32, f64, &String)> = ids.iter().map(|id| (counts[id], rng.random(), id)).collect();
        keyed.sort_by(|x, y| x.0.cmp(&y.0).then(x.1.total_cmp(&y.1)));
        let order: Vec<&String> = keyed.into_iter().map(|(_, _, id)| id).collect();
        let mut pairs = Vec::new();
        let mut used: BTreeSet<&String> = BTreeSet::new();
        for a in &order {
            if used.contains(a) || counts[*a] >= MIN_COMPARISONS {
                continue;
            }
            if let Some(b) = order.iter().find(|b| *b != a && !used.contains(*b) && unplayed(a, b)) {
                pairs.push(((*a).clone(), (*b).clone()));
                used.insert(a);
                used.insert(b);
            }
        }
        if !pairs.is_empty() {
            return pairs;
        }
    }
    let mut ranked: Vec<&String> = ids.iter().collect();
    ranked.sort_by(|x, y| scores[*y].total_cmp(&scores[*x]));
    let mut pairs = Vec::new();
    let mut used: BTreeSet<&String> = BTreeSet::new();
    for (position, a) in ranked.iter().enumerate() {
        if used.contains(a) {
            continue;
        }
        let window = ranked.iter().skip(position + 1).take(SWISS_WINDOW);
        if let Some(b) = window.into_iter().find(|b| !used.contains(*b) && unplayed(a, b)) {
            pairs.push(((*a).clone(), (*b).clone()));
            used.insert(a);
            used.insert(b);
        }
    }
    pairs
}

pub fn tournament(candidates: &[Candidate], compare: &mut Compare<'_>, rng: &mut PyRandom, swiss_rounds: u32) -> Vec<(String, f64)> {
    let by_id: BTreeMap<String, &Candidate> = candidates.iter().map(|c| (c.id.clone(), c)).collect();
    let ids: Vec<String> = by_id.keys().cloned().collect();
    let mut scores: BTreeMap<String, f64> = ids.iter().map(|id| (id.clone(), 0.0)).collect();
    let mut counts: BTreeMap<String, u32> = ids.iter().map(|id| (id.clone(), 0)).collect();
    let mut played = BTreeSet::new();
    let mut swiss = 0;
    while ids.len() > 1 {
        let before_swiss = ids.iter().any(|id| counts[id] < MIN_COMPARISONS);
        if !before_swiss {
            if swiss >= swiss_rounds {
                break;
            }
            swiss += 1;
        }
        let pairs = pairings(&ids, &scores, &played, &counts, rng);
        if pairs.is_empty() {
            break;
        }
        for (a, b) in pairs {
            played.insert(pair_key(&a, &b));
            *counts.get_mut(&a).unwrap_or(&mut 0) += 1;
            *counts.get_mut(&b).unwrap_or(&mut 0) += 1;
            match compare(by_id[&a], by_id[&b]) {
                None => {
                    *scores.entry(a).or_default() += 0.5 * WEIGHT_FLOOR;
                    *scores.entry(b).or_default() += 0.5 * WEIGHT_FLOOR;
                }
                Some((ra, rb)) => {
                    let weight = ((ra - rb).abs() / 9.0).max(WEIGHT_FLOOR);
                    let outcome = if ra > rb { 1.0 } else if ra < rb { 0.0 } else { 0.5 };
                    *scores.entry(a).or_default() += weight * outcome;
                    *scores.entry(b).or_default() += weight * (1.0 - outcome);
                }
            }
        }
    }
    scores.into_iter().collect()
}

pub fn choose(candidates: &[Candidate], compare: &mut Compare<'_>, rng: &mut PyRandom) -> Choice {
    let simple = |stage: &str, id: Option<&str>| Choice {
        stage: stage.to_string(),
        survivors: id.map(|id| vec![id.to_string()]).unwrap_or_default(),
        points: Vec::new(),
        chosen: id.map(|id| vec![id.to_string()]).unwrap_or_default(),
    };
    if candidates.is_empty() {
        return simple("none", None);
    }
    if candidates.len() == 1 {
        return simple("only candidate", Some(&candidates[0].id));
    }
    let survivors: Vec<Candidate> = candidates.iter().filter(|c| !c.breaks()).cloned().collect();
    if survivors.len() == 1 {
        return simple("regression filter", Some(&survivors[0].id));
    }
    let stage = if survivors.is_empty() { "judge, all broke" } else { "judge" };
    let pool: &[Candidate] = if survivors.is_empty() { candidates } else { &survivors };
    let points = tournament(pool, compare, rng, 1);
    let top = points.iter().map(|(_, p)| *p).fold(f64::NEG_INFINITY, f64::max);
    let mut chosen: Vec<String> = points.iter().filter(|(_, p)| *p == top).map(|(id, _)| id.clone()).collect();
    chosen.sort();
    Choice { stage: stage.to_string(), survivors: survivors.iter().map(|c| c.id.clone()).collect(), points, chosen }
}

/// The model server as judge (`server_judge`): one chat completion per pair, no proxy, every call
/// recorded in `log`.
pub struct ServerJudge {
    base: String,
    issue: String,
    seed: Option<u64>,
    timeout: Duration,
    model: Option<String>,
    pub log: Vec<Value>,
}

impl ServerJudge {
    pub fn new(server: &str, issue: &str, seed: Option<u64>, timeout: Duration) -> Self {
        let trimmed = server.trim_end_matches('/');
        let base = format!("{}/v1", trimmed.strip_suffix("/v1").unwrap_or(trimmed));
        ServerJudge { base, issue: issue.to_string(), seed, timeout, model: None, log: Vec::new() }
    }

    fn request(&self, path: &str, payload: Option<&Value>) -> Result<Value, String> {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
        let client = reqwest::Client::builder().no_proxy().timeout(self.timeout).build().map_err(|e| e.to_string())?;
        let url = format!("{}{path}", self.base);
        runtime.block_on(async {
            let request = match payload {
                Some(body) => client.post(&url).json(body),
                None => client.get(&url),
            };
            let response = request.send().await.map_err(|e| e.to_string())?;
            let status = response.status();
            if !status.is_success() {
                return Err(format!("HTTP Error {}: {}", status.as_u16(), status.canonical_reason().unwrap_or("")));
            }
            response.json::<Value>().await.map_err(|e| e.to_string())
        })
    }

    pub fn compare(&mut self, a: &Candidate, b: &Candidate) -> Option<(f64, f64)> {
        let mut record = serde_json::Map::new();
        record.insert("a".into(), json!(a.id));
        record.insert("b".into(), json!(b.id));
        let outcome = (|| -> Result<(Option<(f64, f64)>, String), String> {
            if self.model.is_none() {
                let listing = self.request("/models", None)?;
                let id = listing["data"].get(0).and_then(|card| card["id"].as_str()).unwrap_or("");
                self.model = Some(id.to_string());
            }
            let mut payload = json!({
                "model": self.model.clone().unwrap_or_default(),
                "max_tokens": JUDGE_MAX_TOKENS,
                "temperature": JUDGE_TEMPERATURE,
                "top_p": JUDGE_TOP_P,
                "messages": messages(&self.issue, &a.patch, &b.patch),
            });
            if let Some(seed) = self.seed {
                payload["seed"] = json!(seed);
            }
            let reply = self.request("/chat/completions", Some(&payload))?;
            let message = &reply["choices"][0]["message"];
            let answer = message["content"]
                .as_str()
                .filter(|text| !text.is_empty())
                .or_else(|| message["reasoning_content"].as_str())
                .unwrap_or("")
                .to_string();
            Ok((ratings(&answer), answer))
        })();
        let rated = match outcome {
            Ok((rated, answer)) => {
                record.insert("ratings".into(), rated.map_or(Value::Null, |(x, y)| json!([x, y])));
                let tail: Vec<char> = answer.chars().collect();
                let from = tail.len().saturating_sub(600);
                record.insert("answer_tail".into(), json!(tail[from..].iter().collect::<String>()));
                rated
            }
            Err(error) => {
                record.insert("error".into(), json!(error.chars().take(300).collect::<String>()));
                None
            }
        };
        self.log.push(Value::Object(record));
        rated
    }
}
