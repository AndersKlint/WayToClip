use std::{
    cell::RefCell,
    rc::{Rc, Weak},
};

use gtk::gio::prelude::*;
use gtk::glib;

use crate::{
    common::constants::PrefsFields,
    settings::{SettingsManager, SettingsSnapshot},
};

pub const CLEAR_OFF: i64 = -1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClearDecision {
    Disabled,
    Arm { deadline: i64 },
    ClearNow { deadline: i64 },
}

pub fn plan_start(snapshot: &SettingsSnapshot, now: i64) -> ClearDecision {
    if !snapshot.should_clear_history_on_interval {
        return ClearDecision::Disabled;
    }
    let next_deadline = now + i64::from(snapshot.clear_history_interval) * 60;
    if snapshot.next_history_clear == CLEAR_OFF {
        ClearDecision::Arm {
            deadline: next_deadline,
        }
    } else if snapshot.next_history_clear <= now {
        ClearDecision::ClearNow {
            deadline: next_deadline,
        }
    } else {
        ClearDecision::Arm {
            deadline: snapshot.next_history_clear,
        }
    }
}

pub fn plan_schedule(snapshot: &SettingsSnapshot, now: i64) -> ClearDecision {
    if !snapshot.should_clear_history_on_interval {
        return ClearDecision::Disabled;
    }
    ClearDecision::Arm {
        deadline: now + i64::from(snapshot.clear_history_interval) * 60,
    }
}

pub fn time_left(snapshot: &SettingsSnapshot, now: i64) -> i64 {
    if !snapshot.should_clear_history_on_interval {
        return CLEAR_OFF;
    }
    (snapshot.next_history_clear - now).max(0)
}

pub fn now_seconds() -> i64 {
    glib::real_time() / 1_000_000
}

pub struct HistoryClearScheduler {
    settings: Rc<SettingsManager>,
    on_clear: RefCell<Option<Rc<dyn Fn()>>>,
    on_tick: RefCell<Option<Rc<dyn Fn(i64)>>>,
    handlers: RefCell<Vec<glib::SignalHandlerId>>,
    timeout: RefCell<Option<glib::SourceId>>,
    ticker: RefCell<Option<glib::SourceId>>,
}

impl HistoryClearScheduler {
    pub fn new(settings: Rc<SettingsManager>) -> Rc<Self> {
        Rc::new(Self {
            settings,
            on_clear: RefCell::new(None),
            on_tick: RefCell::new(None),
            handlers: RefCell::new(Vec::new()),
            timeout: RefCell::new(None),
            ticker: RefCell::new(None),
        })
    }

    pub fn start(self: &Rc<Self>, on_clear: impl Fn() + 'static) {
        self.disconnect_settings();
        *self.on_clear.borrow_mut() = Some(Rc::new(on_clear));

        let interval = Rc::downgrade(self);
        let interval_id = self.settings.gio().connect_changed(
            Some(PrefsFields::CLEAR_HISTORY_INTERVAL),
            move |_, _| {
                if let Some(scheduler) = interval.upgrade() {
                    scheduler.schedule();
                }
            },
        );
        let toggle = Rc::downgrade(self);
        let toggle_id = self.settings.gio().connect_changed(
            Some(PrefsFields::CLEAR_HISTORY_ON_INTERVAL),
            move |_, _| {
                if let Some(scheduler) = toggle.upgrade() {
                    scheduler.on_toggle_changed();
                }
            },
        );
        self.handlers.borrow_mut().extend([interval_id, toggle_id]);
        self.restart();
    }

    pub fn set_on_tick(&self, on_tick: impl Fn(i64) + 'static) {
        *self.on_tick.borrow_mut() = Some(Rc::new(on_tick));
    }

    pub fn schedule(self: &Rc<Self>) {
        self.clear_timers();
        let snapshot = self.settings.snapshot();
        let now = now_seconds();
        match plan_schedule(&snapshot, now) {
            ClearDecision::Disabled => {
                self.persist(CLEAR_OFF);
                self.emit_tick(CLEAR_OFF);
            }
            ClearDecision::Arm { deadline } => self.arm(deadline, now),
            ClearDecision::ClearNow { deadline } => {
                self.persist(deadline);
                self.fire_clear();
                self.arm(deadline, now);
            }
        }
    }

    pub fn reset(&self) {
        self.clear_timers();
        self.persist(CLEAR_OFF);
        self.emit_tick(CLEAR_OFF);
    }

    pub fn time_left(&self) -> i64 {
        time_left(&self.settings.snapshot(), now_seconds())
    }

    pub fn stop(&self) {
        self.clear_timers();
        self.disconnect_settings();
    }

    fn restart(self: &Rc<Self>) {
        self.clear_timers();
        let snapshot = self.settings.snapshot();
        let now = now_seconds();
        match plan_start(&snapshot, now) {
            ClearDecision::Disabled => self.emit_tick(CLEAR_OFF),
            ClearDecision::Arm { deadline } => self.arm(deadline, now),
            ClearDecision::ClearNow { deadline } => {
                self.persist(deadline);
                self.fire_clear();
                self.arm(deadline, now);
            }
        }
    }

    fn on_toggle_changed(self: &Rc<Self>) {
        if self.settings.snapshot().should_clear_history_on_interval {
            self.restart();
        } else {
            self.reset();
        }
    }

    fn arm(self: &Rc<Self>, deadline: i64, now: i64) {
        self.clear_timers();
        self.persist(deadline);
        let seconds_left = deadline - now;
        self.emit_tick(seconds_left);

        let ticker_scheduler = Rc::downgrade(self);
        let ticker = glib::timeout_add_seconds_local(1, move || {
            let Some(scheduler) = ticker_scheduler.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let left = scheduler.time_left();
            scheduler.emit_tick(left);
            if left <= 0 {
                scheduler.ticker.borrow_mut().take();
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Continue
            }
        });
        *self.ticker.borrow_mut() = Some(ticker);

        let timeout_scheduler: Weak<Self> = Rc::downgrade(self);
        let timeout = glib::timeout_add_seconds_local(seconds_left.max(1) as u32, move || {
            if let Some(scheduler) = timeout_scheduler.upgrade() {
                scheduler.timeout.borrow_mut().take();
                scheduler.fire_clear();
                scheduler.schedule();
            }
            glib::ControlFlow::Break
        });
        *self.timeout.borrow_mut() = Some(timeout);
    }

    fn fire_clear(&self) {
        let callback = self.on_clear.borrow().clone();
        if let Some(callback) = callback {
            callback();
        }
    }

    fn emit_tick(&self, seconds_left: i64) {
        let callback = self.on_tick.borrow().clone();
        if let Some(callback) = callback {
            callback(seconds_left);
        }
    }

    fn persist(&self, timestamp: i64) {
        let _ = self.settings.set_next_history_clear(timestamp);
    }

    fn clear_timers(&self) {
        if let Some(timeout) = self.timeout.borrow_mut().take() {
            timeout.remove();
        }
        if let Some(ticker) = self.ticker.borrow_mut().take() {
            ticker.remove();
        }
    }

    fn disconnect_settings(&self) {
        let settings = self.settings.gio();
        for handler in self.handlers.borrow_mut().drain(..) {
            settings.disconnect(handler);
        }
    }
}

impl Drop for HistoryClearScheduler {
    fn drop(&mut self) {
        self.clear_timers();
        self.disconnect_settings();
    }
}
