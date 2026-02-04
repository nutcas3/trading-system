mod types;
mod titan;
mod oracle;
mod sentinel;
mod orchestrator;

use orchestrator::{create_test_accounts, PriceFeedMode, TradingPlatform};
use rust_decimal::Decimal;
use std::str::FromStr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("╔═══════════════════════════════════════════════════════════╗");
    println!("║     MODULAR TRADING PLATFORM - PRODUCTION GRADE          ║");
    println!("║  Titan (Matching) | Oracle (Events) | Sentinel (Risk)   ║");
    println!("╚═══════════════════════════════════════════════════════════╝\n");

    let maintenance_margin_ratio = Decimal::from_str("0.005").unwrap();
    let platform = TradingPlatform::new(maintenance_margin_ratio)?;

    platform.start_metrics_server().await?;
    println!();

    let test_accounts = create_test_accounts();
    for account in test_accounts {
        println!("[Platform] Adding account: User {}", account.user_id);
        platform.add_account(account);
    }
    println!();

    let price_mode = PriceFeedMode::Simulation {
        initial_price: Decimal::from(50000),
        volatility: Decimal::from_str("0.002").unwrap(),
    };

    println!("[Platform] Starting price feed (simulation mode)");
    platform.start_price_feed(price_mode).await;

    println!("[Platform] Starting Sentinel liquidation engine");
    platform.start_sentinel().await;

    println!("[Platform] Starting order generator");
    platform.start_order_generator().await;

    println!("[Platform] Starting status reporter");
    platform.start_status_reporter().await;

    println!("\n╔═══════════════════════════════════════════════════════════╗");
    println!("║                  SYSTEM OPERATIONAL                       ║");
    println!("║  Metrics: http://localhost:9000/metrics                  ║");
    println!("║  Press Ctrl+C to shutdown                                ║");
    println!("╚═══════════════════════════════════════════════════════════╝\n");

    tokio::signal::ctrl_c().await?;

    println!("\n[Platform] Shutting down gracefully...");
    println!("✨ Platform stopped");

    Ok(())
}
