use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

use aether_program_governance::{GovernanceState, ProposalType};
use aether_types::{Address, H256};

fn addr(n: u8) -> Address {
    Address::from_slice(&[n; 20]).unwrap()
}

fn prop_id(n: u8) -> H256 {
    H256::from_slice(&[n; 32]).unwrap()
}

fn setup_governance(num_voters: usize) -> GovernanceState {
    let mut gov = GovernanceState::new();
    for i in 0..num_voters {
        let a = addr(i as u8 + 1);
        gov.voting_power.insert(a, 10_000_000_000_000);
        gov.effective_power.insert(a, 10_000_000_000_000);
        gov.total_voting_power = gov.total_voting_power.saturating_add(10_000_000_000_000);
    }
    gov
}

fn bench_propose(c: &mut Criterion) {
    let mut group = c.benchmark_group("governance/propose");
    for num_voters in [10, 50, 100] {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{num_voters}_voters")),
            &num_voters,
            |b, &num_voters| {
                b.iter_batched(
                    || setup_governance(num_voters),
                    |mut gov| {
                        black_box(gov.propose(
                            prop_id(0xFF),
                            addr(1),
                            ProposalType::ParameterChange {
                                parameter: "fee".to_string(),
                                value: 100,
                            },
                            "test".to_string(),
                            1000,
                        ))
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }
    group.finish();
}

fn bench_vote(c: &mut Criterion) {
    let mut group = c.benchmark_group("governance/vote");
    for existing_votes in [0, 50, 200] {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{existing_votes}_existing")),
            &existing_votes,
            |b, &existing_votes| {
                b.iter_batched(
                    || {
                        let total = existing_votes + 10;
                        let mut gov = setup_governance(total);
                        gov.propose(
                            prop_id(1),
                            addr(1),
                            ProposalType::ParameterChange {
                                parameter: "fee".to_string(),
                                value: 100,
                            },
                            "test".to_string(),
                            1000,
                        )
                        .unwrap();
                        // Cast existing votes
                        for i in 0..existing_votes {
                            gov.vote(prop_id(1), addr(i as u8 + 1), true, 1500).unwrap();
                        }
                        (gov, existing_votes)
                    },
                    |(mut gov, existing_votes)| {
                        let voter = addr(existing_votes as u8 + 1);
                        black_box(gov.vote(prop_id(1), voter, true, 1500))
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }
    group.finish();
}

fn bench_finalize(c: &mut Criterion) {
    let mut group = c.benchmark_group("governance/finalize");
    for num_voters in [10, 50, 100] {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{num_voters}_voters")),
            &num_voters,
            |b, &num_voters| {
                b.iter_batched(
                    || {
                        let mut gov = setup_governance(num_voters);
                        gov.propose(
                            prop_id(1),
                            addr(1),
                            ProposalType::ParameterChange {
                                parameter: "fee".to_string(),
                                value: 100,
                            },
                            "test".to_string(),
                            1000,
                        )
                        .unwrap();
                        for i in 0..num_voters {
                            gov.vote(prop_id(1), addr(i as u8 + 1), true, 1500).unwrap();
                        }
                        gov
                    },
                    |mut gov| black_box(gov.finalize(prop_id(1), 200_000)),
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }
    group.finish();
}

fn bench_delegate(c: &mut Criterion) {
    c.bench_function("governance/delegate_50_voters", |b| {
        b.iter_batched(
            || setup_governance(50),
            |mut gov| black_box(gov.delegate(addr(50), addr(1))),
            criterion::BatchSize::SmallInput,
        );
    });
}

criterion_group!(
    benches,
    bench_propose,
    bench_vote,
    bench_finalize,
    bench_delegate,
);
criterion_main!(benches);
