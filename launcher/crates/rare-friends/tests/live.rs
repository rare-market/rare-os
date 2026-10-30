//! The friends client against the deployed API, with throwaway keys:
//!   LIVE=1 cargo test -p rare-friends --test live -- --ignored
//! Two fresh wallets link device keys (the wallet signature Rare Wallet would make), then the
//! client sends a request, accepts it, beats presence and chats, and cleans up.

use rare_friends::api::{FriendsApi, Remote, Result, SignPrompt, WalletSigner};
use rare_friends::sign::{mark_linked, now_ms, DeviceKey};
use rare_friends::storage::MemoryStorage;
use rare_friends::types::{Beat, GameRef, GraphOp, PresenceState};
use rare_market::net::{Call, LineStream, Net, NetError, Reply};
use std::io::Read;
use std::sync::Arc;

/// Uncached calls straight over ureq (the cache is not needed here).
struct Http(ureq::Agent);

impl Net for Http {
    fn read(&self, key: &str) -> Result<Arc<str>, NetError> {
        Err(NetError::NotAllowed(key.to_string()))
    }
    fn peek(&self, _: &str) -> Option<Arc<str>> {
        None
    }
    fn call(&self, c: &Call) -> Result<Reply, NetError> {
        let mut r = self.0.request(c.method.as_str(), &c.url).timeout(c.timeout);
        for (k, v) in &c.headers {
            r = r.set(k, v);
        }
        let res = match &c.body {
            Some(b) => r.send_string(b),
            None => r.call(),
        };
        let resp = match res {
            Ok(r) | Err(ureq::Error::Status(_, r)) => r,
            Err(e) => return Err(NetError::Network(e.to_string())),
        };
        let status = resp.status();
        let mut body = String::new();
        resp.into_reader().read_to_string(&mut body).map_err(|e| NetError::Network(e.to_string()))?;
        Ok(Reply { status, body })
    }
    fn stream(&self, c: &Call) -> Result<LineStream, NetError> {
        Err(NetError::NotAllowed(c.url.clone()))
    }
}

/// A software wallet: what Rare Wallet's review sheet signs after the player approves.
struct Wallet(DeviceKey);

impl WalletSigner for Wallet {
    fn sign_personal_message(&self, message: &[u8], _: &SignPrompt) -> Result<String> {
        Ok(self.0.sign(message))
    }
}

fn linked(net: &Arc<dyn Net>) -> (Remote, DeviceKey) {
    let (api, k, _) = linked_store(net);
    (api, k)
}

/// [`linked`] with the key store the device key lives in (for a `Friends` on it).
fn linked_store(net: &Arc<dyn Net>) -> (Remote, DeviceKey, Arc<MemoryStorage>) {
    let wallet = DeviceKey::generate("0x0");
    let api = Remote::new(net.clone(), Arc::new(Wallet(wallet.clone())));
    let store = Arc::new(MemoryStorage::new());
    let mut k = rare_friends::sign::device_key(&*store, &wallet.key);
    let until = api.link(&k, now_ms()).expect("link");
    mark_linked(&*store, &mut k, until);
    (api, k, store)
}

#[test]
#[ignore = "talks to rare.market; run with LIVE=1 and --ignored"]
fn requests_accepts_sees_presence_parties_and_chats() {
    if std::env::var_os("LIVE").is_none() {
        return;
    }
    let net: Arc<dyn Net> = Arc::new(Http(ureq::AgentBuilder::new().build()));
    let (api, ka) = linked(&net);
    let (_, kb) = linked(&net);
    let run = || -> Result<()> {
        api.graph(&ka, GraphOp::Request, &kb.address)?;
        assert!(api.overview(&kb)?.received.iter().any(|x| x.address == ka.address));
        api.graph(&kb, GraphOp::Accept, &ka.address)?;
        api.heartbeat(&ka, &Beat { state: PresenceState::Online, game: Some(GameRef::web("web:act", "ACT", "https://play.actuniverse.com/")), avatar: None })?;
        let seen = api.overview(&kb)?.friends.into_iter().find(|f| f.address == ka.address);
        assert_eq!(seen.and_then(|f| f.presence.game).map(|g| g.id).as_deref(), Some("web:act"));
        let p = api.create_party(&ka, None, std::slice::from_ref(&kb.address))?;
        api.join_party(&kb, &p.id)?;
        api.post_message(&kb, &p.id, "hi from the launcher")?;
        assert_eq!(api.messages(&ka, &p.id)?.last().map(|m| m.text.as_str()), Some("hi from the launcher"));
        let code = api.make_code(&ka)?;
        assert!(code.url.contains(&code.code));
        Ok(())
    };
    let r = run();
    // Clean up whatever happened.
    let _ = api.graph(&ka, GraphOp::Remove, &kb.address);
    for k in [&ka, &kb] {
        for p in api.overview(k).map(|o| o.parties).unwrap_or_default() {
            let _ = api.leave_party(k, &p.id);
        }
        let _ = api.heartbeat(k, &Beat { state: PresenceState::Offline, game: None, avatar: None });
        let _ = api.unlink(k);
    }
    r.expect("live friends flow");
}

/// Paging a long conversation through `Friends` (the screen's store) against rare.market:
///   LIVE=1 DM_SEED='ssh <box> sudo -u postgres psql -d market' cargo test -p rare-friends --test live -- --ignored dm_pages
/// Two throwaway friends; 320 messages are seeded straight into the table (the API's chat limit is
/// 20 a minute), 5 more go through the API. The store loads the newest page, then older pages until
/// the whole history is there, in order, once each. Everything is deleted at the end.
#[test]
#[ignore = "talks to rare.market and its database; run with LIVE=1, DM_SEED and --ignored"]
fn dm_pages_through_a_long_conversation() {
    if std::env::var_os("LIVE").is_none() {
        return;
    }
    let seed = std::env::var("DM_SEED").expect("DM_SEED: a command that runs SQL from stdin on the market database");
    let sql = |q: String| {
        use std::io::Write;
        let mut c = std::process::Command::new("sh").arg("-c").arg(&seed).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::null()).spawn().expect("seed command");
        c.stdin.take().unwrap().write_all(q.as_bytes()).unwrap();
        assert!(c.wait().unwrap().success(), "seed SQL failed");
    };
    let net: Arc<dyn Net> = Arc::new(Http(ureq::AgentBuilder::new().build()));
    let (api, ka) = linked(&net);
    let (_, kb, kb_store) = linked_store(&net);
    let (a, b) = (ka.address.clone(), kb.address.clone());
    let noise = (format!("0x{}", "e".repeat(64)), format!("0x{}", "f".repeat(64)));
    let run = || -> Result<()> {
        api.graph(&ka, GraphOp::Request, &b)?;
        api.graph(&kb, GraphOp::Accept, &a)?;
        // One statement: every row gets the same created_at (same instant), the pair's 320 messages
        // interleaved with 300 between two other addresses, which must never show up here.
        let (n1, n2) = (noise.0.clone(), noise.1.clone());
        // Ids cross a digit boundary (9999999 -> 10000000), where ordering by the text of the id
        // (SELECT id::text ... ORDER BY id) once put an older message last; the seeded rows are
        // deleted at the end, long before the sequence gets there.
        let mut rows: Vec<String> = Vec::new();
        let base: u64 = 9_999_800;
        for i in 0..320u64 {
            let (s, r) = if i % 2 == 0 { (&a, &b) } else { (&b, &a) };
            rows.push(format!("({}, '{s}','{r}','seeded {i}', now())", base + 2 * i));
            if i < 300 {
                rows.push(format!("({}, '{n1}','{n2}','noise {i}', now())", base + 2 * i + 1));
            }
        }
        sql(format!("INSERT INTO direct_messages (id, sender, recipient, body, created_at) VALUES {};", rows.join(",")));
        for i in 0..5 {
            api.send_dm(&ka, &b, &format!("live {i}"), None)?;
        }
        // B's store, as the Friends screen uses it.
        let f = rare_friends::Friends::new(Arc::new(api.clone()), kb_store.clone(), Arc::new(MemoryStorage::new()));
        f.sync_account(Some(&rare_friends::Account { address: b.clone(), can_sign: true }));
        f.load_dms(&a);
        let first = f.read(|s| s.dms.get(&a).map(|v| v.len()).unwrap_or(0));
        assert_eq!(first, 100, "the newest page");
        assert_eq!(f.read(|s| s.dms[&a].last().map(|m| m.text.clone())).as_deref(), Some("live 4"));
        let mut pages = 1;
        while !f.read(|s| s.dm_full.contains(&a)) {
            f.load_older_dms(&a);
            pages += 1;
            assert!(pages < 10, "paging never ended");
        }
        let (ids, texts) = f.read(|s| (s.dms[&a].iter().map(|m| m.id.parse::<u64>().unwrap()).collect::<Vec<_>>(), s.dms[&a].iter().map(|m| m.text.clone()).collect::<Vec<_>>()));
        assert_eq!(ids.len(), 325, "the whole history");
        assert!(f.read(|s| s.dms[&a].iter().all(|m| (m.from == a && m.to == b) || (m.from == b && m.to == a))), "only this pair's messages");
        // (created_at, id) order: the seeded rows (one instant, ids past 10^7) then the five sent
        // after them (lower ids from the live sequence); each message once.
        let want: Vec<String> = (0..320).map(|i| format!("seeded {i}")).chain((0..5).map(|i| format!("live {i}"))).collect();
        assert_eq!(texts, want, "oldest first, in order, each once");
        let mut uniq = ids.clone();
        uniq.sort();
        uniq.dedup();
        assert_eq!(uniq.len(), 325, "no message twice");
        assert_eq!(texts[0], "seeded 0");
        assert_eq!(texts[319], "seeded 319");
        assert_eq!(texts[324], "live 4");
        assert_eq!(pages, 4, "100 + 100 + 100 + 25");
        // At the start: nothing more is asked for.
        f.load_older_dms(&a);
        assert_eq!(f.read(|s| s.dms[&a].len()), 325);
        eprintln!("paged {} messages in {pages} pages", ids.len());
        Ok(())
    };
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(run));
    eprintln!("cleanup: remove {:?}", api.graph(&ka, GraphOp::Remove, &b).map(|_| ()));
    for k in [&ka, &kb] {
        eprintln!("cleanup: unlink {:?}", api.unlink(k));
    }
    eprintln!("cleanup: rows of {a} and {b}");
    sql(format!(
        "DELETE FROM direct_messages WHERE sender IN ('{a}','{b}','{}','{}') OR recipient IN ('{a}','{b}'); DELETE FROM friend_links WHERE owner IN ('{a}','{b}') OR peer IN ('{a}','{b}'); DELETE FROM friend_keys WHERE address IN ('{a}','{b}'); DELETE FROM friend_settings WHERE address IN ('{a}','{b}'); DELETE FROM played_with WHERE owner IN ('{a}','{b}') OR peer IN ('{a}','{b}');",
        noise.0, noise.1
    ));
    match r {
        Ok(r) => r.expect("live dm paging"),
        Err(p) => std::panic::resume_unwind(p),
    }
}
