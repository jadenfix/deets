use criterion::{black_box, criterion_group, criterion_main, Criterion};

use aether_state_merkle::SparseMerkleTree;
use aether_types::{Address, H256};

fn make_addr(i: u16) -> Address {
    let mut bytes = [0u8; 20];
    bytes[0] = (i >> 8) as u8;
    bytes[1] = (i & 0xff) as u8;
    Address::from_slice(&bytes).unwrap()
}

fn make_hash(i: u16) -> H256 {
    let mut bytes = [0u8; 32];
    bytes[0] = (i >> 8) as u8;
    bytes[1] = (i & 0xff) as u8;
    H256::from_slice(&bytes).unwrap()
}

fn bench_smt_insert_1(c: &mut Criterion) {
    let addr = make_addr(0);
    let val = make_hash(0);

    c.bench_function("smt_insert_1_key", |b| {
        b.iter(|| {
            let mut tree = SparseMerkleTree::new();
            tree.update(black_box(addr), black_box(val));
        })
    });
}

fn bench_smt_insert_100(c: &mut Criterion) {
    c.bench_function("smt_insert_100_keys", |b| {
        b.iter(|| {
            let mut tree = SparseMerkleTree::new();
            for i in 0..100u16 {
                tree.update(make_addr(i), make_hash(i));
            }
        })
    });
}

fn bench_smt_prove(c: &mut Criterion) {
    let mut tree = SparseMerkleTree::new();
    for i in 0..100u16 {
        tree.update(make_addr(i), make_hash(i));
    }

    let target = make_addr(50);

    c.bench_function("smt_prove_in_100_keys", |b| {
        b.iter(|| tree.prove(black_box(&target)))
    });
}

fn bench_smt_verify(c: &mut Criterion) {
    let mut tree = SparseMerkleTree::new();
    for i in 0..100u16 {
        tree.update(make_addr(i), make_hash(i));
    }

    let proof = tree.prove(&make_addr(50));

    c.bench_function("smt_verify_proof", |b| {
        b.iter(|| black_box(&proof).verify())
    });
}

fn bench_smt_insert_1000(c: &mut Criterion) {
    c.bench_function("smt_insert_1000_keys", |b| {
        b.iter(|| {
            let mut tree = SparseMerkleTree::new();
            for i in 0..1000u16 {
                tree.update(make_addr(i), make_hash(i));
            }
        })
    });
}

fn bench_smt_root_after_updates(c: &mut Criterion) {
    let mut group = c.benchmark_group("smt_root_after_updates");
    for count in [10u16, 100, 500] {
        group.bench_with_input(
            criterion::BenchmarkId::from_parameter(count),
            &count,
            |b, &count| {
                b.iter(|| {
                    let mut tree = SparseMerkleTree::new();
                    for i in 0..count {
                        tree.update(make_addr(i), make_hash(i));
                    }
                    black_box(tree.root());
                })
            },
        );
    }
    group.finish();
}

fn bench_smt_incremental_update(c: &mut Criterion) {
    let mut tree = SparseMerkleTree::new();
    for i in 0..1000u16 {
        tree.update(make_addr(i), make_hash(i));
    }

    c.bench_function("smt_single_update_in_1000_key_tree", |b| {
        let mut counter = 0u16;
        b.iter(|| {
            counter = counter.wrapping_add(1);
            tree.update(make_addr(counter % 1000), make_hash(counter));
            black_box(tree.root());
        })
    });
}

fn bench_smt_batch_prove(c: &mut Criterion) {
    let mut group = c.benchmark_group("smt_batch_prove");
    for (tree_size, proof_count) in [(100u16, 10u16), (1000, 50), (1000, 100)] {
        let mut tree = SparseMerkleTree::new();
        for i in 0..tree_size {
            tree.update(make_addr(i), make_hash(i));
        }
        let keys: Vec<Address> = (0..proof_count).map(make_addr).collect();

        group.bench_with_input(
            criterion::BenchmarkId::new(format!("tree_{}", tree_size), proof_count),
            &keys,
            |b, keys| {
                b.iter(|| {
                    for key in keys {
                        black_box(tree.prove(key));
                    }
                })
            },
        );
    }
    group.finish();
}

fn bench_smt_batch_verify(c: &mut Criterion) {
    let mut group = c.benchmark_group("smt_batch_verify");
    for count in [10u16, 50, 100] {
        let mut tree = SparseMerkleTree::new();
        for i in 0..1000u16 {
            tree.update(make_addr(i), make_hash(i));
        }
        let proofs: Vec<_> = (0..count).map(|i| tree.prove(&make_addr(i))).collect();

        group.bench_with_input(
            criterion::BenchmarkId::from_parameter(count),
            &proofs,
            |b, proofs| {
                b.iter(|| {
                    for proof in proofs {
                        black_box(proof.verify());
                    }
                })
            },
        );
    }
    group.finish();
}

fn bench_smt_delete(c: &mut Criterion) {
    let mut group = c.benchmark_group("smt_delete");
    for tree_size in [100u16, 500, 1000] {
        group.bench_with_input(
            criterion::BenchmarkId::from_parameter(tree_size),
            &tree_size,
            |b, &tree_size| {
                b.iter_with_setup(
                    || {
                        let mut tree = SparseMerkleTree::new();
                        for i in 0..tree_size {
                            tree.update(make_addr(i), make_hash(i));
                        }
                        tree
                    },
                    |mut tree| {
                        for i in 0..tree_size / 2 {
                            tree.delete(&make_addr(i));
                        }
                        black_box(tree.root());
                    },
                )
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_smt_insert_1,
    bench_smt_insert_100,
    bench_smt_insert_1000,
    bench_smt_prove,
    bench_smt_verify,
    bench_smt_root_after_updates,
    bench_smt_incremental_update,
    bench_smt_batch_prove,
    bench_smt_batch_verify,
    bench_smt_delete,
);
criterion_main!(benches);
