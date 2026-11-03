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
