use std::hint::black_box;

use criterion::Criterion;
use criterion::Throughput;
use criterion::criterion_group;
use criterion::criterion_main;
use mm_binary::from_fixed_point;
use mm_orderbook::OrderBook;
use mm_orderbook::json_to_binary;
use mm_orderbook::process_orderbook_update;

// Sample Binance orderbook update (realistic structure)
const BINANCE_UPDATE_JSON: &str = r#"{
    "e":"depthUpdate",
    "E":1672531200000,
    "s":"BTCUSDT",
    "U":123456789,
    "u":123456799,
    "b":[
        ["50000.00","1.5"],
        ["49999.50","2.0"],
        ["49999.00","0.75"]
    ],
    "a":[
        ["50001.00","1.0"],
        ["50001.50","0.5"],
        ["50002.00","3.2"]
    ]
}"#;

const LARGE_BINANCE_UPDATE: &str = r#"{
    "e":"depthUpdate",
    "E":1672531200000,
    "s":"BTCUSDT",
    "U":123456789,
    "u":123456799,
    "b":[
        ["50000.00","1.5"],["49999.50","2.0"],["49999.00","0.75"],
        ["49998.50","1.2"],["49998.00","0.9"],["49997.50","2.5"],
        ["49997.00","1.8"],["49996.50","0.6"],["49996.00","1.1"],
        ["49995.50","2.2"],["49995.00","0.8"],["49994.50","1.4"],
        ["49994.00","0.7"],["49993.50","1.9"],["49993.00","1.3"],
        ["49992.50","0.95"],["49992.00","1.7"],["49991.50","1.25"],
        ["49991.00","0.85"],["49990.50","1.15"]
    ],
    "a":[
        ["50001.00","1.0"],["50001.50","0.5"],["50002.00","3.2"],
        ["50002.50","1.1"],["50003.00","0.8"],["50003.50","2.0"],
        ["50004.00","1.3"],["50004.50","0.6"],["50005.00","1.5"],
        ["50005.50","0.9"],["50006.00","2.1"],["50006.50","1.2"],
        ["50007.00","0.7"],["50007.50","1.4"],["50008.00","0.85"],
        ["50008.50","1.6"],["50009.00","0.95"],["50009.50","1.35"],
        ["50010.00","1.05"],["50010.50","0.75"]
    ]
}"#;

fn bench_json_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("json_parsing");

    // Standard JSON parsing with serde_json
    group.bench_function("serde_json_parse", |b| {
        b.iter(|| {
            let _: serde_json::Value = serde_json::from_str(black_box(BINANCE_UPDATE_JSON)).unwrap();
        });
    });

    // SIMD JSON parsing
    group.bench_function("simd_json_parse", |b| {
        b.iter(|| {
            let mut bytes = black_box(BINANCE_UPDATE_JSON).as_bytes().to_vec();
            let _: simd_json::BorrowedValue = simd_json::to_borrowed_value(&mut bytes).unwrap();
        });
    });

    group.finish();
}

fn bench_json_to_binary_conversion(c: &mut Criterion) {
    let mut group = c.benchmark_group("json_to_binary");
    group.throughput(Throughput::Bytes(BINANCE_UPDATE_JSON.len() as u64));

    group.bench_function("convert", |b| {
        b.iter(|| {
            json_to_binary(black_box(BINANCE_UPDATE_JSON)).unwrap();
        });
    });

    group.finish();
}

fn bench_orderbook_updates(c: &mut Criterion) {
    let mut group = c.benchmark_group("orderbook_updates");

    // SortedOrderBook (BTreeMap-based)
    group.bench_function("sorted_orderbook_update", |b| {
        b.iter(|| {
            let mut ob = OrderBook::new("BTCUSDT");
            process_orderbook_update(&mut ob, black_box(BINANCE_UPDATE_JSON)).unwrap();
        });
    });

    group.finish();
}

fn bench_orderbook_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("orderbook_operations");

    // Setup orderbook with some data
    let mut ob = OrderBook::new("BTCUSDT");
    process_orderbook_update(&mut ob, BINANCE_UPDATE_JSON).unwrap();

    group.bench_function("best_bid_ask", |b| {
        b.iter(|| {
            let _bid = ob.best_bid();
            let _ask = ob.best_ask();
        });
    });

    group.bench_function("mid_price", |b| {
        b.iter(|| {
            let _mid = ob.mid_price();
        });
    });

    group.bench_function("spread", |b| {
        b.iter(|| {
            let _spread = ob.spread();
        });
    });

    group.bench_function("top_5_levels", |b| {
        b.iter(|| {
            let _bids = ob.top_bids(5);
            let _asks = ob.top_asks(5);
        });
    });

    group.finish();
}

fn bench_large_orderbook_updates(c: &mut Criterion) {
    let mut group = c.benchmark_group("large_updates");
    group.throughput(Throughput::Bytes(LARGE_BINANCE_UPDATE.len() as u64));

    group.bench_function("sorted_orderbook_large", |b| {
        b.iter(|| {
            let mut ob = OrderBook::new("BTCUSDT");
            process_orderbook_update(&mut ob, black_box(LARGE_BINANCE_UPDATE)).unwrap();
        });
    });

    group.finish();
}

fn bench_end_to_end_pipeline(c: &mut Criterion) {
    let mut group = c.benchmark_group("end_to_end");
    group.throughput(Throughput::Bytes(BINANCE_UPDATE_JSON.len() as u64));

    group.bench_function("full_pipeline", |b| {
        b.iter(|| {
            // Simulate full pipeline: JSON -> Binary -> Orderbook update
            let mut ob = OrderBook::new("BTCUSDT");

            // 1. Parse JSON and update orderbook
            process_orderbook_update(&mut ob, black_box(BINANCE_UPDATE_JSON)).unwrap();

            // 2. Convert to binary message
            let binary_msg = json_to_binary(black_box(BINANCE_UPDATE_JSON)).unwrap();

            // 3. Extract values from binary
            let _bid = from_fixed_point(binary_msg.bid_price);
            let _ask = from_fixed_point(binary_msg.ask_price);

            // 4. Get orderbook state
            let _mid = ob.mid_price();
            let _spread = ob.spread();
        });
    });

    group.finish();
}

fn bench_message_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("message_sizes");

    // Compare JSON vs Binary sizes
    group.bench_function("json_size", |b| {
        b.iter(|| black_box(BINANCE_UPDATE_JSON.len()));
    });

    group.bench_function("binary_size", |b| {
        b.iter(|| {
            let binary_msg = json_to_binary(BINANCE_UPDATE_JSON).unwrap();
            black_box(std::mem::size_of_val(&binary_msg))
        });
    });

    // Print actual sizes
    let json_size = BINANCE_UPDATE_JSON.len();
    let binary_msg = json_to_binary(BINANCE_UPDATE_JSON).unwrap();
    let binary_size = std::mem::size_of_val(&binary_msg);
    let compression_ratio = json_size as f64 / binary_size as f64;

    println!("\n=== Message Size Comparison ===");
    println!("JSON size:     {} bytes", json_size);
    println!("Binary size:   {} bytes", binary_size);
    println!("Compression:   {:.2}x smaller", compression_ratio);
    println!("Space saved:   {:.1}%", (1.0 - (binary_size as f64 / json_size as f64)) * 100.0);

    group.finish();
}

criterion_group!(
    benches,
    bench_json_parsing,
    bench_json_to_binary_conversion,
    bench_orderbook_updates,
    bench_orderbook_operations,
    bench_large_orderbook_updates,
    bench_end_to_end_pipeline,
    bench_message_sizes
);

criterion_main!(benches);
