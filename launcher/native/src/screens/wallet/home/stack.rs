//! The wallet tab's page stack (launcher/src/wallet/ui/store.ts `useWalletNav`, WalletScreen.tsx):
//! like the Marketplace's, pages underneath stay alive, B goes back with focus restored, at most
//! eight pages deep. With no accounts home is the welcome page.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HomeTab {
    Coins,
    Collectibles,
    Activity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasskeyMode {
    Create,
    Signin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportWhat {
    Phrase,
    Key,
    /// the 12 words of a PIN account, to sign with a PIN on this SuiPlay again
    Restore,
}

/// `WalletView`.
#[derive(Clone, Debug, PartialEq)]
pub enum WalletView {
    Home { tab: Option<HomeTab> },
    Coin { coin_type: String },
    Nft { id: String },
    Activity { digest: String },
    Send { coin_type: Option<String>, to: Option<String> },
    SendNfts { ids: Vec<String>, to: Option<String> },
    Receive,
    Accounts,
    Settings,
    /// add account menu: passkey, new phrase, import, Ledger, Keystone, watch
    Add,
    Passkey { mode: PasskeyMode },
    CreatePhrase,
    Import { what: ImportWhat },
    Watch,
    Ledger,
    Keystone,
    /// move from your old SuiPlay wallet
    Playtron,
    /// Create account with PIN (device key, PIN, phone and/or security key)
    PinAccount,
    /// move everything from another account here
    MoveAll { to: String },
}

/// One page in the stack; `focus` is where focus was when another page went over it.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub key: u64,
    pub view: WalletView,
    pub focus: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stack {
    pub frames: Vec<Frame>,
    seq: u64,
}

impl Default for Stack {
    fn default() -> Self {
        Stack { frames: vec![Frame { key: 1, view: WalletView::Home { tab: None }, focus: None }], seq: 1 }
    }
}

impl Stack {
    fn next(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }

    pub fn top(&self) -> &Frame {
        self.frames.last().expect("home is always there")
    }

    /// Opens a page over the current one; `focused` is what had focus (restored on the way back).
    pub fn push(&mut self, view: WalletView, focused: Option<String>) {
        if let Some(t) = self.frames.last_mut() {
            t.focus = focused;
        }
        let key = self.next();
        self.frames.push(Frame { key, view, focus: None });
        while self.frames.len() > 8 {
            self.frames.remove(1);
        }
    }

    /// Replaces the top page (finishing a flow lands on its result).
    pub fn replace(&mut self, view: WalletView) {
        let key = self.next();
        if let Some(t) = self.frames.last_mut() {
            *t = Frame { key, view, focus: None };
        }
    }

    /// B: back one page, false on home. The page we land on brings its `focus` back.
    pub fn pop(&mut self) -> bool {
        if self.frames.len() <= 1 {
            return false;
        }
        self.frames.pop();
        true
    }

    /// Back to home, with `views` opened over it.
    pub fn reset(&mut self, views: Vec<WalletView>) {
        let home = Frame { key: self.next(), view: WalletView::Home { tab: None }, focus: None };
        self.frames = vec![home];
        for v in views {
            let key = self.next();
            self.frames.push(Frame { key, view: v, focus: None });
        }
    }

    /// `?wv=send,receive,coin/<type>`: the dev and screenshot deep link.
    pub fn from_query(wv: &str) -> Vec<WalletView> {
        wv.split(',')
            .filter_map(|p| {
                let (kind, arg) = p.split_once('/').map(|(k, a)| (k, Some(a))).unwrap_or((p, None));
                Some(match kind {
                    "send" => WalletView::Send { coin_type: None, to: None },
                    "receive" => WalletView::Receive,
                    "accounts" => WalletView::Accounts,
                    "settings" => WalletView::Settings,
                    "add" => WalletView::Add,
                    "create-phrase" => WalletView::CreatePhrase,
                    "watch" => WalletView::Watch,
                    "ledger" => WalletView::Ledger,
                    "keystone" => WalletView::Keystone,
                    "playtron" => WalletView::Playtron,
                    "pin" => WalletView::PinAccount,
                    "move-all" => WalletView::MoveAll { to: arg.unwrap_or("").to_string() },
                    "passkey" => WalletView::Passkey { mode: if arg == Some("signin") { PasskeyMode::Signin } else { PasskeyMode::Create } },
                    "import" => WalletView::Import { what: match arg {
                        Some("key") => ImportWhat::Key,
                        Some("restore") => ImportWhat::Restore,
                        _ => ImportWhat::Phrase,
                    } },
                    "coin" => WalletView::Coin { coin_type: percent_decode(arg.filter(|a| !a.is_empty())?) },
                    "nft" => WalletView::Nft { id: arg.filter(|a| !a.is_empty())?.to_string() },
                    "activity" => WalletView::Activity { digest: arg.filter(|a| !a.is_empty())?.to_string() },
                    _ => return None,
                })
            })
            .collect()
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stack() {
        let mut s = Stack::default();
        assert!(!s.pop());
        s.push(WalletView::Receive, Some("receive-btn".into()));
        assert_eq!(s.top().view, WalletView::Receive);
        assert_eq!(s.frames[0].focus.as_deref(), Some("receive-btn"));
        s.replace(WalletView::Accounts);
        assert_eq!(s.frames.len(), 2);
        assert!(s.pop());
        assert_eq!(s.top().focus.as_deref(), Some("receive-btn"));
        for _ in 0..10 {
            s.push(WalletView::Settings, None);
        }
        assert_eq!(s.frames.len(), 8);
        assert_eq!(s.frames[0].view, WalletView::Home { tab: None });
    }

    #[test]
    fn deep_link() {
        let v = Stack::from_query("send,coin/0x2%3A%3Asui%3A%3ASUI,passkey/signin,import/key,nft/,bogus");
        assert_eq!(
            v,
            vec![
                WalletView::Send { coin_type: None, to: None },
                WalletView::Coin { coin_type: "0x2::sui::SUI".into() },
                WalletView::Passkey { mode: PasskeyMode::Signin },
                WalletView::Import { what: ImportWhat::Key },
            ]
        );
    }
}
