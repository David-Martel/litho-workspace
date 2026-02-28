use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use litho_qmd_storage::fast_table::FastHashTable;
use std::collections::{BTreeMap, HashMap};

fn bench_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("index_insert");
    for size in [1_000usize, 10_000usize, 50_000usize] {
        let keys = (0..size).map(|i| format!("k-{i}")).collect::<Vec<_>>();

        group.bench_with_input(BenchmarkId::new("fast_table", size), &size, |b, _| {
            b.iter(|| {
                let mut table = FastHashTable::<String, usize>::with_capacity(size * 2);
                for (i, key) in keys.iter().enumerate() {
                    let _ = table.insert(black_box(key.clone()), black_box(i));
                }
                black_box(table);
            });
        });

        group.bench_with_input(BenchmarkId::new("std_hashmap", size), &size, |b, _| {
            b.iter(|| {
                let mut table = HashMap::<String, usize>::with_capacity(size * 2);
                for (i, key) in keys.iter().enumerate() {
                    table.insert(black_box(key.clone()), black_box(i));
                }
                black_box(table);
            });
        });

        group.bench_with_input(BenchmarkId::new("std_btreemap", size), &size, |b, _| {
            b.iter(|| {
                let mut table = BTreeMap::<String, usize>::new();
                for (i, key) in keys.iter().enumerate() {
                    table.insert(black_box(key.clone()), black_box(i));
                }
                black_box(table);
            });
        });
    }
    group.finish();
}

fn bench_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("index_lookup");
    for size in [1_000usize, 10_000usize, 50_000usize] {
        let keys = (0..size).map(|i| format!("k-{i}")).collect::<Vec<_>>();
        let lookup_keys = keys
            .iter()
            .step_by(7)
            .cloned()
            .chain((0..500).map(|i| format!("missing-{i}")))
            .collect::<Vec<_>>();

        let mut fast = FastHashTable::<String, usize>::with_capacity(size * 2);
        let mut hmap = HashMap::<String, usize>::with_capacity(size * 2);
        let mut btree = BTreeMap::<String, usize>::new();
        for (i, key) in keys.iter().enumerate() {
            let _ = fast.insert(key.clone(), i);
            hmap.insert(key.clone(), i);
            btree.insert(key.clone(), i);
        }

        group.bench_with_input(BenchmarkId::new("fast_table", size), &size, |b, _| {
            b.iter(|| {
                let mut found = 0usize;
                for key in &lookup_keys {
                    if fast.get(black_box(key)).is_some() {
                        found += 1;
                    }
                }
                black_box(found);
            });
        });

        group.bench_with_input(BenchmarkId::new("std_hashmap", size), &size, |b, _| {
            b.iter(|| {
                let mut found = 0usize;
                for key in &lookup_keys {
                    if hmap.contains_key(black_box(key)) {
                        found += 1;
                    }
                }
                black_box(found);
            });
        });

        group.bench_with_input(BenchmarkId::new("std_btreemap", size), &size, |b, _| {
            b.iter(|| {
                let mut found = 0usize;
                for key in &lookup_keys {
                    if btree.contains_key(black_box(key)) {
                        found += 1;
                    }
                }
                black_box(found);
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_insert, bench_lookup);
criterion_main!(benches);
