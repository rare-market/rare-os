//! Rare's notices: kinds, delivery rules, the SSE stream client, alerts, wake summary.
//! Ported from launcher/src/notify (logic only; no UI: the screens own Toast, Centre, AlertSheet,
//! BatteryNotice, OsUpdateNotice and the Settings section).
//!
//! How the parts fit:
//! - [`stream::NotifyStream`] runs the SSE connection on its own thread (resume, backoff, `recent`
//!   backfill) and sends [`stream::StreamEvent`]s to a sink, usually a channel.
//! - The UI thread owns a [`notifier::Notifier`] (the [`store::NotifyStore`] plus routing, the play
//!   queue and the wake summary) and drains the channel into `Notifier::handle`, calling
//!   `Notifier::tick` for toast and wake timers. What only the front end can do (sound, HUD toast,
//!   desktop notification) is a [`notifier::NotifyHost`] hook.
//! - Launcher-made notices (battery, downloads, emulators, OS updates, followed trades) are
//!   [`notifier::LocalNotice`]s handed to `Notifier::notify_local`.
//! - State persists through a [`storage::Storage`] (a [`storage::FileStorage`] under a directory the
//!   host gives) under the web launcher's keys.

pub mod actions;
pub mod alerts;
pub mod api;
pub mod battery;
pub mod desktop;
pub mod device;
pub mod downloads;
pub mod kinds;
pub mod latency;
pub mod logic;
#[cfg(any(test, feature = "mock"))]
pub mod mock;
pub mod notifier;
pub mod sse;
pub mod storage;
pub mod store;
pub mod stream;
pub mod types;

pub use logic::{class_of, delivery_for, Delivery, NoticeClass, Prefs, CLASSES};
pub use notifier::{LocalNotice, Notifier, NotifyHost};
pub use store::{NotifyStore, Status};
pub use stream::{NotifyStream, StreamEvent, StreamOptions};
pub use types::{Notice, NoticeAction, NoticeKind, PriceAlert};

pub use rare_core::Error;
pub type Result<T, E = Error> = std::result::Result<T, E>;
