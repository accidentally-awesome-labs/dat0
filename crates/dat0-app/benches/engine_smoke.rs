use criterion::{Criterion, black_box, criterion_group, criterion_main};

fn smoke_no_op(c: &mut Criterion) {
    c.bench_function("smoke/no_op", |b| b.iter(|| black_box(2 + 2)));
}

criterion_group!(benches, smoke_no_op);
criterion_main!(benches);
