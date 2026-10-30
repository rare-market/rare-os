//! Apps on the device: the shared model (screens/apps/state.rs) over the backend, so opening an
//! app, resuming it, closing it and installing go through rare-launch. Screens take `shared()`.

use crate::data::backend::Backend;
use crate::screens::apps::state::{GameRef, Host, StreamRef};
use crate::screens::apps::{apps, run_id, AppEntry, Apps};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

thread_local! {
    static MODEL: RefCell<Option<Rc<RefCell<Apps>>>> = const { RefCell::new(None) };
}

struct BackendHost(Arc<dyn Backend>);

impl Host for BackendHost {
    fn open(&mut self, app: &AppEntry, way: Option<usize>) -> Result<(), String> {
        self.0.app_open(&app.id, way)
    }
    fn resume_app(&mut self, app: &AppEntry) -> Result<(), String> {
        self.0.app_resume(&app.id)
    }
    fn close_app(&mut self, app: &AppEntry) -> Result<(), String> {
        self.0.quit(&run_id(&app.id))
    }
    fn install(&mut self, app: &AppEntry) -> Result<(), String> {
        self.0.app_install(&app.id)
    }
    fn toast(&mut self, msg: &str, error: bool) {
        super::bus::note(msg, error);
    }
    fn game_by_id(&self, _rid: &str) -> Option<GameRef> {
        None
    }
    fn stream_of(&self, _rid: &str) -> Option<StreamRef> {
        None
    }
    fn stream_title(&self, _rid: &str) -> Option<String> {
        None
    }
    fn resume_game(&mut self, g: &GameRef) -> Result<(), String> {
        self.0.launch(&g.id)
    }
    fn quit_game(&mut self, g: &GameRef) -> Result<(), String> {
        self.0.quit(&g.id)
    }
    fn resume_stream(&mut self, s: &StreamRef) -> Result<(), String> {
        Err(format!("stream {} is not wired", s.app))
    }
    fn quit_stream(&mut self, s: &StreamRef) -> Result<(), String> {
        Err(format!("stream {} is not wired", s.app))
    }
    fn native_resume(&mut self, rid: &str) -> Result<(), String> {
        self.0.launch(rid)
    }
    fn native_quit(&mut self, rid: &str) -> Result<(), String> {
        self.0.quit(rid)
    }
}

/// Builds the model on the UI thread: real statuses and what runs now.
pub fn init(backend: Arc<dyn Backend>) {
    let mut a = Apps::new(Box::new(BackendHost(backend.clone())));
    a.set_status(backend.app_statuses());
    a.running = backend.running();
    let _ = apps();
    MODEL.with(|m| *m.borrow_mut() = Some(a.shared()));
    BACKEND.with(|b| *b.borrow_mut() = Some(backend));
}

thread_local! {
    static BACKEND: RefCell<Option<Arc<dyn Backend>>> = const { RefCell::new(None) };
}

/// The model the Apps screens share (UI thread only).
pub fn shared() -> Option<Rc<RefCell<Apps>>> {
    MODEL.with(|m| m.borrow().clone())
}

/// The failure sheet's Try again for an app: the same app, the same way. False for anything else.
pub fn retry(rid: &str) -> bool {
    shared().is_some_and(|a| a.borrow_mut().reopen(rid))
}

/// Install progress and the running list reach the model.
pub fn on_event(ev: &rare_core::Event) {
    let Some(a) = shared() else { return };
    match ev {
        rare_core::Event::AppInstall(i) => {
            crate::screens::stream::device::install_event(i);
            a.borrow_mut().install_event(&i.id, i.progress, i.done, i.error.clone());
            if i.done && i.error.is_none() {
                if let Some(b) = BACKEND.with(|b| b.borrow().clone()) {
                    a.borrow_mut().set_status(b.app_statuses());
                }
            }
        }
        rare_core::Event::GameState(r) => {
            let mut m = a.borrow_mut();
            if m.running != r.running {
                m.running = r.running.clone();
                m.touch();
            }
        }
        _ => {}
    }
}
