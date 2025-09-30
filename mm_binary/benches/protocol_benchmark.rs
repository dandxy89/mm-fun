use std::hint::black_box;

use criterion::Criterion;
use criterion::criterion_group;
use criterion::criterion_main;
use mm_binary::CompressedString;
use mm_binary::Exchange;
use mm_binary::MarketDataMessage;
use mm_binary::PricingOutputMessage;
use mm_binary::from_fixed_point;
use mm_binary::messages::UpdateType;
use mm_binary::to_fixed_point;

fn bench_string_compression(c: &mut Criterion) {
    c.bench_function("compress BTCUSDT", |b| b.iter(|| CompressedString::from_str(black_box("BTCUSDT"))));

    c.bench_function("compress ETH-USDT", |b| b.iter(|| CompressedString::from_str(black_box("ETH-USDT"))));

    c.bench_function("decompress BTCUSDT", |b| {
        let (compressed, scheme) = CompressedString::from_str("BTCUSDT").unwrap();
        b.iter(|| compressed.decode(black_box(scheme)))
    });
}

fn bench_market_data_message(c: &mut Criterion) {
    let (symbol, encoding) = CompressedString::from_str("BTCUSDT").unwrap();

    c.bench_function("create market data message", |b| {
        b.iter(|| {
            MarketDataMessage::new(
                black_box(Exchange::Binance),
                black_box(UpdateType::Snapshot),
                black_box(symbol),
                black_box(encoding),
                black_box(1_000_000_000),
                black_box(to_fixed_point(50000.0)),
                black_box(to_fixed_point(50001.0)),
                black_box(to_fixed_point(10.5)),
                black_box(to_fixed_point(10.3)),
            )
        })
    });

    let msg = MarketDataMessage::new(
        Exchange::Binance,
        UpdateType::Snapshot,
        symbol,
        encoding,
        1_000_000_000,
        to_fixed_point(50000.0),
        to_fixed_point(50001.0),
        to_fixed_point(10.5),
        to_fixed_point(10.3),
    );
    let bytes = msg.to_bytes();

    c.bench_function("deserialize market data message", |b| b.iter(|| MarketDataMessage::from_bytes(black_box(&bytes))));

    c.bench_function("zero-copy deserialize", |b| b.iter(|| unsafe { MarketDataMessage::from_bytes_unchecked(black_box(&bytes)) }));
}

fn bench_pricing_message(c: &mut Criterion) {
    let (symbol, encoding) = CompressedString::from_str("BTCUSDT").unwrap();

    c.bench_function("create pricing message", |b| {
        b.iter(|| {
            PricingOutputMessage::new(
                black_box(1),
                black_box(symbol),
                black_box(encoding),
                black_box(1_000_000_000),
                black_box(to_fixed_point(50000.5)),
                black_box(to_fixed_point(0.95)),
                black_box(to_fixed_point(0.015)),
            )
        })
    });

    let msg =
        PricingOutputMessage::new(1, symbol, encoding, 1_000_000_000, to_fixed_point(50000.5), to_fixed_point(0.95), to_fixed_point(0.015));
    let bytes = msg.to_bytes();

    c.bench_function("deserialize pricing message", |b| b.iter(|| PricingOutputMessage::from_bytes(black_box(&bytes))));
}

fn bench_fixed_point(c: &mut Criterion) {
    c.bench_function("to_fixed_point", |b| b.iter(|| to_fixed_point(black_box(12345.6789))));

    c.bench_function("from_fixed_point", |b| b.iter(|| from_fixed_point(black_box(1234567890))));
}

criterion_group!(benches, bench_string_compression, bench_market_data_message, bench_pricing_message, bench_fixed_point);
criterion_main!(benches);
