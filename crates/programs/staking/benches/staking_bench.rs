use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

use aether_program_staking::StakingState;
use aether_types::Address;

fn addr(n: u8) -> Address {
    Address::from_slice(&[n; 20]).unwrap()
}

fn setup_state(num_validators: usize) -> StakingState {
    let mut state = StakingState::new();
    for i in 0..num_validators {
        let a = addr(i as u8 + 1);
        state
            .register_validator(a, a, 1_000_000_000, 500, a)
            .unwrap();
    }
    state
}

fn bench_register_validator(c: &mut Criterion) {
    let mut group = c.benchmark_group("staking/register_validator");
    for count in [10, 50, 100] {
        group.bench_with_input(
            BenchmarkId::from_parameter(count),
            &count,
            |b, &count| {
                b.iter_batched(
                    || setup_state(count),
                    |mut state| {
                        // Register one more validator
                        let a = addr(count as u8 + 1);
                        black_box(state.register_validator(a, a, 1_000_000_000, 500, a))
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }
    group.finish();
}

fn bench_delegate(c: &mut Criterion) {
    let mut group = c.benchmark_group("staking/delegate");
    for num_delegations in [10, 100, 500] {
        group.bench_with_input(
            BenchmarkId::from_parameter(num_delegations),
            &num_delegations,
            |b, &num_delegations| {
                b.iter_batched(
                    || {
                        let mut state = setup_state(5);
                        // Pre-populate delegations
                        for i in 0..num_delegations {
                            let d = addr(200u8.wrapping_add(i as u8));
                            let v = addr((i % 5) as u8 + 1);
                            state.delegate(d, d, v, 500_000_000).unwrap();
                        }
                        state
                    },
                    |mut state| {
                        let d = addr(199);
                        let v = addr(1);
                        black_box(state.delegate(d, d, v, 500_000_000))
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }
    group.finish();
}

fn bench_slash(c: &mut Criterion) {
    let mut group = c.benchmark_group("staking/slash");
    for num_delegations in [0, 10, 100] {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{num_delegations}_delegations")),
            &num_delegations,
            |b, &num_delegations| {
                b.iter_batched(
                    || {
                        let mut state = setup_state(5);
                        for i in 0..num_delegations {
                            let d = addr(200u8.wrapping_add(i as u8));
                            state.delegate(d, d, addr(1), 500_000_000).unwrap();
                        }
                        state
                    },
                    |mut state| black_box(state.slash(addr(1), 500, 1000)),
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }
    group.finish();
}

fn bench_distribute_rewards(c: &mut Criterion) {
    let mut group = c.benchmark_group("staking/distribute_rewards");
    for (validators, delegations_per) in [(10, 5), (50, 10), (100, 20)] {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{validators}v_{delegations_per}d")),
            &(validators, delegations_per),
            |b, &(validators, delegations_per)| {
                b.iter_batched(
                    || {
                        let mut state = setup_state(validators);
                        for v in 0..validators {
                            for d in 0..delegations_per {
                                let da = addr(((v * delegations_per + d + 200) % 256) as u8);
                                let va = addr(v as u8 + 1);
                                let _ = state.delegate(da, da, va, 100_000_000);
                            }
                        }
                        state
                    },
                    |mut state| {
                        state.distribute_rewards(10_000_000_000);
                        black_box(&state);
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }
    group.finish();
}

fn bench_unbond_and_complete(c: &mut Criterion) {
    let mut group = c.benchmark_group("staking/unbond_complete");
    group.bench_function("unbond", |b| {
        b.iter_batched(
            || {
                let mut state = setup_state(5);
                let d = addr(200);
                state.delegate(d, d, addr(1), 1_000_000_000).unwrap();
                state
            },
            |mut state| {
                let d = addr(200);
                black_box(state.unbond(d, d, addr(1), 100_000_000, 1000))
            },
            criterion::BatchSize::SmallInput,
        );
    });
    group.bench_function("complete_unbonding/100_entries", |b| {
        b.iter_batched(
            || {
                let mut state = setup_state(5);
                for i in 0..100u64 {
                    let d = addr(200u8.wrapping_add(i as u8));
                    state.delegate(d, d, addr(1), 1_000_000_000).unwrap();
                    state.unbond(d, d, addr(1), 500_000_000, i).unwrap();
                }
                state
            },
            |mut state| black_box(state.complete_unbonding(200_000)),
            criterion::BatchSize::SmallInput,
        );
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_register_validator,
    bench_delegate,
    bench_slash,
    bench_distribute_rewards,
    bench_unbond_and_complete,
);
criterion_main!(benches);
