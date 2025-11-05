#!/usr/bin/env python3
"""
Generate sample historical data for backtesting
Creates CSV files with orderbook and trade data
"""

import csv
import random
from datetime import datetime, timedelta

def generate_orderbook_data(symbol, start_time, duration_hours, output_file):
    """Generate sample orderbook CSV data"""

    fieldnames = [
        'timestamp_ms', 'symbol',
        'bid_price_1', 'bid_qty_1', 'bid_price_2', 'bid_qty_2', 'bid_price_3', 'bid_qty_3',
        'ask_price_1', 'ask_qty_1', 'ask_price_2', 'ask_qty_2', 'ask_price_3', 'ask_qty_3'
    ]

    with open(output_file, 'w', newline='') as csvfile:
        writer = csv.DictWriter(csvfile, fieldnames=fieldnames)
        writer.writeheader()

        current_time = start_time
        end_time = start_time + timedelta(hours=duration_hours)

        mid_price = 50000.0  # Starting BTC price

        # Generate updates every 100ms
        while current_time < end_time:
            timestamp_ms = int(current_time.timestamp() * 1000)

            # Random walk for mid price
            mid_price += random.uniform(-10, 10)
            mid_price = max(40000, min(60000, mid_price))  # Keep in reasonable range

            # Generate spread
            spread = random.uniform(1, 5)  # $1-5 spread

            row = {
                'timestamp_ms': timestamp_ms,
                'symbol': symbol,
                'bid_price_1': round(mid_price - spread/2, 2),
                'bid_qty_1': round(random.uniform(0.5, 5.0), 4),
                'bid_price_2': round(mid_price - spread/2 - 1, 2),
                'bid_qty_2': round(random.uniform(1.0, 10.0), 4),
                'bid_price_3': round(mid_price - spread/2 - 2, 2),
                'bid_qty_3': round(random.uniform(2.0, 15.0), 4),
                'ask_price_1': round(mid_price + spread/2, 2),
                'ask_qty_1': round(random.uniform(0.5, 5.0), 4),
                'ask_price_2': round(mid_price + spread/2 + 1, 2),
                'ask_qty_2': round(random.uniform(1.0, 10.0), 4),
                'ask_price_3': round(mid_price + spread/2 + 2, 2),
                'ask_qty_3': round(random.uniform(2.0, 15.0), 4),
            }

            writer.writerow(row)

            # Next update (100ms)
            current_time += timedelta(milliseconds=100)

    print(f"Generated orderbook data: {output_file}")

def generate_trade_data(symbol, start_time, duration_hours, output_file):
    """Generate sample trade CSV data"""

    fieldnames = ['timestamp_ms', 'symbol', 'trade_id', 'price', 'quantity', 'is_buyer_maker']

    with open(output_file, 'w', newline='') as csvfile:
        writer = csv.DictWriter(csvfile, fieldnames=fieldnames)
        writer.writeheader()

        current_time = start_time
        end_time = start_time + timedelta(hours=duration_hours)

        mid_price = 50000.0
        trade_id = 1

        # Generate trades every 500ms on average
        while current_time < end_time:
            timestamp_ms = int(current_time.timestamp() * 1000)

            # Random walk
            mid_price += random.uniform(-10, 10)
            mid_price = max(40000, min(60000, mid_price))

            # Trade price slightly off mid
            trade_price = mid_price + random.uniform(-2, 2)

            row = {
                'timestamp_ms': timestamp_ms,
                'symbol': symbol,
                'trade_id': trade_id,
                'price': round(trade_price, 2),
                'quantity': round(random.uniform(0.01, 1.0), 4),
                'is_buyer_maker': random.choice([True, False]),
            }

            writer.writerow(row)

            trade_id += 1

            # Next trade (random interval around 500ms)
            current_time += timedelta(milliseconds=random.randint(100, 1000))

    print(f"Generated trade data: {output_file}")

def main():
    import argparse

    parser = argparse.ArgumentParser(description='Generate sample historical data for backtesting')
    parser.add_argument('--symbol', default='BTCUSDT', help='Trading symbol')
    parser.add_argument('--hours', type=int, default=24, help='Duration in hours')
    parser.add_argument('--output-dir', default='./data', help='Output directory')

    args = parser.parse_args()

    import os
    os.makedirs(args.output_dir, exist_ok=True)

    # Start from yesterday
    start_time = datetime.now() - timedelta(days=1)

    # Generate data
    orderbook_file = os.path.join(args.output_dir, f"{args.symbol.lower()}_orderbook.csv")
    trades_file = os.path.join(args.output_dir, f"{args.symbol.lower()}_trades.csv")

    print(f"Generating {args.hours} hours of sample data for {args.symbol}...")
    generate_orderbook_data(args.symbol, start_time, args.hours, orderbook_file)
    generate_trade_data(args.symbol, start_time, args.hours, trades_file)

    print("\nDone! Run backtest with:")
    print(f"  cargo run --release --bin mm_backtest")

if __name__ == '__main__':
    main()
