use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PositionSide {
    Long,
    Short,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub order_id: u64,
    pub user_id: u64,
    pub symbol: String,
    pub side: Side,
    pub price: u64,
    pub quantity: u64,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Execution {
    pub buy_order_id: u64,
    pub sell_order_id: u64,
    pub price: u64,
    pub quantity: u64,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub symbol: String,
    pub side: PositionSide,
    pub size: Decimal,
    pub entry_price: Decimal,
    pub leverage: u8,
    pub liquidation_price: Decimal,
    pub unrealized_pnl: Decimal,
}

impl Position {
    pub fn calculate_pnl(&mut self, mark_price: Decimal) -> Decimal {
        let price_diff = match self.side {
            PositionSide::Long => mark_price - self.entry_price,
            PositionSide::Short => self.entry_price - mark_price,
        };
        
        self.unrealized_pnl = price_diff * self.size;
        self.unrealized_pnl
    }

    pub fn should_liquidate(&self, mark_price: Decimal) -> bool {
        match self.side {
            PositionSide::Long => mark_price <= self.liquidation_price,
            PositionSide::Short => mark_price >= self.liquidation_price,
        }
    }

    pub fn initial_margin(&self) -> Decimal {
        (self.entry_price * self.size) / Decimal::from(self.leverage)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub user_id: u64,
    pub collateral: Decimal,
    pub unrealized_pnl: Decimal,
    pub margin_ratio: Decimal,
    pub positions: Vec<Position>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceUpdate {
    pub symbol: String,
    pub mark_price: Decimal,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidationEvent {
    pub user_id: u64,
    pub symbol: String,
    pub side: PositionSide,
    pub size: Decimal,
    pub entry_price: Decimal,
    pub liquidation_price: Decimal,
    pub actual_price: Decimal,
    pub loss: Decimal,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SystemEvent {
    OrderPlaced(Order),
    OrderExecuted(Execution),
    PositionOpened {
        user_id: u64,
        position: Position,
        timestamp: u64,
    },
    PositionLiquidated(LiquidationEvent),
    PriceUpdate {
        symbol: String,
        price: Decimal,
        timestamp: u64,
    },
    AccountUpdated {
        user_id: u64,
        collateral: Decimal,
        margin_ratio: Decimal,
        timestamp: u64,
    },
}
