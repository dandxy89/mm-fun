# Docker Deployment

This directory contains Docker configurations for running the market making system.

## Architecture

The system consists of the following services:

1. **aeronmd** - Aeron Media Driver for ultra-low latency IPC messaging
2. **mm_collector** - WebSocket collector for market data (depth + trades) from Binance
3. **mm_pricing** - Pricing engine that calculates fair value, drift, and volatility
4. **mm_strategy** - Market making strategy that generates quotes
5. **mm_simulator** - Order fill simulator that simulates fills based on market crossing

## Quick Start

```bash
# Build all images
docker-compose build

# Start all services
docker-compose up -d

# View logs
docker-compose logs -f

# View logs for specific service
docker-compose logs -f mm_strategy

# Stop all services
docker-compose down

# Restart a specific service
docker-compose restart mm_strategy
```

## Configuration

Configuration files are located in the `config/` directory:

- `config/strategy.toml` - Strategy parameters (spreads, inventory limits, etc.)
- `config/simulator.toml` - Simulator parameters (latency, fill probability, etc.)

To modify configuration:
1. Edit the config files
2. Restart the affected services: `docker-compose restart mm_strategy mm_simulator`

## Service Dependencies

```
aeronmd (Aeron Media Driver)
    ↓
mm_collector (WebSocket → Aeron)
    ↓
mm_pricing (Market data → Fair value + drift)
    ↓
mm_strategy (Fair value → Quotes)
    ↓
mm_simulator (Quotes → Fills)
```

## Monitoring

Logs are written to `./logs/` directory with hourly rotation:
- `mm_collector.log.YYYY-MM-DD-HH`
- `mm_pricing.log.YYYY-MM-DD-HH`
- `mm_strategy.log.YYYY-MM-DD-HH`
- `mm_simulator.log.YYYY-MM-DD-HH`

## Resource Requirements

- **Memory**: 512MB shared memory for Aeron
- **CPU**: 4+ cores recommended for best performance
- **Network**: Stable internet connection for WebSocket data

## Aeron IPC

All services communicate via Aeron IPC (shared memory):
- Stream 10: Market data (orderbook updates)
- Stream 11: State updates
- Stream 12: Heartbeats
- Stream 13: Trade data
- Stream 14: Pricing output
- Stream 15: Strategy quotes
- Stream 16: Order fills
- Stream 17: Position updates

## Troubleshooting

### Service won't start
Check that aeronmd is healthy:
```bash
docker-compose ps aeronmd
docker-compose logs aeronmd
```

### No market data
Check mm_collector logs:
```bash
docker-compose logs mm_collector
```

### No quotes generated
1. Check mm_pricing is publishing
2. Check mm_strategy logs for errors
3. Verify orderbook is synchronized

### Simulator not filling orders
1. Check orderbook is synchronized
2. Check quotes are being received
3. Review simulator config (fill_probability_factor)

## Development

To rebuild after code changes:
```bash
# Rebuild specific service
docker-compose build mm_strategy

# Rebuild all services
docker-compose build

# Rebuild without cache
docker-compose build --no-cache
```

## Production Considerations

1. **Use UDP mode** for Aeron instead of IPC if running services on different machines
2. **Tune Aeron parameters** for your network and latency requirements
3. **Monitor shared memory usage** (`/dev/shm/aeron`)
4. **Set proper resource limits** in docker-compose.yml
5. **Use production logging levels** (RUST_LOG=warn or RUST_LOG=error)
6. **Implement health checks** for all services
7. **Set up log aggregation** (e.g., ELK stack, Grafana Loki)
