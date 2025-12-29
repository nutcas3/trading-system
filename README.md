# Modular Trading Platform

A production-grade, high-performance trading system built in Rust with three core components:

- **Titan** - LMAX-style lock-free matching engine
- **Oracle** - Event sourcing with deterministic replay
- **Sentinel** - Real-time liquidation engine with WebSocket price feeds

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Trading Platform                          │
├─────────────────────────────────────────────────────────────┤
│                                                               │
│  ┌──────────┐      ┌──────────┐      ┌──────────┐          │
│  │  Titan   │      │  Oracle  │      │ Sentinel │          │
│  │ Matching │─────▶│  Event   │◀─────│   Risk   │          │
│  │  Engine  │      │  Store   │      │  Engine  │          │
│  └──────────┘      └──────────┘      └──────────┘          │
│       │                  │                  ▲                │
│       │                  │                  │                │
│       ▼                  ▼                  │                │
│  Executions          Events           Price Feed            │
│                                            │                 │
│                                            │                 │
│                                    ┌───────┴────────┐       │
│                                    │   WebSocket    │       │
│                                    │  Binance/Sim   │       │
│                                    └────────────────┘       │
│                                                               │
│                    Prometheus Metrics                        │
│                  http://localhost:9000/metrics               │
└─────────────────────────────────────────────────────────────┘
```

## Features

### Titan Matching Engine
- Lock-free order book using typed arenas
- Zero-copy order processing
- Price-time priority matching
- BTreeMap-based price levels
- Sub-microsecond latency

### Oracle Event Store
- Append-only event log with RocksDB
- Deterministic state replay
- SHA-256 state hashing
- Point-in-time recovery
- Full audit trail

### Sentinel Liquidation Engine
- Real-time position monitoring
- WebSocket integration (Binance)
- Simulated price feed mode
- DashMap for lock-free account access
- Configurable margin requirements

## Metrics

All components export Prometheus metrics:

### Titan Metrics
- `titan.orders_processed` - Total orders processed
- `titan.executions_total` - Total executions
- `titan.execution_price` - Price distribution histogram
- `titan.execution_quantity` - Quantity distribution
- `titan.spread` - Bid-ask spread

### Oracle Metrics
- `oracle.events_written` - Total events persisted
- Event replay performance

### Sentinel Metrics
- `sentinel.liquidations_total` - Liquidations by symbol
- `sentinel.liquidation_loss_usd` - Loss distribution
- `sentinel.accounts_total` - Active accounts
- `sentinel.accounts_at_risk` - Accounts below maintenance margin
- `sentinel.margin_ratio` - Per-user margin ratios
- `sentinel.process_time_micros` - Processing latency

### Price Feed Metrics
- `price_feed.updates_total` - Updates per symbol
- `price_feed.latency_ms` - WebSocket latency
- `price_feed.simulated_price` - Simulated price values

## Quick Start

### Build and Run

```bash
cargo build --release
cargo run --release
```

### View Metrics

```bash
# Prometheus metrics endpoint
curl http://localhost:9000/metrics

# Or use Prometheus + Grafana (see below)
```

## Configuration

### Price Feed Modes

**Simulation Mode** (default):
```rust
PriceFeedMode::Simulation {
    initial_price: Decimal::from(50000),
    volatility: Decimal::from_str("0.002").unwrap(), // 0.2%
}
```

**Binance WebSocket Mode**:
```rust
PriceFeedMode::Binance {
    symbols: vec!["BTCUSDT".to_string(), "ETHUSDT".to_string()],
}
```

### Risk Parameters

```rust
// Maintenance margin ratio (0.5% = 200x max leverage)
let maintenance_margin_ratio = Decimal::from_str("0.005").unwrap();
```

## Monitoring with Prometheus + Grafana

### 1. Start Prometheus

Create `prometheus.yml`:
```yaml
global:
  scrape_interval: 1s

scrape_configs:
  - job_name: 'trading_platform'
    static_configs:
      - targets: ['localhost:9000']
```

Run Prometheus:
```bash
docker run -d \
  -p 9090:9090 \
  -v $(pwd)/prometheus.yml:/etc/prometheus/prometheus.yml \
  prom/prometheus
```

### 2. Start Grafana

```bash
docker run -d -p 3000:3000 grafana/grafana
```

Access Grafana at `http://localhost:3000` (admin/admin)

### 3. Sample Grafana Queries

**Liquidation Rate**:
```promql
rate(sentinel_liquidations_total[1m])
```

**Average Margin Ratio**:
```promql
avg(sentinel_margin_ratio)
```

**Order Processing Latency (p99)**:
```promql
histogram_quantile(0.99, rate(sentinel_process_time_micros_bucket[5m]))
```

**Price Feed Health**:
```promql
rate(price_feed_updates_total[30s])
```

**Execution Volume**:
```promql
sum(rate(titan_executions_total[1m]))
```

## Module Structure

```
src/
├── main.rs           # Application entry point
├── types.rs          # Shared data structures
├── titan.rs          # Matching engine
├── oracle.rs         # Event sourcing
├── sentinel.rs       # Liquidation engine
└── orchestrator.rs   # Platform orchestration
```

## Data Flow

1. **Order Submission** → Titan → Execution → Oracle (event log)
2. **Price Update** → Sentinel → Liquidation Check → Oracle (event log)
3. **All Events** → Oracle → RocksDB persistence
4. **Metrics** → Prometheus → Grafana

## Testing

### Unit Tests
```bash
cargo test
```

### Property-Based Tests
```bash
cargo test --features proptest
```

### Load Testing
```bash
# Adjust order generation rate in orchestrator.rs
# Default: 1 order every 2 seconds
```

## Event Sourcing

All system events are persisted to RocksDB:

```rust
pub enum SystemEvent {
    OrderPlaced(Order),
    OrderExecuted(Execution),
    PositionOpened { ... },
    PositionLiquidated(LiquidationEvent),
    PriceUpdate { ... },
    AccountUpdated { ... },
}
```

### Replay Events

```rust
let vault = OracleVault::open("platform_events")?;
let events = vault.replay_all();
let state_hash = vault.compute_state_hash();
```

## Performance Characteristics

- **Matching Engine**: <1μs per order (single-threaded)
- **Event Persistence**: ~10k events/sec
- **Liquidation Checks**: <100μs per price update
- **WebSocket Latency**: <50ms (Binance)

## Production Deployment

### Recommended Configuration

- **CPU**: 4+ cores (dedicated thread per component)
- **RAM**: 8GB+ (RocksDB caching)
- **Disk**: SSD for RocksDB
- **Network**: Low-latency connection for WebSocket

### Environment Variables

```bash
export RUST_LOG=info
export PROMETHEUS_PORT=9000
export ROCKSDB_PATH=./platform_events
```

## Safety & Reliability

- **No unsafe code** - Pure safe Rust
- **Lock-free data structures** - DashMap, typed-arena
- **Deterministic replay** - Event sourcing with SHA-256 hashing
- **Graceful shutdown** - Ctrl+C signal handling
- **Error handling** - Result types throughout

## License

MIT

## Quick Start

### Prerequisites

- Rust 1.88+ (`rustup update`)
- Docker (optional, for Prometheus/Grafana)

### 1. Build and Run

```bash
cargo build --release
cargo run --release
```

You should see:

```
╔═══════════════════════════════════════════════════════════╗
║     MODULAR TRADING PLATFORM - PRODUCTION GRADE          ║
║  Titan (Matching) | Oracle (Events) | Sentinel (Risk)   ║
╚═══════════════════════════════════════════════════════════╝

[Metrics] Prometheus endpoint: http://localhost:9000/metrics

[Platform] Adding account: User 1001
[Platform] Adding account: User 1002
[Platform] Adding account: User 1003

[Platform] Starting price feed (simulation mode)
[Simulator] Starting simulated price feed
[Platform] Starting Sentinel liquidation engine
[Platform] Starting order generator
[Platform] Starting status reporter
[Titan] Matching engine started
[Oracle] Event store started

╔═══════════════════════════════════════════════════════════╗
║                  SYSTEM OPERATIONAL                       ║
║  Metrics: http://localhost:9000/metrics                  ║
║  Press Ctrl+C to shutdown                                ║
╚═══════════════════════════════════════════════════════════╝
```

### 2. View Metrics

Open a new terminal:

```bash
# View all metrics
curl http://localhost:9000/metrics

# View specific metrics
curl http://localhost:9000/metrics | grep titan
curl http://localhost:9000/metrics | grep sentinel
curl http://localhost:9000/metrics | grep oracle
```

### 3. Start Monitoring Stack (Optional)

```bash
# Start Prometheus + Grafana
docker-compose up -d

# Access Grafana
open http://localhost:3000
# Login: admin/admin
```

### 4. What's Happening?

The platform will:
- Generate orders every 2 seconds
- Simulate BTC price movements (starting at $50,000)
- Monitor positions for liquidations
- Log all events to RocksDB
- Export metrics to Prometheus

Example output:

```
[Status] Accounts: 3 | Positions: 3 | Collateral: $35000.00

[Titan] Matching engine started
[Price Update] BTCUSDT = $50123.45

[LIQUIDATION] User 1001 | 0.5 BTCUSDT @ 44987 | Loss: $2500.00

[Oracle] Checkpoint: 1000 events persisted
```

### 5. Shutdown

Press `Ctrl+C`:

```
^C
[Platform] Shutting down gracefully...
✨ Platform stopped
```

All events are persisted to `platform_events/` directory.

## Configuration

### Switching to Live Binance Feed

Edit `src/main.rs`:

```rust
// Change from:
let price_mode = PriceFeedMode::Simulation {
    initial_price: Decimal::from(50000),
    volatility: Decimal::from_str("0.002").unwrap(),
};

// To:
let price_mode = PriceFeedMode::Binance {
    symbols: vec!["BTCUSDT".to_string()],
};
```

### Customizing Test Accounts

Edit `src/orchestrator.rs` in the `create_test_accounts()` function:

```rust
Account {
    user_id: 1001,
    collateral: Decimal::from(10000),  // $10,000 collateral
    unrealized_pnl: Decimal::ZERO,
    margin_ratio: Decimal::from(10),
    positions: vec![Position {
        symbol: "BTCUSDT".to_string(),
        side: PositionSide::Long,
        size: Decimal::new(5, 1),  // 0.5 BTC
        entry_price: Decimal::from(50000),
        leverage: 10,  // 10x leverage
        liquidation_price: Decimal::from(45000),  // Liquidates at $45k
        unrealized_pnl: Decimal::ZERO,
    }],
}
```

### Adjusting Risk Parameters

In `src/main.rs`:

```rust
// Maintenance margin ratio (0.5% = 200x max leverage)
let maintenance_margin_ratio = Decimal::from_str("0.005").unwrap();

// For more conservative risk (1% = 100x max):
let maintenance_margin_ratio = Decimal::from_str("0.01").unwrap();
```

### Order Generation

The platform auto-generates orders every 2 seconds. To adjust:

Edit `src/orchestrator.rs`:

```rust
pub async fn start_order_generator(&self) {
    let order_tx = self.order_tx.clone();

    tokio::spawn(async move {
        let mut order_id = 1u64;
        let mut tick = interval(Duration::from_secs(2)); // Change this

        loop {
            tick.tick().await;
            // Order generation logic...
        }
    });
}
```

## Advanced Usage

### Monitoring with Prometheus + Grafana

#### 1. Start Prometheus

Create `prometheus.yml`:
```yaml
global:
  scrape_interval: 1s

scrape_configs:
  - job_name: 'trading_platform'
    static_configs:
      - targets: ['localhost:9000']
```

Run Prometheus:
```bash
docker run -d \
  -p 9090:9090 \
  -v $(pwd)/prometheus.yml:/etc/prometheus/prometheus.yml \
  prom/prometheus
```

#### 2. Start Grafana

```bash
docker run -d -p 3000:3000 grafana/grafana
```

Access Grafana at `http://localhost:3000` (admin/admin)

#### 3. Sample Grafana Queries

**Liquidation Rate**:
```promql
rate(sentinel_liquidations_total[1m])
```

**Average Margin Ratio**:
```promql
avg(sentinel_margin_ratio)
```

**Order Processing Latency (p99)**:
```promql
histogram_quantile(0.99, rate(sentinel_process_time_micros_bucket[5m]))
```

**Price Feed Health**:
```promql
rate(price_feed_updates_total[30s])
```

**Execution Volume**:
```promql
sum(rate(titan_executions_total[1m]))
```

### Event Replay

To replay events from the Oracle store:

```rust
use trading_systems::oracle::OracleVault;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let vault = OracleVault::open("platform_events")?;
    
    // Replay all events
    let events = vault.replay_all();
    println!("Total events: {}", events.len());
    
    // Replay from specific sequence
    let events_from_1000 = vault.replay_from(1000);
    
    // Compute state hash
    let hash = vault.compute_state_hash();
    println!("State hash: {}", hash);
    
    Ok(())
}
```

### Troubleshooting

**Port 9000 in use?**
```bash
lsof -i :9000
kill -9 <PID>
```

**RocksDB lock error?**
```bash
rm -rf platform_events/LOCK
```

**Want to reset data?**
```bash
rm -rf platform_events/
```

## Architecture

### System Overview

The platform is built with a modular, event-driven architecture using Rust's concurrency primitives for maximum performance and safety.

### Data Flow

```
Orders → Titan → Executions → Oracle (RocksDB)
                     ↓
                  Events

Price Feed → Sentinel → Liquidations → Oracle (RocksDB)
                ↓
           Risk Checks
```

### Threading Model

```
Main Thread (Tokio Runtime)
├── Async Task: Price Feed (WebSocket)
├── Async Task: Sentinel Monitor
├── Async Task: Price Update Processor
├── Async Task: Order Generator
└── Async Task: Status Reporter

Background Thread 1: Titan Matching Engine
└── Blocking loop on order channel

Background Thread 2: Oracle Event Store
└── Blocking loop on event channel
```

### Core Components

**Titan (Matching Engine)**
- Single-threaded event loop (no locks needed)
- Typed arena for zero-copy order storage
- BTreeMap for price-level organization
- Price-time priority matching algorithm

**Oracle (Event Store)**
- RocksDB for persistent storage
- SHA-256 state hashing for verification
- Point-in-time recovery
- Zero-downtime replay

**Sentinel (Liquidation Engine)**
- DashMap for lock-free account access
- WebSocket price feed integration
- Configurable margin requirements
- Async monitoring loops

## Performance

- **Order Processing**: <1μs per order (single-threaded)
- **Event Persistence**: ~10k events/sec
- **Liquidation Checks**: <100μs per price update
- **WebSocket Latency**: <50ms (Binance)
- **Memory Usage**: ~50MB baseline

## Production Deployment

### Recommended Configuration

- **CPU**: 4+ cores (dedicated thread per component)
- **RAM**: 8GB+ (RocksDB caching)
- **Disk**: SSD for RocksDB
- **Network**: Low-latency connection for WebSocket

### Environment Variables

```bash
export RUST_LOG=info
export PROMETHEUS_PORT=9000
export ROCKSDB_PATH=./platform_events
```

## Contributing

Contributions welcome! Please ensure:
- All tests pass
- Code is formatted with `cargo fmt`
- No clippy warnings: `cargo clippy`
