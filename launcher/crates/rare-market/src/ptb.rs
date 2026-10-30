//! The unresolved programmable transaction every builder produces. It lives in `rare-sui` (which
//! builds it against the chain with `rare_sui::tx::build`), so every crate shares one type; see
//! `rare_sui::ptb` for the layout and its parity with `@mysten/sui`'s `tx.getData()`.

pub use rare_sui::ptb::*;
