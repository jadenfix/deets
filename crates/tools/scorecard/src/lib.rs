use std::fmt::Write as _;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const TARGET_LATENCY_MS: f64 = 150.0;

#[derive(Debug, Clone, Deserialize)]
pub struct ValidatorSample {
    pub identity: String,
    pub uptime: f64,
    #[serde(default)]
    pub avg_latency_ms: f64,
    #[serde(default)]
    pub finality_faults: u32,
    #[serde(default)]
    pub missed_slots: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ScorecardEntry {
    pub identity: String,
    pub score: f64,
    pub grade: String,
    pub uptime: f64,
    pub avg_latency_ms: f64,
    pub finality_faults: u32,
    pub missed_slots: u32,
}

#[derive(Debug, Error)]
pub enum ScorecardError {
    #[error("no validator samples provided")]
    Empty,
}

pub fn load_samples(json: &str) -> Result<Vec<ValidatorSample>> {
    let samples: Vec<ValidatorSample> = serde_json::from_str(json)?;
    Ok(samples)
}

pub fn compute_score(sample: &ValidatorSample) -> f64 {
    let mut score = 100.0;

    let uptime_gap = (100.0 - sample.uptime).max(0.0);
    score -= uptime_gap * 0.6;

    if sample.avg_latency_ms > TARGET_LATENCY_MS {
        let latency_over = sample.avg_latency_ms - TARGET_LATENCY_MS;
        let penalty = (latency_over / TARGET_LATENCY_MS) * 25.0;
        score -= penalty;
    }

    score -= sample.finality_faults as f64 * 6.0;
    score -= sample.missed_slots as f64 * 0.4;

    score.clamp(0.0, 100.0)
}

fn letter_grade(score: f64) -> String {
    if score >= 90.0 {
        "A".into()
    } else if score >= 75.0 {
        "B".into()
    } else if score >= 60.0 {
        "C".into()
    } else {
        "D".into()
    }
}

pub fn generate_scorecard(samples: &[ValidatorSample]) -> Result<Vec<ScorecardEntry>> {
    if samples.is_empty() {
        return Err(ScorecardError::Empty.into());
    }

    let mut entries: Vec<ScorecardEntry> = samples
        .iter()
        .map(|sample| {
            let score = compute_score(sample);
            ScorecardEntry {
                identity: sample.identity.clone(),
                score,
                grade: letter_grade(score),
                uptime: sample.uptime,
                avg_latency_ms: sample.avg_latency_ms,
                finality_faults: sample.finality_faults,
                missed_slots: sample.missed_slots,
            }
        })
        .collect();

    entries.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    Ok(entries)
}

pub fn render_markdown(entries: &[ScorecardEntry]) -> String {
    let mut out = String::new();
    out.push_str("| Rank | Validator | Score | Grade | Uptime | Latency (ms) | Faults | Missed |");
    out.push('\n');
    out.push_str("|------|-----------|-------|-------|--------|--------------|--------|--------|");
    out.push('\n');

    for (idx, entry) in entries.iter().enumerate() {
        let _ = writeln!(
            out,
            "| {} | {} | {:.1} | {} | {:.2}% | {:.1} | {} | {} |",
            idx + 1,
            entry.identity,
            entry.score,
            entry.grade,
            entry.uptime,
            entry.avg_latency_ms,
            entry.finality_faults,
            entry.missed_slots
        );
    }

    out
}

pub fn render_csv(entries: &[ScorecardEntry]) -> String {
    let mut out =
        String::from("rank,validator,score,grade,uptime,latency_ms,finality_faults,missed_slots\n");
    for (idx, entry) in entries.iter().enumerate() {
        let _ = writeln!(
            out,
            "{},{},{:.2},{},{:.4},{:.2},{},{}",
            idx + 1,
            entry.identity,
            entry.score,
            entry.grade,
            entry.uptime,
            entry.avg_latency_ms,
            entry.finality_faults,
            entry.missed_slots
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn computes_scorecard_and_markdown() {
        let samples = vec![
            ValidatorSample {
                identity: "atlas".into(),
                uptime: 99.2,
                avg_latency_ms: 90.0,
                finality_faults: 0,
                missed_slots: 1,
            },
            ValidatorSample {
                identity: "nova".into(),
                uptime: 96.0,
                avg_latency_ms: 140.0,
                finality_faults: 1,
                missed_slots: 5,
            },
        ];

        let entries = generate_scorecard(&samples).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries[0].score >= entries[1].score);
        assert_eq!(entries[0].grade, "A");

        let markdown = render_markdown(&entries);
        assert!(markdown.contains("| 1 | atlas"));
        assert!(markdown.contains("| 2 | nova"));

        let csv = render_csv(&entries);
        assert!(csv.contains("atlas"));
        assert!(csv.contains("nova"));
    }

    #[test]
    fn errors_on_empty_input() {
        let err = generate_scorecard(&[]).unwrap_err();
        assert_eq!(err.to_string(), "no validator samples provided");
    }

    #[cfg(test)]
    mod proptests {
        use super::*;
        use proptest::prelude::*;

        fn arb_validator_sample() -> impl Strategy<Value = ValidatorSample> {
            (
                "[a-z]{3,12}",
                0.0f64..=100.0f64,
                0.0f64..=2000.0f64,
                0u32..=50u32,
                0u32..=200u32,
            )
                .prop_map(
                    |(identity, uptime, avg_latency_ms, finality_faults, missed_slots)| {
                        ValidatorSample {
                            identity,
                            uptime,
                            avg_latency_ms,
                            finality_faults,
                            missed_slots,
                        }
                    },
                )
        }

        proptest! {
            /// Score is always in [0.0, 100.0] for any inputs.
            #[test]
            fn score_always_in_bounds(sample in arb_validator_sample()) {
                let score = compute_score(&sample);
                prop_assert!(score >= 0.0, "score below 0: {}", score);
                prop_assert!(score <= 100.0, "score above 100: {}", score);
            }

            /// Grade assignment is consistent with score thresholds.
            #[test]
            fn grade_consistent_with_score(sample in arb_validator_sample()) {
                let score = compute_score(&sample);
                let entry = ScorecardEntry {
                    identity: sample.identity.clone(),
                    score,
                    grade: letter_grade(score),
                    uptime: sample.uptime,
                    avg_latency_ms: sample.avg_latency_ms,
                    finality_faults: sample.finality_faults,
                    missed_slots: sample.missed_slots,
                };
                if score >= 90.0 {
                    prop_assert_eq!(&entry.grade, "A");
                } else if score >= 75.0 {
                    prop_assert_eq!(&entry.grade, "B");
                } else if score >= 60.0 {
                    prop_assert_eq!(&entry.grade, "C");
                } else {
                    prop_assert_eq!(&entry.grade, "D");
                }
            }

            /// Higher uptime always produces a score >= lower uptime (all else equal).
            #[test]
            fn higher_uptime_not_worse(
                base_uptime in 0.0f64..=100.0f64,
                delta in 0.0f64..=10.0f64,
                avg_latency_ms in 0.0f64..=500.0f64,
                finality_faults in 0u32..=20u32,
                missed_slots in 0u32..=50u32,
            ) {
                let high_uptime = (base_uptime + delta).min(100.0);
                let lo = ValidatorSample {
                    identity: "lo".into(),
                    uptime: base_uptime,
                    avg_latency_ms,
                    finality_faults,
                    missed_slots,
                };
                let hi = ValidatorSample {
                    identity: "hi".into(),
                    uptime: high_uptime,
                    avg_latency_ms,
                    finality_faults,
                    missed_slots,
                };
                prop_assert!(
                    compute_score(&hi) >= compute_score(&lo) - 1e-9,
                    "higher uptime produced lower score"
                );
            }

            /// More finality faults never produces a higher score (all else equal).
            #[test]
            fn more_faults_not_better(
                uptime in 0.0f64..=100.0f64,
                avg_latency_ms in 0.0f64..=500.0f64,
                base_faults in 0u32..=10u32,
                extra_faults in 1u32..=5u32,
                missed_slots in 0u32..=50u32,
            ) {
                let fewer = ValidatorSample {
                    identity: "fewer".into(),
                    uptime,
                    avg_latency_ms,
                    finality_faults: base_faults,
                    missed_slots,
                };
                let more = ValidatorSample {
                    identity: "more".into(),
                    uptime,
                    avg_latency_ms,
                    finality_faults: base_faults + extra_faults,
                    missed_slots,
                };
                prop_assert!(
                    compute_score(&fewer) >= compute_score(&more) - 1e-9,
                    "more faults produced higher score"
                );
            }

            /// More missed slots never produces a higher score (all else equal).
            #[test]
            fn more_missed_slots_not_better(
                uptime in 0.0f64..=100.0f64,
                avg_latency_ms in 0.0f64..=500.0f64,
                finality_faults in 0u32..=10u32,
                base_missed in 0u32..=50u32,
                extra_missed in 1u32..=20u32,
            ) {
                let fewer = ValidatorSample {
                    identity: "fewer".into(),
                    uptime,
                    avg_latency_ms,
                    finality_faults,
                    missed_slots: base_missed,
                };
                let more = ValidatorSample {
                    identity: "more".into(),
                    uptime,
                    avg_latency_ms,
                    finality_faults,
                    missed_slots: base_missed + extra_missed,
                };
                prop_assert!(
                    compute_score(&fewer) >= compute_score(&more) - 1e-9,
                    "more missed slots produced higher score"
                );
            }

            /// generate_scorecard output is sorted descending by score.
            #[test]
            fn scorecard_is_sorted_descending(
                samples in prop::collection::vec(arb_validator_sample(), 1..=20)
            ) {
                let entries = generate_scorecard(&samples).unwrap();
                for window in entries.windows(2) {
                    prop_assert!(
                        window[0].score >= window[1].score,
                        "scorecard not sorted: {} < {}",
                        window[0].score, window[1].score
                    );
                }
            }

            /// generate_scorecard preserves all validators (no entries dropped/added).
            #[test]
            fn scorecard_preserves_count(
                samples in prop::collection::vec(arb_validator_sample(), 1..=20)
            ) {
                let entries = generate_scorecard(&samples).unwrap();
                prop_assert_eq!(entries.len(), samples.len());
            }

            /// render_csv header is always present and each row has 8 comma-separated fields.
            #[test]
            fn csv_format_invariants(
                samples in prop::collection::vec(arb_validator_sample(), 1..=10)
            ) {
                let entries = generate_scorecard(&samples).unwrap();
                let csv = render_csv(&entries);
                let lines: Vec<&str> = csv.lines().collect();
                // header + one row per entry
                prop_assert_eq!(lines.len(), entries.len() + 1, "wrong line count");
                prop_assert!(lines[0].starts_with("rank,"), "missing header");
                for row in &lines[1..] {
                    let fields: Vec<&str> = row.split(',').collect();
                    prop_assert_eq!(fields.len(), 8, "row has wrong field count: {}", row);
                }
            }

            /// render_markdown contains every validator identity.
            #[test]
            fn markdown_contains_all_identities(
                samples in prop::collection::vec(arb_validator_sample(), 1..=10)
            ) {
                let entries = generate_scorecard(&samples).unwrap();
                let md = render_markdown(&entries);
                for entry in &entries {
                    prop_assert!(
                        md.contains(&entry.identity),
                        "markdown missing identity: {}",
                        entry.identity
                    );
                }
            }

            /// load_samples round-trips through JSON correctly.
            #[test]
            fn load_samples_roundtrip(
                samples in prop::collection::vec(arb_validator_sample(), 0..=10)
            ) {
                let json = serde_json::to_string(&samples.iter().map(|s| {
                    serde_json::json!({
                        "identity": s.identity,
                        "uptime": s.uptime,
                        "avg_latency_ms": s.avg_latency_ms,
                        "finality_faults": s.finality_faults,
                        "missed_slots": s.missed_slots,
                    })
                }).collect::<Vec<_>>()).unwrap();
                let loaded = load_samples(&json).unwrap();
                prop_assert_eq!(loaded.len(), samples.len());
                for (orig, loaded) in samples.iter().zip(loaded.iter()) {
                    prop_assert_eq!(&orig.identity, &loaded.identity);
                }
            }
        }
    }
}
