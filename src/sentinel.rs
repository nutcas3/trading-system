use crate::types::{Account, LiquidationEvent, Position, PositionSide, PriceUpdate, SystemEvent};
use dashmap::DashMap;
use futures_util::StreamExt;
use metrics::{counter, gauge, histogram};
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;
use tokio::time::{interval, Duration};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use serde::Deserialize;
use crossbeam::channel::Sender;

#[derive(Debug, Deserialize)]
struct BinanceTickerMessage {
    #[serde(rename = "e")]
    event_type: String,
    #[serde(rename = "s")]
    symbol: String,
    #[serde(rename = "c")]
    close_price: String,
    #[serde(rename = "E")]
    event_time: u64,
}

pub struct WebSocketPriceFeed {
    price_tx: broadcast::Sender<PriceUpdate>,
}

impl WebSocketPriceFeed {
    pub fn new(price_tx: broadcast::Sender<PriceUpdate>) -> Self {
        WebSocketPriceFeed { price_tx }
    }

    pub async fn connect_binance(
        &self,
        symbols: Vec<String>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let streams: Vec<String> = symbols
            .iter()
            .map(|s| format!("{}@ticker", s.to_lowercase()))
            .collect();

        let stream_param = streams.join("/");
        let url = format!("wss://stream.binance.com:9443/stream?streams={}", stream_param);

        println!("[WebSocket] Connecting to: {}", url);
        let (ws_stream, _) = connect_async(&url).await?;
        println!("[WebSocket] Connected successfully");

        let (_, mut read) = ws_stream.split();

        while let Some(message) = read.next().await {
            match message {
                Ok(Message::Text(text)) => {
                    if let Ok(wrapper) = serde_json::from_str::<serde_json::Value>(&text) {
                        if let Some(data) = wrapper.get("data") {
                            if let Ok(ticker) =
                                serde_json::from_value::<BinanceTickerMessage>(data.clone())
                            {
                                if let Ok(price) = Decimal::from_str(&ticker.close_price) {
                                    let update = PriceUpdate {
                                        symbol: ticker.symbol.clone(),
                                        mark_price: price,
                                        timestamp: ticker.event_time,
                                    };

                                    histogram!("price_feed.latency_ms")
                                        .record(self.calculate_latency(ticker.event_time) as f64);

                                    counter!("price_feed.updates_total", "symbol" => ticker.symbol.clone())
                                        .increment(1);

                                    let _ = self.price_tx.send(update);
                                }
                            }
                        }
                    }
                }
                Ok(Message::Ping(_)) => {
                    counter!("price_feed.pings_received").increment(1);
                }
                Err(e) => {
                    counter!("price_feed.errors_total").increment(1);
                    eprintln!("[WebSocket] Error: {}", e);
                }
                _ => {}
            }
        }

        Ok(())
    }

    pub async fn simulate_feed(&self, initial_price: Decimal, volatility: Decimal) {
        println!("[Simulator] Starting simulated price feed");
        let mut price = initial_price;
        let mut tick = interval(Duration::from_millis(100));

        loop {
            tick.tick().await;

            let change_pct = (rand::random::<f64>() - 0.5) * 2.0;
            let change = price * volatility * Decimal::from_f64(change_pct).unwrap();
            price = (price + change).max(Decimal::from(1000));

            let update = PriceUpdate {
                symbol: "BTCUSDT".to_string(),
                mark_price: price,
                timestamp: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64,
            };

            histogram!("price_feed.simulated_price").record(price.to_f64().unwrap_or(0.0));
            let _ = self.price_tx.send(update);
        }
    }

    fn calculate_latency(&self, event_time: u64) -> u64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        now.saturating_sub(event_time)
    }
}

pub struct SentinelEngine {
    accounts: Arc<DashMap<u64, Account>>,
    liquidation_tx: broadcast::Sender<LiquidationEvent>,
    event_tx: Sender<SystemEvent>,
    maintenance_margin_ratio: Decimal,
}

impl SentinelEngine {
    pub fn new(
        maintenance_margin_ratio: Decimal,
        event_tx: Sender<SystemEvent>,
    ) -> (Self, broadcast::Receiver<LiquidationEvent>) {
        let (liquidation_tx, liquidation_rx) = broadcast::channel(1000);

        let engine = SentinelEngine {
            accounts: Arc::new(DashMap::new()),
            liquidation_tx,
            event_tx,
            maintenance_margin_ratio,
        };

        (engine, liquidation_rx)
    }

    pub fn update_account(&self, account: Account) {
        gauge!("sentinel.accounts_total").set(self.accounts.len() as f64);
        self.accounts.insert(account.user_id, account);
    }

    pub async fn process_price_update(&self, update: PriceUpdate) {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let start = std::time::Instant::now();

        for mut entry in self.accounts.iter_mut() {
            let account = entry.value_mut();
            let mut liquidations = Vec::new();

            for position in &mut account.positions {
                if position.symbol == update.symbol {
                    position.calculate_pnl(update.mark_price);

                    if position.should_liquidate(update.mark_price) {
                        let loss = position.initial_margin() + position.unrealized_pnl;

                        liquidations.push(LiquidationEvent {
                            user_id: account.user_id,
                            symbol: position.symbol.clone(),
                            side: position.side,
                            size: position.size,
                            entry_price: position.entry_price,
                            liquidation_price: position.liquidation_price,
                            actual_price: update.mark_price,
                            loss: loss.abs(),
                            timestamp,
                        });
                    }
                }
            }

            if !liquidations.is_empty() {
                account
                    .positions
                    .retain(|p| !liquidations.iter().any(|liq| liq.symbol == p.symbol));

                for liq in &liquidations {
                    account.collateral -= liq.loss;

                    println!(
                        "[LIQUIDATION] User {} | {} {} @ {} | Loss: ${:.2}",
                        liq.user_id, liq.size, liq.symbol, liq.actual_price, liq.loss
                    );

                    counter!("sentinel.liquidations_total", "symbol" => liq.symbol.clone())
                        .increment(1);
                    histogram!("sentinel.liquidation_loss_usd")
                        .record(liq.loss.to_f64().unwrap_or(0.0));

                    let _ = self.liquidation_tx.send(liq.clone());
                    let _ = self
                        .event_tx
                        .send(SystemEvent::PositionLiquidated(liq.clone()));
                }

                self.calculate_margin_ratio(account);
            }
        }

        let elapsed = start.elapsed();
        histogram!("sentinel.process_time_micros").record(elapsed.as_micros() as f64);
    }

    fn calculate_margin_ratio(&self, account: &mut Account) {
        let total_initial_margin: Decimal =
            account.positions.iter().map(|p| p.initial_margin()).sum();

        let total_unrealized_pnl: Decimal =
            account.positions.iter().map(|p| p.unrealized_pnl).sum();

        account.unrealized_pnl = total_unrealized_pnl;
        let equity = account.collateral + total_unrealized_pnl;

        if total_initial_margin > Decimal::ZERO {
            account.margin_ratio = equity / total_initial_margin;
        } else {
            account.margin_ratio = Decimal::from(100);
        }

        gauge!("sentinel.margin_ratio", "user_id" => account.user_id.to_string())
            .set(account.margin_ratio.to_f64().unwrap_or(0.0));
    }

    pub async fn spawn_monitors(&self) {
        let accounts_clone = Arc::clone(&self.accounts);
        let mmr = self.maintenance_margin_ratio;

        tokio::spawn(async move {
            let mut tick = interval(Duration::from_millis(100));

            loop {
                tick.tick().await;
                let mut at_risk_count = 0;

                for entry in accounts_clone.iter() {
                    let account = entry.value();

                    if account.margin_ratio < mmr && !account.positions.is_empty() {
                        at_risk_count += 1;
                    }
                }

                gauge!("sentinel.accounts_at_risk").set(at_risk_count as f64);
            }
        });
    }

    pub fn get_accounts(&self) -> Arc<DashMap<u64, Account>> {
        Arc::clone(&self.accounts)
    }
}
