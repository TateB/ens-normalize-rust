use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use ens_normalize::{ens_normalize, ens_tokenize, nfc};
use std::hint::black_box;

const NAMES: &[&str] = &[
    "raffy.eth",
    "RaFfY.eth",
    "vitalik.eth",
    "👩🏽‍⚕️.eth",
    "0️⃣1️⃣2️⃣3️⃣.eth",
    "àáâãāäåăąćĉčç.eth",
    "ガギグゲゴザジズゼゾ.eth",
    "nowzad.loopring.eth",
];

fn bench_normalize(c: &mut Criterion) {
    let mut group = c.benchmark_group("ens_normalize");
    for name in NAMES {
        group.bench_with_input(BenchmarkId::from_parameter(name), name, |b, name| {
            b.iter(|| ens_normalize(black_box(name)).unwrap())
        });
    }
    group.finish();
}

fn bench_tokenize(c: &mut Criterion) {
    let mut group = c.benchmark_group("ens_tokenize");
    for name in NAMES {
        group.bench_with_input(BenchmarkId::from_parameter(name), name, |b, name| {
            b.iter(|| ens_tokenize(black_box(name)))
        });
    }
    group.finish();
}

fn bench_nfc(c: &mut Criterion) {
    let cps = "RaFfY àáâãāäåăąćĉč 👩🏽‍⚕️"
        .chars()
        .map(|ch| ch as u32)
        .collect::<Vec<_>>();
    c.bench_function("nfc/mixed", |b| b.iter(|| nfc(black_box(&cps))));
}

criterion_group!(benches, bench_normalize, bench_tokenize, bench_nfc);
criterion_main!(benches);
