// SPDX-License-Identifier: Apache-2.0

//! `fluence-ngram serve` — a JSON-lines prediction server over stdin/stdout.
//!
//! The evaluation harness (`ml/eval`, Python) drives this binary so it measures
//! the *real* fallback model rather than a reimplementation (SPEC §8.A,
//! ADR-0006). One request per input line, one response per output line:
//!
//! - `{"train":{"text":"…"}}`  → `{"ok":true}` (count words into the model)
//! - `{"complete":{"context":"…","prefix":"vou","n":3}}` → `{"words":["voudrais"]}`
//!
//! `context` is accepted for forward compatibility (bigram models); the v0
//! unigram model ignores it. The process holds one model in memory for its
//! lifetime, so a whole corpus run is a single spawn.

use std::io::{self, BufRead, Write};

use fluence_ngram::NgramModel;
use serde::Deserialize;
use serde_json::json;

/// One request line.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Request {
    /// Count the words of `text` into the model.
    Train {
        /// Text to train on.
        text: String,
    },
    /// Rank completions of `prefix`. A `context` key is tolerated (serde
    /// ignores unknown fields) for the future bigram model; v0 needs only the
    /// prefix.
    Complete {
        /// Characters of the current word typed so far.
        prefix: String,
        /// How many completions to return.
        n: usize,
    },
}

fn main() -> io::Result<()> {
    let mut model = NgramModel::new();
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Request>(&line) {
            Ok(Request::Train { text }) => {
                model.train(&text);
                json!({ "ok": true })
            }
            Ok(Request::Complete { prefix, n }) => {
                let words: Vec<String> = model
                    .complete(&prefix, n)
                    .into_iter()
                    .map(|c| c.word)
                    .collect();
                json!({ "words": words })
            }
            Err(error) => json!({ "error": error.to_string() }),
        };
        writeln!(out, "{response}")?;
        out.flush()?;
    }
    Ok(())
}
