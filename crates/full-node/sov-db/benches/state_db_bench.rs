extern crate criterion;

use std::sync::{Arc, RwLock};

use criterion::measurement::WallTime;
use criterion::{
    black_box, criterion_group, criterion_main, BenchmarkGroup, BenchmarkId, Criterion,
};
use jmt::{JellyfishMerkleTree, KeyHash, Version};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rockbound::cache::cache_container::CacheContainer;
use rockbound::cache::cache_db::CacheDb;
use rockbound::cache::change_set::ChangeSet;
use rockbound::{ReadOnlyLock, SchemaBatch, DB};
use sov_db::namespaces::{KernelNamespace, Namespace, UserNamespace};
use sov_db::state_db::{JmtHandler, StateDb};

// TODO: Improve for collisions
fn generate_random_bytes(count: usize) -> Vec<Vec<u8>> {
    let seed: [u8; 32] = [1; 32];

    // Create an RNG with the specified seed
    let mut rng = StdRng::from_seed(seed);

    let mut samples: Vec<Vec<u8>> = Vec::with_capacity(count);

    for _ in 0..count {
        let inner_vec_size = rng.gen_range(32..=256);
        let storage_key: Vec<u8> = (0..inner_vec_size).map(|_| rng.gen::<u8>()).collect();
        samples.push(storage_key);
    }

    samples
}

struct TestData {
    largest_key: Vec<u8>,
    random_key: Vec<u8>,
    non_existing_key: Vec<u8>,
    db: StateDb,
}

fn put_data<N: Namespace>(
    state_db: &StateDb,
    raw_data: Vec<Vec<u8>>,
    version: Version,
) -> SchemaBatch {
    let mut key_preimages = Vec::with_capacity(raw_data.len());
    let mut batch = Vec::with_capacity(raw_data.len());

    for chunk in raw_data.chunks(2) {
        let key = &chunk[0];
        let value = chunk[1].clone();
        let key_hash = KeyHash::with::<sha2::Sha256>(&key);
        key_preimages.push((key_hash, key));
        batch.push((key_hash, Some(value)));
    }

    let mut preimages_batch = StateDb::materialize_preimages::<N>(key_preimages).unwrap();

    let db_handler: JmtHandler<'_, N> = state_db.get_jmt_handler();

    let jmt = JellyfishMerkleTree::<JmtHandler<N>, sha2::Sha256>::new(&db_handler);

    let (_new_root, _update_proof, tree_update) = jmt
        .put_value_set_with_proof(batch, version)
        .expect("JMT update must succeed");
    let node_batch = state_db
        .materialize_node_batch::<N>(&tree_update.node_batch, Some(&preimages_batch))
        .unwrap();
    preimages_batch.merge(node_batch);

    preimages_batch
}

fn prepare_data(size: usize, rocksdb: DB) -> TestData {
    assert!(size > 0, "Do not generate empty TestData");
    let to_parent = Arc::new(RwLock::new(Default::default()));
    let cache_container = Arc::new(RwLock::new(CacheContainer::new(
        rocksdb,
        to_parent.clone().into(),
    )));
    let manager = ReadOnlyLock::new(cache_container.clone());
    let cache_db = CacheDb::new(0, manager);
    let state_db = StateDb::with_cache_db(cache_db).unwrap();

    let mut raw_data = generate_random_bytes(size * 2 + 1);
    let non_existing_key = raw_data.pop().unwrap();
    let random_key = raw_data.first().unwrap().clone();
    let largest_key = raw_data
        .iter()
        .enumerate()
        .filter_map(|(i, elem)| if i % 2 == 0 { Some(elem) } else { None })
        .max()
        .unwrap()
        .clone();

    let version = 1;
    let mut user_data = put_data::<UserNamespace>(&state_db, raw_data.clone(), version);
    // TODO: Fails if kernel data does not have anything https://github.com/Sovereign-Labs/sovereign-sdk-wip/issues/648
    let kernel_data = put_data::<KernelNamespace>(&state_db, vec![vec![1], vec![2]], version);
    user_data.merge(kernel_data);

    {
        let mut cache_container = cache_container.write().unwrap();
        let change_set = ChangeSet::new_with_operations(0, user_data);
        cache_container.add_snapshot(change_set).unwrap();
        cache_container.commit_snapshot(&0).unwrap();
    }

    // re-initialize `StateDb` so latest version is updated.
    let manager = ReadOnlyLock::new(cache_container.clone());
    let cache_db = CacheDb::new(0, manager);
    let db = StateDb::with_cache_db(cache_db).unwrap();
    let version = db.get_next_version() - 1;
    for chunk in raw_data.chunks(2) {
        let key = &chunk[0];
        let value = chunk[1].clone();
        let res = db
            .get_value_option_by_key::<UserNamespace>(version, key)
            .unwrap();
        assert_eq!(Some(value), res);
    }

    let random_value = db
        .get_value_option_by_key::<UserNamespace>(version, &random_key)
        .unwrap();
    assert!(random_value.is_some());

    TestData {
        largest_key,
        random_key,
        non_existing_key,
        db,
    }
}

fn bench_random_read(g: &mut BenchmarkGroup<WallTime>, size: usize) {
    let tempdir = tempfile::tempdir().unwrap();
    let state_rocksdb = StateDb::get_rockbound_options()
        .default_setup_db_in_path(tempdir.path())
        .unwrap();
    let TestData { db, random_key, .. } = prepare_data(size, state_rocksdb);
    let version = db.get_next_version() - 1;
    g.bench_with_input(
        BenchmarkId::new("bench_random_read", size),
        &(db, random_key, version),
        |b, i| {
            b.iter(|| {
                let (db, key, version) = i;
                let result = black_box(
                    db.get_value_option_by_key::<UserNamespace>(*version, key)
                        .unwrap(),
                );
                assert!(result.is_some());
                black_box(result);
            });
        },
    );
}

fn bench_largest_read(g: &mut BenchmarkGroup<WallTime>, size: usize) {
    let tempdir = tempfile::tempdir().unwrap();
    let state_rocksdb = StateDb::get_rockbound_options()
        .default_setup_db_in_path(tempdir.path())
        .unwrap();
    let TestData {
        db,
        largest_key: _largest_key,
        ..
    } = prepare_data(size, state_rocksdb);
    let version = db.get_next_version() - 1;
    g.bench_with_input(
        BenchmarkId::new("bench_largest_read", size),
        &(db, _largest_key, version),
        |b, i| {
            b.iter(|| {
                let (db, key, version) = i;
                let result = black_box(
                    db.get_value_option_by_key::<UserNamespace>(*version, key)
                        .unwrap(),
                );
                assert!(result.is_some());
                black_box(result);
            });
        },
    );
}

fn bench_not_found_read(g: &mut BenchmarkGroup<WallTime>, size: usize) {
    let tempdir = tempfile::tempdir().unwrap();
    let state_rocksdb = StateDb::get_rockbound_options()
        .default_setup_db_in_path(tempdir.path())
        .unwrap();
    let TestData {
        db,
        non_existing_key,
        ..
    } = prepare_data(size, state_rocksdb);
    let version = db.get_next_version() - 1;
    g.bench_with_input(
        BenchmarkId::new("bench_not_found_read", size),
        &(db, non_existing_key, version),
        |b, i| {
            b.iter(|| {
                let (db, key, version) = i;
                let result = black_box(
                    db.get_value_option_by_key::<UserNamespace>(*version, key)
                        .unwrap(),
                );
                assert!(result.is_none());
                black_box(result);
            });
        },
    );
}

fn state_db_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("StateDb");
    group.noise_threshold(0.3);
    for size in [1000, 10_000, 30_000] {
        bench_random_read(&mut group, size);
        bench_not_found_read(&mut group, size);
        bench_largest_read(&mut group, size);
    }
    group.finish();
}

criterion_group!(benches, state_db_benchmark);
criterion_main!(benches);
