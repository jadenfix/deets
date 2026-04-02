use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

use aether_program_amm::LiquidityPool;
use aether_types::{Address, H256};

fn pool_id() -> H256 {
    H256::from_slice(&[0xAB; 32]).unwrap()
}

fn token_a() -> Address {
    Address::from_slice(&[1; 20]).unwrap()
}

fn token_b() -> Address {
    Address::from_slice(&[2; 20]).unwrap()
}

fn seeded_pool(reserve: u128) -> LiquidityPool {
    let mut pool = LiquidityPool::new(pool_id(), token_a(), token_b(), 30).unwrap();
    pool.add_liquidity(reserve, reserve, 0).unwrap();
    pool
}

fn bench_add_liquidity(c: &mut Criterion) {
    let mut group = c.benchmark_group("amm/add_liquidity");
    group.bench_function("initial", |b| {
        b.iter_batched(
            || LiquidityPool::new(pool_id(), token_a(), token_b(), 30).unwrap(),
            |mut pool| black_box(pool.add_liquidity(1_000_000_000, 1_000_000_000, 0)),
            criterion::BatchSize::SmallInput,
        );
    });
    for reserve in [1_000_000u128, 1_000_000_000, 1_000_000_000_000] {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("proportional_{reserve}")),
            &reserve,
            |b, &reserve| {
                b.iter_batched(
                    || seeded_pool(reserve),
                    |mut pool| black_box(pool.add_liquidity(reserve / 10, reserve / 10, 0)),
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }
    group.finish();
}

fn bench_swap(c: &mut Criterion) {
    let mut group = c.benchmark_group("amm/swap");
    for (name, reserve, swap_amt) in [
        ("small_pool", 1_000_000u128, 1_000u128),
        ("medium_pool", 1_000_000_000, 1_000_000),
        ("large_pool", 1_000_000_000_000, 1_000_000_000),
    ] {
        group.bench_function(format!("a_to_b/{name}"), |b| {
            b.iter_batched(
                || seeded_pool(reserve),
                |mut pool| black_box(pool.swap_a_to_b(swap_amt, 0)),
                criterion::BatchSize::SmallInput,
            );
        });
        group.bench_function(format!("b_to_a/{name}"), |b| {
            b.iter_batched(
                || seeded_pool(reserve),
                |mut pool| black_box(pool.swap_b_to_a(swap_amt, 0)),
                criterion::BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

fn bench_remove_liquidity(c: &mut Criterion) {
    let mut group = c.benchmark_group("amm/remove_liquidity");
    for reserve in [1_000_000u128, 1_000_000_000, 1_000_000_000_000] {
        group.bench_with_input(
            BenchmarkId::from_parameter(reserve),
            &reserve,
            |b, &reserve| {
                b.iter_batched(
                    || seeded_pool(reserve),
                    |mut pool| {
                        let lp = pool.lp_token_supply / 10;
                        black_box(pool.remove_liquidity(lp, 0, 0))
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }
    group.finish();
}

fn bench_swap_sequence(c: &mut Criterion) {
    c.bench_function("amm/swap_sequence_100", |b| {
        b.iter_batched(
            || seeded_pool(1_000_000_000_000),
            |mut pool| {
                for i in 0..100u128 {
                    let amt = 1_000_000 + i * 10_000;
                    if i % 2 == 0 {
                        let _ = pool.swap_a_to_b(amt, 0);
                    } else {
                        let _ = pool.swap_b_to_a(amt, 0);
                    }
                }
                black_box(&pool);
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

criterion_group!(
    benches,
    bench_add_liquidity,
    bench_swap,
    bench_remove_liquidity,
    bench_swap_sequence,
);
criterion_main!(benches);
