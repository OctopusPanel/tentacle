mod docker;
mod communication;
mod metrics;

use std::time::Duration;

#[tokio::main]
async fn main() {
    println!("🐙 Tentacle Daemon booting up...");
    println!("🔌 Connecting to Octopus Panel...");
    println!("🐳 Initializing Docker connection...");
    
    // Dummy Loop für kontinuierliches Streamen der Metriken
    loop {
        println!("📊 Fetching Docker stats and streaming to Panel...");
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}
