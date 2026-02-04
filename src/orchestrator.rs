use crate::oracle::OracleEngine;
use crate::sentinel::{SentinelEngine, WebSocketPriceFeed};
use crate::titan::TitanEngine;
use crate::types::{Account, Execution, Order, Position, PositionSide, PriceUpdate, SystemEvent};
use crossbeam::channel::{bounded, Receiver, Sender};
use metrics_exporter_prometheus::PrometheusBuilder;
use rust_decimal::Decimal;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::time::{interval, Duration};

pub struct TradingPlatform {
    order_tx: Sender<Order>,
    execution_rx: Receiver<Execution>,
    price_tx: broadcast::Sender<PriceUpdate>,
    event_tx: Sender<SystemEvent>,
    sentinel: Arc<SentinelEngine>,
}

impl TradingPlatform {
    pub fn new(maintenance_margin_ratio: Decimal) -> Result<Self, Box<dyn std::error::Error>> {
        let (order_tx, order_rx) = bounded::<Order>(10000);
        let (execution_tx, execution_rx) = bounded::<Execution>(10000);
        let (price_tx, _) = broadcast::channel::<PriceUpdate>(1000);
        let (event_tx, event_rx) = bounded::<SystemEvent>(10000);

        let event_tx_titan = event_tx.clone();
        std::thread::spawn(move || {
            let engine = TitanEngine::new(order_rx, execution_tx, event_tx_titan);
            engine.run();
        });

        std::thread::spawn(move || {
            let engine = OracleEngine::new("platform_events", event_rx)
                .expect("Failed to create Oracle engine");
            engine.run();
        });

        let (sentinel, _liquidation_rx) =
            SentinelEngine::new(maintenance_margin_ratio, event_tx.clone());
        let sentinel = Arc::new(sentinel);

        Ok(TradingPlatform {
            order_tx,
            execution_rx,
            price_tx,
            event_tx,
            sentinel,
        })
    }

    pub async fn start_price_feed(&self, mode: PriceFeedMode) {
        let price_tx = self.price_tx.clone();
        let feed = WebSocketPriceFeed::new(price_tx);

        tokio::spawn(async move {
            match mode {
                PriceFeedMode::Simulation { initial_price, volatility } => {
                    feed.simulate_feed(initial_price, volatility).await;
                }
                PriceFeedMode::Binance { symbols } => {
                    if let Err(e) = feed.connect_binance(symbols).await {
                        eprintln!("[PriceFeed] Binance connection failed: {}", e);
                    }
                }
            }
        });
    }

    pub async fn start_sentinel(&self) {
        self.sentinel.spawn_monitors().await;

        let sentinel = Arc::clone(&self.sentinel);
        let mut price_rx = self.price_tx.subscribe();

        tokio::spawn(async move {
            while let Ok(update) = price_rx.recv().await {
                sentinel.process_price_update(update).await;
            }
        });
    }

    pub fn submit_order(&self, order: Order) -> Result<(), String> {
        self.order_tx
            .send(order)
            .map_err(|e| format!("Failed to submit order: {}", e))
    }

    pub fn add_account(&self, account: Account) {
        self.sentinel.update_account(account);
    }

    pub async fn start_metrics_server(&self) -> Result<(), Box<dyn std::error::Error>> {
        PrometheusBuilder::new()
            .install()
            .expect("Failed to install Prometheus exporter");

        println!("[Metrics] Prometheus endpoint: http://localhost:9000/metrics");
        Ok(())
    }

    pub async fn start_order_generator(&self) {
        let order_tx = self.order_tx.clone();

        tokio::spawn(async move {
            let mut order_id = 1u64;
            let mut tick = interval(Duration::from_secs(2));

            loop {
                tick.tick().await;

                let order = Order {
                    order_id,
                    user_id: 1001,
                    symbol: "BTCUSD".to_string(),
                    side: if order_id % 2 == 0 {
                        crate::types::Side::Buy
                    } else {
                        crate::types::Side::Sell
                    },
                    price: 50000 + (order_id * 100),
                    quantity: 100,
                    timestamp: 0,
                };

                if order_tx.send(order).is_err() {
                    break;
                }

                order_id += 1;
            }
        });
    }

    pub async fn start_status_reporter(&self) {
        let sentinel = Arc::clone(&self.sentinel);

        tokio::spawn(async move {
            let mut tick = interval(Duration::from_secs(10));

            loop {
                tick.tick().await;

                let accounts = sentinel.get_accounts();
                let total_accounts = accounts.len();
                let mut total_positions = 0;
                let mut total_collateral = Decimal::ZERO;

                for entry in accounts.iter() {
                    let account = entry.value();
                    total_positions += account.positions.len();
                    total_collateral += account.collateral;
                }

                println!(
                    "\n[Status] Accounts: {} | Positions: {} | Collateral: ${:.2}",
                    total_accounts, total_positions, total_collateral
                );
            }
        });
    }
}

pub enum PriceFeedMode {
    Simulation {
        initial_price: Decimal,
        volatility: Decimal,
    },
    Binance {
        symbols: Vec<String>,
    },
}

pub fn create_test_accounts() -> Vec<Account> {
    vec![
        Account {
            user_id: 1001,
            collateral: Decimal::from(10000),
            unrealized_pnl: Decimal::ZERO,
            margin_ratio: Decimal::from(10),
            positions: vec![Position {
                symbol: "BTCUSDT".to_string(),
                side: PositionSide::Long,
                size: Decimal::new(5, 1),
                entry_price: Decimal::from(50000),
                leverage: 10,
                liquidation_price: Decimal::from(45000),
                unrealized_pnl: Decimal::ZERO,
            }],
        },
        Account {
            user_id: 1002,
            collateral: Decimal::from(5000),
            unrealized_pnl: Decimal::ZERO,
            margin_ratio: Decimal::from(20),
            positions: vec![Position {
                symbol: "BTCUSDT".to_string(),
                side: PositionSide::Long,
                size: Decimal::from(1),
                entry_price: Decimal::from(50000),
                leverage: 20,
                liquidation_price: Decimal::from(47500),
                unrealized_pnl: Decimal::ZERO,
            }],
        },
        Account {
            user_id: 1003,
            collateral: Decimal::from(20000),
            unrealized_pnl: Decimal::ZERO,
            margin_ratio: Decimal::from(5),
            positions: vec![Position {
                symbol: "BTCUSDT".to_string(),
                side: PositionSide::Short,
                size: Decimal::from(2),
                entry_price: Decimal::from(50000),
                leverage: 5,
                liquidation_price: Decimal::from(60000),
                unrealized_pnl: Decimal::ZERO,
            }],
        },
    ]
}
