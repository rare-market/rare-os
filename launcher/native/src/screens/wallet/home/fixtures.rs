//! Wallet home fixtures: the demo account the web shots watch (`?watch=`), a priced portfolio, and
//! a day of activity (received, sent yesterday, a failed call).

use crate::screens::wallet::sign::fixtures::OTHER;
pub use crate::screens::wallet::sign::fixtures::ME;
use crate::screens::wallet::types::*;

/// 2026-10-27 15:07 UTC.
pub const NOW: i64 = 1_793_113_620_000;

pub fn account() -> RareAccount {
    RareAccount::new(ME, "Main", "passkey")
}

pub fn portfolio() -> Portfolio {
    Portfolio {
        total_usd: Some(1284.07),
        change_24h_usd: Some(12.40),
        priced: true,
        tokens: vec![
            PortfolioToken {
                coin_type: "0x2::sui::SUI".into(),
                symbol: "SUI".into(),
                name: "Sui".into(),
                decimals: 9,
                icon_url: None,
                raw: 1_204_500_000_000,
                amount: 1204.5,
                price_usd: Some(0.906),
                value_usd: Some(1091.28),
                change_24h: Some(1.56),
                verified: true,
            },
            PortfolioToken {
                coin_type: "0xdba34672e30cb065b1f93e3ab55318768fd6fef66c15942c9f7cb846e2f900e7::usdc::USDC".into(),
                symbol: "USDC".into(),
                name: "USD Coin".into(),
                decimals: 6,
                icon_url: Some("https://circle.com/usdc.png".into()),
                raw: 192_790_000,
                amount: 192.79,
                price_usd: Some(1.0),
                value_usd: Some(192.79),
                change_24h: Some(0.01),
                verified: true,
            },
        ],
    }
}

pub fn activity() -> Vec<ActivityItem> {
    vec![
        ActivityItem {
            digest: "9xQm3ZfLr1vT8kPbN2cW4yHs7DgJ5eAa".into(),
            timestamp: NOW - 55 * 60_000,
            kind: ActivityKind::Receive,
            changes: vec![],
            summary: "25 SUI".into(),
            counterparty: Some(OTHER.into()),
            failed: false,
            count: None,
        },
        ActivityItem {
            digest: "4hTq8WnB6sLm2ZpXc9VdR3yKf7GjE1uA".into(),
            timestamp: NOW - 26 * 3_600_000,
            kind: ActivityKind::Send,
            changes: vec![],
            summary: "0.1 SUI".into(),
            counterparty: Some(OTHER.into()),
            failed: false,
            count: None,
        },
        ActivityItem {
            digest: "7aBc2DeF3gHi4JkL5mNo6PqR7sTu8VwX".into(),
            timestamp: NOW - 27 * 3_600_000,
            kind: ActivityKind::Swap,
            changes: vec![],
            summary: "215.195 SUI → 502.6154 MAGMA".into(),
            counterparty: Some(OTHER.into()),
            failed: false,
            count: None,
        },
        ActivityItem {
            digest: "2zYx3WvU4tSr5QpO6nMl7KjI8hGf9EdC".into(),
            timestamp: NOW - 28 * 3_600_000,
            kind: ActivityKind::Other,
            changes: vec![],
            summary: "Contract call".into(),
            counterparty: None,
            failed: true,
            count: None,
        },
    ]
}
