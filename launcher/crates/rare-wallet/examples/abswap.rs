//! Swaps from an address balance (no keys for the trader, nothing signed or sent): asks the Trade
//! API for a quote and the router's transaction for `owner`, turns it into the wallet's PTB exactly
//! as the Trade tab does (`adapters::trade_input`), builds it on the node and dry-runs it.
//!
//! `cargo run -p rare-wallet --example abswap -- <owner> [amount in SUI]`

use std::sync::Arc;

use rare_market::net::{Call, LineStream, Net, NetError, Reply, MARKET, TRADE};
use rare_trade::swap::{MemKeys, QuoteParams, Swap};
use rare_wallet::sign::TxInput;

struct Live(ureq::Agent);

impl Net for Live {
    fn read(&self, key: &str) -> Result<Arc<str>, NetError> {
        let url = if let Some(p) = key.strip_prefix("rare:") {
            format!("{MARKET}{p}")
        } else if let Some(p) = key.strip_prefix("trade:") {
            format!("{TRADE}{p}")
        } else {
            return Err(NetError::NotAllowed(key.into()));
        };
        let r = self.0.get(&url).call().map_err(|e| NetError::Status(format!("{url}: {e}")))?;
        Ok(r.into_string().map_err(|e| NetError::Network(e.to_string()))?.into())
    }
    fn peek(&self, _key: &str) -> Option<Arc<str>> {
        None
    }
    fn call(&self, c: &Call) -> Result<Reply, NetError> {
        let mut r = self.0.request(c.method.as_str(), &c.url);
        for (k, v) in &c.headers {
            r = r.set(k, v);
        }
        let res = match &c.body {
            Some(b) => r.set("content-type", "application/json").send_string(b),
            None => r.call(),
        };
        match res {
            Ok(r) => Ok(Reply { status: r.status(), body: r.into_string().unwrap_or_default() }),
            Err(ureq::Error::Status(s, r)) => Ok(Reply { status: s, body: r.into_string().unwrap_or_default() }),
            Err(e) => Err(NetError::Network(e.to_string())),
        }
    }
    fn stream(&self, _c: &Call) -> Result<LineStream, NetError> {
        Err(NetError::NotAllowed("stream".into()))
    }
}

const USDC: &str = "0xdba34672e30cb065b1f93e3ab55318768fd6fef66c15942c9f7cb846e2f900e7::usdc::USDC";
const SUI: &str = "0x2::sui::SUI";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let owner = args.first().expect("owner").clone();
    let sui_in = (args.get(1).and_then(|s| s.parse::<f64>().ok()).unwrap_or(0.1) * 1e9) as u128;
    let rt = tokio::runtime::Runtime::new().unwrap();
    let sui = {
        let _g = rt.enter();
        rare_sui::Sui::current(&rare_sui::NetworkConfig::mainnet()).unwrap()
    };
    let swap = Swap::new(Arc::new(Live(ureq::AgentBuilder::new().user_agent("RareLauncher/0.1").build())), Arc::new(MemKeys::default()), rare_wallet::adapters::identity_maker());
    for (label, coin_in, coin_out, amount) in [("SUI -> USDC", SUI, USDC, sui_in), ("USDC -> SUI", USDC, SUI, 1_000_000u128)] {
        let q = match swap.quote(&QuoteParams { coin_in: coin_in.into(), coin_out: coin_out.into(), amount_in: amount, owner: Some(owner.clone()) }) {
            Ok(q) => q,
            Err(e) => {
                println!("{label}: quote failed: {e:?}");
                continue;
            }
        };
        let b = match swap.build_default(&q, &owner, false) {
            Ok(b) => b,
            Err(e) => {
                println!("{label}: router build failed: {e:?}");
                continue;
            }
        };
        let input = match rare_wallet::adapters::trade_input(&b.tx, None) {
            Ok(i) => i,
            Err(e) => {
                println!("{label}: the wallet cannot read the router's transaction: {e}");
                continue;
            }
        };
        let (tx, opts) = match input {
            TxInput::Ptb(t) => (t, Default::default()),
            TxInput::PtbGas(t, o) => (t, o),
            TxInput::Bytes(_) => {
                println!("{label}: bytes");
                continue;
            }
        };
        let withdrawals = tx.inputs.iter().filter(|i| matches!(i, rare_sui::ptb::Input::Withdrawal { .. })).count();
        match rt.block_on(rare_sui::tx::build(&sui, &tx, &opts)) {
            Ok(built) => {
                let res = built.data.gas_payment.objects.iter().filter(|o| o.digest().inner()[12..32] == [0xac; 20]).count();
                let sim = rt.block_on(sui.simulate(&built.data)).unwrap();
                let st = sim.transaction.as_ref().and_then(|t| t.effects.as_ref()).and_then(|e| e.status.as_ref());
                println!("{label}: {} out, router PTB has {withdrawals} address-balance withdrawal(s), {} gas object(s) ({res} reservation), dry run success {:?} {:?}", q.amount_out, built.data.gas_payment.objects.len(), st.and_then(|s| s.success), st.and_then(|s| s.error.as_ref()).and_then(|e| e.description.clone()));
            }
            Err(e) => println!("{label}: wallet build FAILED: {e}"),
        }
    }
}
