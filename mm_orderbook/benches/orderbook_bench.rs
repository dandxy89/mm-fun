use std::f64::consts::PI;
use std::hint::black_box;

use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;
use mm_binary::FIXED_POINT_MULTIPLIER;
use mm_orderbook::OrderBook;

fn bench_orderbook_monotonic(c: &mut Criterion) {
    c.bench_function("orderbook_monotonic", |b| {
        let mut ob = OrderBook::new("BTCUSDT");
        let base_price = 50_000 * FIXED_POINT_MULTIPLIER;

        b.iter(|| {
            for i in 0..1000 {
                let price = base_price + (i * FIXED_POINT_MULTIPLIER);
                let qty_1_5 = 150_000_000; // 1.5
                let qty_1_0 = 100_000_000; // 1.0
                ob.update_bid(black_box(price - 100 * FIXED_POINT_MULTIPLIER), black_box(qty_1_5));
                ob.update_ask(black_box(price + 100 * FIXED_POINT_MULTIPLIER), black_box(qty_1_0));
            }
        });
    });
}

fn bench_orderbook_sin(c: &mut Criterion) {
    c.bench_function("orderbook_sin", |b| {
        b.iter(|| {
            let mut ob = OrderBook::new("BTCUSDT");
            let base_price = 50_000 * FIXED_POINT_MULTIPLIER;
            let amplitude = 500 * FIXED_POINT_MULTIPLIER;

            // Insert prices following a sin curve
            for i in 0..1000 {
                let angle = (i as f64 / 100.0) * 2.0 * PI;
                let sin_component = (amplitude as f64 * angle.sin()) as i64;
                let price = base_price + sin_component;
                let qty_1_5 = 150_000_000; // 1.5
                let qty_1_0 = 100_000_000; // 1.0
                ob.update_bid(price - 100 * FIXED_POINT_MULTIPLIER, black_box(qty_1_5));
                ob.update_ask(price + 100 * FIXED_POINT_MULTIPLIER, black_box(qty_1_0));
            }

            black_box(ob)
        })
    });
}

fn bench_top_bids(c: &mut Criterion) {
    let mut ob = OrderBook::new("BTCUSDT");
    let base_price = 50_000 * FIXED_POINT_MULTIPLIER;
    let qty_1_0 = 100_000_000; // 1.0
    for i in 0..5000 {
        ob.update_bid(base_price + (i * FIXED_POINT_MULTIPLIER), qty_1_0);
    }

    c.bench_function("top_bids_10", |b| {
        b.iter(|| {
            black_box(ob.top_bids(10));
        });
    });
}

criterion_group!(benches, bench_orderbook_monotonic, bench_orderbook_sin, bench_top_bids);
criterion_main!(benches);
