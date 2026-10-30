//! SuiNS over the fullnode's gRPC NameService (LookupName / ReverseLookupName), cached for five
//! minutes. Ports `src/wallet/data/names.ts` (failures are not cached).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::grpc::ChainRead;
use crate::ids::canon_addr;
use crate::Result;

const TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Default)]
pub struct NameCache {
    forward: Mutex<HashMap<String, (Instant, Option<String>)>>,
    reverse: Mutex<HashMap<String, (Instant, Option<String>)>>,
}

fn hit(m: &Mutex<HashMap<String, (Instant, Option<String>)>>, key: &str) -> Option<Option<String>> {
    let m = m.lock().unwrap_or_else(|e| e.into_inner());
    m.get(key).filter(|(at, _)| at.elapsed() < TTL).map(|(_, v)| v.clone())
}

fn keep(m: &Mutex<HashMap<String, (Instant, Option<String>)>>, key: String, v: Option<String>) {
    m.lock().unwrap_or_else(|e| e.into_inner()).insert(key, (Instant::now(), v));
}

impl NameCache {
    /// `alice.sui` -> address, or None when the name has no target. Errors when the node can't be
    /// reached.
    pub async fn name_to_address(&self, chain: &dyn ChainRead, name: &str) -> Result<Option<String>> {
        let key = name.trim().to_lowercase();
        if let Some(v) = hit(&self.forward, &key) {
            return Ok(v);
        }
        let v = chain.resolve_name(&key).await?.map(|a| canon_addr(&a));
        keep(&self.forward, key, v.clone());
        Ok(v)
    }

    /// Reverse lookup: the address's default SuiNS name, or None.
    pub async fn name_of(&self, chain: &dyn ChainRead, address: &str) -> Result<Option<String>> {
        let key = canon_addr(address);
        if let Some(v) = hit(&self.reverse, &key) {
            return Ok(v);
        }
        let v = chain.default_name(&key).await?;
        keep(&self.reverse, key, v.clone());
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BoxFut, Error};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct C {
        calls: AtomicUsize,
        fail: bool,
    }
    impl ChainRead for C {
        fn default_name(&self, _a: &str) -> BoxFut<'_, Option<String>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let fail = self.fail;
            Box::pin(async move { if fail { Err(Error::invalid("down")) } else { Ok(Some("bob.sui".into())) } })
        }
        fn resolve_name(&self, _n: &str) -> BoxFut<'_, Option<String>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Ok(Some("0xB2".into())) })
        }
    }

    // data.test.ts "names"
    #[tokio::test]
    async fn reverse_lookup_returns_the_default_name_once() {
        let bob = format!("0x{}", "b2".repeat(32));
        let c = C::default();
        let n = NameCache::default();
        assert_eq!(n.name_of(&c, &bob).await.unwrap().as_deref(), Some("bob.sui"));
        assert_eq!(n.name_of(&c, &bob).await.unwrap().as_deref(), Some("bob.sui"));
        assert_eq!(c.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn failures_are_not_cached_and_names_normalize() {
        let n = NameCache::default();
        let bad = C { fail: true, ..Default::default() };
        assert!(n.name_of(&bad, "0x1").await.is_err());
        assert!(n.name_of(&bad, "0x1").await.is_err());
        assert_eq!(bad.calls.load(Ordering::SeqCst), 2);
        let c = C::default();
        assert_eq!(n.name_to_address(&c, " Alice.SUI ").await.unwrap(), Some(canon_addr("0xb2")));
        n.name_to_address(&c, "alice.sui").await.unwrap();
        assert_eq!(c.calls.load(Ordering::SeqCst), 1);
    }
}
