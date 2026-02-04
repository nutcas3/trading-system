use crate::types::{Execution, Order, Side, SystemEvent};
use crossbeam::channel::{Receiver, Sender};
use metrics::{counter, histogram};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use typed_arena::Arena;

struct PriceLevel<'a> {
    orders: Vec<&'a Order>,
}

impl<'a> PriceLevel<'a> {
    fn new() -> Self {
        PriceLevel { orders: Vec::new() }
    }

    fn add_order(&mut self, order: &'a Order) {
        self.orders.push(order);
    }

    fn total_quantity(&self) -> u64 {
        self.orders.iter().map(|o| o.quantity).sum()
    }
}

pub struct OrderBook<'a> {
    symbol: String,
    bids: BTreeMap<u64, PriceLevel<'a>>,
    asks: BTreeMap<u64, PriceLevel<'a>>,
    order_arena: &'a Arena<Order>,
    executions: Vec<Execution>,
    sequence: AtomicU64,
}

impl<'a> OrderBook<'a> {
    pub fn new(symbol: String, arena: &'a Arena<Order>) -> Self {
        OrderBook {
            symbol,
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            order_arena: arena,
            executions: Vec::new(),
            sequence: AtomicU64::new(0),
        }
    }

    pub fn process_order(&mut self, order: Order) -> Vec<Execution> {
        let seq = self.sequence.fetch_add(1, Ordering::SeqCst);
        let mut order = order;
        order.timestamp = seq;

        let order_ref = self.order_arena.alloc(order);
        
        self.executions.clear();

        match order_ref.side {
            Side::Buy => self.match_buy_order(order_ref),
            Side::Sell => self.match_sell_order(order_ref),
        }

        counter!("titan.orders_processed").increment(1);
        histogram!("titan.executions_per_order").record(self.executions.len() as f64);

        self.executions.clone()
    }

    fn match_buy_order(&mut self, order: &'a Order) {
        let mut remaining_qty = order.quantity;

        let ask_prices: Vec<u64> = self.asks.keys().copied().collect();
        
        for ask_price in ask_prices {
            if ask_price > order.price {
                break;
            }

            if let Some(level) = self.asks.get_mut(&ask_price) {
                let mut i = 0;
                while i < level.orders.len() && remaining_qty > 0 {
                    let sell_order = level.orders[i];
                    let exec_qty = remaining_qty.min(sell_order.quantity);

                    self.executions.push(Execution {
                        buy_order_id: order.order_id,
                        sell_order_id: sell_order.order_id,
                        price: ask_price,
                        quantity: exec_qty,
                        timestamp: order.timestamp,
                    });

                    remaining_qty -= exec_qty;

                    if exec_qty == sell_order.quantity {
                        level.orders.remove(i);
                    } else {
                        i += 1;
                    }
                }

                if level.orders.is_empty() {
                    self.asks.remove(&ask_price);
                }
            }

            if remaining_qty == 0 {
                break;
            }
        }

        if remaining_qty > 0 {
            let mut partial_order = order.clone();
            partial_order.quantity = remaining_qty;
            let partial_ref = self.order_arena.alloc(partial_order);
            
            self.bids.entry(order.price)
                .or_insert_with(PriceLevel::new)
                .add_order(partial_ref);
        }
    }

    fn match_sell_order(&mut self, order: &'a Order) {
        let mut remaining_qty = order.quantity;

        let bid_prices: Vec<u64> = self.bids.keys().rev().copied().collect();
        
        for bid_price in bid_prices {
            if bid_price < order.price {
                break;
            }

            if let Some(level) = self.bids.get_mut(&bid_price) {
                let mut i = 0;
                while i < level.orders.len() && remaining_qty > 0 {
                    let buy_order = level.orders[i];
                    let exec_qty = remaining_qty.min(buy_order.quantity);

                    self.executions.push(Execution {
                        buy_order_id: buy_order.order_id,
                        sell_order_id: order.order_id,
                        price: bid_price,
                        quantity: exec_qty,
                        timestamp: order.timestamp,
                    });

                    remaining_qty -= exec_qty;

                    if exec_qty == buy_order.quantity {
                        level.orders.remove(i);
                    } else {
                        i += 1;
                    }
                }

                if level.orders.is_empty() {
                    self.bids.remove(&bid_price);
                }
            }

            if remaining_qty == 0 {
                break;
            }
        }

        if remaining_qty > 0 {
            let mut partial_order = order.clone();
            partial_order.quantity = remaining_qty;
            let partial_ref = self.order_arena.alloc(partial_order);
            
            self.asks.entry(order.price)
                .or_insert_with(PriceLevel::new)
                .add_order(partial_ref);
        }
    }

    pub fn best_bid(&self) -> Option<u64> {
        self.bids.keys().next_back().copied()
    }

    pub fn best_ask(&self) -> Option<u64> {
        self.asks.keys().next().copied()
    }

    pub fn depth(&self, levels: usize) -> (Vec<(u64, u64)>, Vec<(u64, u64)>) {
        let bids: Vec<(u64, u64)> = self.bids.iter()
            .rev()
            .take(levels)
            .map(|(price, level)| (*price, level.total_quantity()))
            .collect();

        let asks: Vec<(u64, u64)> = self.asks.iter()
            .take(levels)
            .map(|(price, level)| (*price, level.total_quantity()))
            .collect();

        (bids, asks)
    }
}

pub struct TitanEngine {
    order_rx: Receiver<Order>,
    execution_tx: Sender<Execution>,
    event_tx: Sender<SystemEvent>,
}

impl TitanEngine {
    pub fn new(
        order_rx: Receiver<Order>,
        execution_tx: Sender<Execution>,
        event_tx: Sender<SystemEvent>,
    ) -> Self {
        TitanEngine {
            order_rx,
            execution_tx,
            event_tx,
        }
    }

    pub fn run(&self) {
        let arena = Arena::new();
        let mut book = OrderBook::new("BTCUSD".to_string(), &arena);

        println!("[Titan] Matching engine started");

        while let Ok(order) = self.order_rx.recv() {
            let _ = self.event_tx.send(SystemEvent::OrderPlaced(order.clone()));

            let execs = book.process_order(order);
            
            if !execs.is_empty() {
                counter!("titan.executions_total").increment(execs.len() as u64);
                
                for exec in &execs {
                    histogram!("titan.execution_price").record(exec.price as f64);
                    histogram!("titan.execution_quantity").record(exec.quantity as f64);
                    
                    let _ = self.event_tx.send(SystemEvent::OrderExecuted(exec.clone()));
                }
                
                let _ = self.execution_tx.send(execs[0].clone());
            }

            if let (Some(bid), Some(ask)) = (book.best_bid(), book.best_ask()) {
                let spread = ask.saturating_sub(bid);
                histogram!("titan.spread").record(spread as f64);
            }
        }
    }
}
