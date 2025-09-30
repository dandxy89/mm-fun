use std::f64::consts::PI;
use std::hint::black_box;

use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;
use mm_orderbook::OrderBook;

fn bench_orderbook_monotonic(c: &mut Criterion) {
    c.bench_function("orderbook_monotonic", |b| {
        let mut ob = OrderBook::new("BTCUSDT");
        let base_price = 50_000.0;

        b.iter(|| {
            for i in 0..1000 {
                let price = base_price + i as f64;
                ob.update_bid(black_box(price - 100.0), black_box(1.5));
                ob.update_ask(black_box(price + 100.0), black_box(1.0));
            }
        });
    });
}

fn bench_orderbook_sin(c: &mut Criterion) {
    c.bench_function("orderbook_sin", |b| {
        b.iter(|| {
            let mut ob = OrderBook::new("BTCUSDT");
            let base_price = 50_000.0;
            let amplitude = 500.0;

            // Insert prices following a sin curve
            for i in 0..1000 {
                let angle = (i as f64 / 100.0) * 2.0 * PI;
                let price = base_price + amplitude * angle.sin();
                ob.update_bid(price - 100.0, black_box(1.5));
                ob.update_ask(price + 100.0, black_box(1.0));
            }

            black_box(ob)
        })
    });
}

fn bench_top_bids(c: &mut Criterion) {
    let mut ob = OrderBook::new("BTCUSDT");
    for i in 0..5000 {
        ob.update_bid(50_000.0 + i as f64, 1.0);
    }

    c.bench_function("top_bids_10", |b| {
        b.iter(|| {
            black_box(ob.top_bids(10));
        });
    });
}

criterion_group!(benches, bench_orderbook_monotonic, bench_orderbook_sin, bench_top_bids);
criterion_main!(benches);
