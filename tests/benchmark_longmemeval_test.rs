//! The converter that turns LongMemEval into something `mem eval` can
//! measure. The fixture here is written by hand in the dataset's shape, so
//! the test runs without the 278 MB download and without redistributing a
//! single line of someone else's corpus.
use std::process::Command;

/// The suite must not depend on Python being installed: CI runs on a Rust
/// image. When it is missing the test says so and passes, the way
/// `concurrency_test` steps aside when it cannot create the condition it
/// needs.
fn python() -> Option<String> {
    for c in ["python3", "python"] {
        if Command::new(c).arg("--version").output().is_ok_and(|o| o.status.success()) {
            return Some(c.to_string());
        }
    }
    eprintln!("no python on the PATH: skipping the LongMemEval converter test");
    None
}

const FIXTURE: &str = r#"[
  {
    "question_id": "gpt4_abc",
    "question_type": "single-session-user",
    "question": "What did I say about the cache going stale?",
    "answer": "it was rolled back",
    "question_date": "2023/05/20 (Sat) 10:11",
    "haystack_session_ids": ["answer_abc", "distractor_1"],
    "haystack_dates": ["2023/05/01 (Mon) 09:00", "2023/05/02 (Tue) 09:00"],
    "haystack_sessions": [
      [{"role": "user", "content": "the cache went stale so we rolled the deploy back", "has_answer": true},
       {"role": "assistant", "content": "noted"}],
      [{"role": "user", "content": "what is the capital of Peru"},
       {"role": "assistant", "content": "Lima"}]
    ],
    "answer_session_ids": ["answer_abc"]
  },
  {
    "question_id": "gpt4_xyz_abs",
    "question_type": "single-session-user",
    "question": "Which laptop did I buy?",
    "answer": "abstention",
    "question_date": "2023/06/01 (Thu) 10:00",
    "haystack_session_ids": ["d1"],
    "haystack_dates": ["2023/05/09 (Tue) 09:00"],
    "haystack_sessions": [[{"role": "user", "content": "unrelated chatter"}]],
    "answer_session_ids": []
  }
]"#;

fn run_converter(py: &str, dir: &std::path::Path) -> std::process::Output {
    let input = dir.join("longmemeval_fixture.json");
    std::fs::write(&input, FIXTURE).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = Command::new(py)
        .arg(root.join("tools/longmemeval_to_memories.py"))
        .arg(&input)
        .arg(dir.join("out"))
        .output()
        .expect("could not run the converter");
    assert!(out.status.success(), "the converter failed: {}", String::from_utf8_lossy(&out.stderr));
    out
}

#[test]
fn it_writes_one_case_per_answerable_question() {
    let Some(py) = python() else { return };
    let dir = tempfile::tempdir().unwrap();
    run_converter(&py, dir.path());

    let raw = std::fs::read_to_string(dir.path().join("out/evalset.json")).unwrap();
    let set: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let cases = set["cases"].as_array().unwrap();

    // The abstention instance has no evidence session, so there is nothing to
    // measure retrieval against: counting it would drag recall down for a
    // question the benchmark itself excludes.
    assert_eq!(cases.len(), 1, "the _abs instance must not become a case");
    let c = &cases[0];
    assert_eq!(c["id"], "gpt4_abc");
    assert_eq!(c["query"], "What did I say about the cache going stale?");
    assert_eq!(c["kind"], "single-session-user");
    // Scoped to its own haystack: the question has to find its evidence among
    // its own sessions, not among every instance's pooled together.
    assert_eq!(c["project"], "gpt4_abc");
    assert_eq!(c["expect_docs"][0], "gpt4_abc/answer_abc");
}

#[test]
fn every_haystack_session_becomes_a_memory_the_importer_accepts() {
    let Some(py) = python() else { return };
    let dir = tempfile::tempdir().unwrap();
    run_converter(&py, dir.path());

    let mut found = Vec::new();
    for e in std::fs::read_dir(dir.path().join("out/memories")).unwrap() {
        let p = e.unwrap().path();
        let text = std::fs::read_to_string(&p).unwrap();
        // Parsing with the real importer, not a regex: what this has to
        // guarantee is that `mem import` accepts the file, and the only
        // honest check of that is the code that will read it.
        let m = anathem::import::parse(&text)
            .unwrap_or_else(|| panic!("mem import would skip {}", p.display()));
        found.push((m.project, m.ext_id.unwrap(), m.body));
    }
    found.sort();

    assert_eq!(found.len(), 3, "two sessions for the first question, one for the second");
    assert_eq!(found[0].0, "gpt4_abc");
    assert_eq!(found[0].1, "gpt4_abc/answer_abc");
    assert!(found[0].2.contains("rolled the deploy back"), "body was {}", found[0].2);
    // Both speakers are kept: evidence lands in assistant turns too, and a
    // corpus of only the user's half would make some questions unanswerable.
    assert!(found[0].2.contains("noted"), "the assistant turn is missing");
}

#[test]
fn ext_ids_are_namespaced_so_two_questions_can_share_a_session_id() {
    let Some(py) = python() else { return };
    let dir = tempfile::tempdir().unwrap();
    run_converter(&py, dir.path());

    // `memories.ext_id` is unique across the whole database. LongMemEval
    // session ids are only unique inside one instance, so loading several
    // instances with the raw ids would silently drop every collision — and
    // the dropped memory could be the evidence.
    let mut ids: Vec<String> = std::fs::read_dir(dir.path().join("out/memories"))
        .unwrap()
        .map(|e| {
            let t = std::fs::read_to_string(e.unwrap().path()).unwrap();
            anathem::import::parse(&t).unwrap().ext_id.unwrap()
        })
        .collect();
    ids.sort();
    let n = ids.len();
    ids.dedup();
    assert_eq!(ids.len(), n, "two memories share an ext_id: {ids:?}");
    assert!(ids.iter().all(|i| i.contains('/')), "ext_ids are not namespaced: {ids:?}");
}

/// LongMemEval lists the same session twice inside one instance in 13 of its
/// 500 cases. Writing the file twice is harmless when the content matches,
/// but reporting a count that does not match what landed on disk is not:
/// this repository has been bitten three times by a tool that announced work
/// it had not done.
const DUPLICATE_FIXTURE: &str = r#"[
  {
    "question_id": "dup",
    "question_type": "multi-session",
    "question": "what happened",
    "haystack_session_ids": ["s1", "s1", "s2"],
    "haystack_dates": ["2023/05/01 (Mon) 09:00", "2023/05/01 (Mon) 09:00", "2023/05/02 (Tue) 09:00"],
    "haystack_sessions": [
      [{"role": "user", "content": "the same session listed twice"}],
      [{"role": "user", "content": "the same session listed twice"}],
      [{"role": "user", "content": "a different one"}]
    ],
    "answer_session_ids": ["s1"]
  }
]"#;

const CONFLICTING_FIXTURE: &str = r#"[
  {
    "question_id": "clash",
    "question_type": "multi-session",
    "question": "what happened",
    "haystack_session_ids": ["s1", "s1"],
    "haystack_dates": ["2023/05/01 (Mon) 09:00", "2023/05/02 (Tue) 09:00"],
    "haystack_sessions": [
      [{"role": "user", "content": "one body"}],
      [{"role": "user", "content": "a completely different body"}]
    ],
    "answer_session_ids": ["s1"]
  }
]"#;

fn convert(py: &str, dir: &std::path::Path, fixture: &str) -> std::process::Output {
    let input = dir.join("in.json");
    std::fs::write(&input, fixture).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    Command::new(py)
        .arg(root.join("tools/longmemeval_to_memories.py"))
        .arg(&input)
        .arg(dir.join("out"))
        .output()
        .expect("could not run the converter")
}

#[test]
fn the_count_it_reports_matches_the_files_on_disk() {
    let Some(py) = python() else { return };
    let dir = tempfile::tempdir().unwrap();
    let out = convert(&py, dir.path(), DUPLICATE_FIXTURE);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    let on_disk = std::fs::read_dir(dir.path().join("out/memories")).unwrap().count();
    let said = String::from_utf8_lossy(&out.stdout);
    let reported: usize = said.split_whitespace().next().unwrap().parse().unwrap();
    assert_eq!(reported, on_disk, "it reported {reported} memories and wrote {on_disk}: {said}");
    assert_eq!(on_disk, 2, "the repeated session is one memory, not two");
    assert!(said.contains("duplicate"), "the repetition has to be said out loud: {said}");
}

#[test]
fn it_refuses_when_a_repeated_session_id_carries_different_bodies() {
    let Some(py) = python() else { return };
    let dir = tempfile::tempdir().unwrap();
    let out = convert(&py, dir.path(), CONFLICTING_FIXTURE);

    // Here the second write would silently replace the first, and the lost
    // memory can be the evidence the question is scored on. Failing loudly is
    // the only honest option: the measurement would be wrong and nothing
    // would say so.
    assert!(!out.status.success(), "it should have refused");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("clash/s1"), "the message has to name the ext_id: {err}");
}

#[test]
fn it_can_take_a_subset_of_the_questions() {
    let Some(py) = python() else { return };
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in.json");
    std::fs::write(&input, FIXTURE).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = Command::new(&py)
        .arg(root.join("tools/longmemeval_to_memories.py"))
        .arg(&input)
        .arg(dir.path().join("out"))
        .arg("--questions")
        .arg("1")
        .output()
        .expect("could not run the converter");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    // LongMemEval-M gives each question around 500 haystack sessions, and
    // 500 questions of that do not fit on a laptop. What makes -M the harder
    // benchmark is the depth of one haystack, not how many of them there
    // are, so taking fewer questions keeps the comparison and drops the cost.
    // No seed to record: the selection is deterministic, so the same N gives
    // the same corpus to anyone who runs it.
    let raw = std::fs::read_to_string(dir.path().join("out/evalset.json")).unwrap();
    let set: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(set["cases"].as_array().unwrap().len(), 1);
    assert_eq!(set["cases"][0]["id"], "gpt4_abc");

    // And the dropped question's sessions are not in the corpus either: they
    // would be distractors nothing ever searches, inflating the index for
    // nothing.
    let n = std::fs::read_dir(dir.path().join("out/memories")).unwrap().count();
    assert_eq!(n, 2, "only the kept question's two sessions");
}

/// Four instances, two of each type, laid out the way LongMemEval-M really
/// is: sorted by question type. A prefix of this file is all of one type.
const SORTED_FIXTURE: &str = r#"[
  {"question_id":"easy_1","question_type":"single-session-user","question":"q1",
   "haystack_session_ids":["a"],"haystack_dates":["2023/05/01 (Mon) 09:00"],
   "haystack_sessions":[[{"role":"user","content":"one"}]],"answer_session_ids":["a"]},
  {"question_id":"easy_2","question_type":"single-session-user","question":"q2",
   "haystack_session_ids":["a"],"haystack_dates":["2023/05/01 (Mon) 09:00"],
   "haystack_sessions":[[{"role":"user","content":"two"}]],"answer_session_ids":["a"]},
  {"question_id":"hard_1","question_type":"multi-session","question":"q3",
   "haystack_session_ids":["a"],"haystack_dates":["2023/05/01 (Mon) 09:00"],
   "haystack_sessions":[[{"role":"user","content":"three"}]],"answer_session_ids":["a"]},
  {"question_id":"hard_2","question_type":"multi-session","question":"q4",
   "haystack_session_ids":["a"],"haystack_dates":["2023/05/01 (Mon) 09:00"],
   "haystack_sessions":[[{"role":"user","content":"four"}]],"answer_session_ids":["a"]}
]"#;

#[test]
fn a_subset_keeps_the_mix_of_question_types() {
    let Some(py) = python() else { return };
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in.json");
    std::fs::write(&input, SORTED_FIXTURE).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = Command::new(&py)
        .arg(root.join("tools/longmemeval_to_memories.py"))
        .arg(&input)
        .arg(dir.path().join("out"))
        .arg("--questions")
        .arg("2")
        .output()
        .expect("could not run the converter");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    let raw = std::fs::read_to_string(dir.path().join("out/evalset.json")).unwrap();
    let set: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let kinds: Vec<&str> = set["cases"].as_array().unwrap()
        .iter().map(|c| c["kind"].as_str().unwrap()).collect();

    // LongMemEval-M is sorted by question type: its first 100 instances are
    // entirely `single-session-*`, the two types anathem already answers
    // perfectly, while the file as a whole is 54% `multi-session` and
    // `temporal-reasoning`. Taking a prefix would measure the easiest
    // stratum and report it as the benchmark.
    assert_eq!(kinds.len(), 2);
    assert!(
        kinds.contains(&"single-session-user") && kinds.contains(&"multi-session"),
        "a subset of a type-sorted file has to span the types, got {kinds:?}"
    );
}

/// A fixture with enough distractors that a fraction of them is a
/// meaningful thing to take: one evidence session and six that are not.
const FIXTURE_MANY_DISTRACTORS: &str = r#"[
  {
    "question_id": "gpt4_many",
    "question_type": "multi-session",
    "question": "What did I say about the cache?",
    "answer": "it went stale",
    "question_date": "2023/05/20 (Sat) 10:11",
    "haystack_session_ids": ["answer_1", "d1", "d2", "d3", "d4", "d5", "d6"],
    "haystack_dates": ["2023/05/01 (Mon) 09:00", "2023/05/02 (Tue) 09:00",
                       "2023/05/03 (Wed) 09:00", "2023/05/04 (Thu) 09:00",
                       "2023/05/05 (Fri) 09:00", "2023/05/06 (Sat) 09:00",
                       "2023/05/07 (Sun) 09:00"],
    "haystack_sessions": [
      [{"role": "user", "content": "the cache went stale"}],
      [{"role": "user", "content": "distractor one"}],
      [{"role": "user", "content": "distractor two"}],
      [{"role": "user", "content": "distractor three"}],
      [{"role": "user", "content": "distractor four"}],
      [{"role": "user", "content": "distractor five"}],
      [{"role": "user", "content": "distractor six"}]
    ],
    "answer_session_ids": ["answer_1"]
  }
]"#;

fn run_with_distractors(py: &str, dir: &std::path::Path, keep: &str) -> Vec<String> {
    let input = dir.join("fixture.json");
    std::fs::write(&input, FIXTURE_MANY_DISTRACTORS).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = Command::new(py)
        .arg(root.join("tools/longmemeval_to_memories.py"))
        .arg(&input)
        .arg(dir.join("out"))
        .arg("--distractors")
        .arg(keep)
        .output()
        .expect("could not run the converter");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let mut names: Vec<String> = std::fs::read_dir(dir.path_join_out())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    names.sort();
    names
}

/// A helper so the line above reads: the memories live under `out/memories`.
trait OutDir {
    fn path_join_out(&self) -> std::path::PathBuf;
}
impl OutDir for std::path::Path {
    fn path_join_out(&self) -> std::path::PathBuf {
        self.join("out/memories")
    }
}

#[test]
fn dropping_distractors_never_drops_an_evidence_session() {
    // The synthetic scope the research issue asks for is "all the evidence,
    // fewer distractors". A run that loses evidence is not a bound on what
    // scoping could buy; it is a different and worse benchmark.
    let Some(py) = python() else { return };
    let dir = tempfile::tempdir().unwrap();
    let names = run_with_distractors(&py, dir.path(), "0.25");

    assert!(
        names.iter().any(|n| n.contains("answer_1")),
        "la sesión con la evidencia no puede caer nunca: {names:?}"
    );
    assert!(names.len() < 7, "it has to have removed distractors: {names:?}");
    assert!(names.len() > 1, "and not all of them: {names:?}");
}

#[test]
fn the_distractors_kept_are_the_same_ones_every_run() {
    // No seed to write down: two people running the same command have to
    // get the same corpus, or the numbers are not comparable.
    let Some(py) = python() else { return };
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    assert_eq!(
        run_with_distractors(&py, a.path(), "0.5"),
        run_with_distractors(&py, b.path(), "0.5")
    );
}

#[test]
fn a_distractor_fraction_outside_zero_to_one_is_refused() {
    let Some(py) = python() else { return };
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("fixture.json");
    std::fs::write(&input, FIXTURE_MANY_DISTRACTORS).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = Command::new(py)
        .arg(root.join("tools/longmemeval_to_memories.py"))
        .arg(&input)
        .arg(dir.path().join("out"))
        .arg("--distractors")
        .arg("1.5")
        .output()
        .unwrap();
    assert!(!out.status.success(), "an out-of-range fraction cannot pass silently");
}
