use std::cell::{Cell, RefCell};

use gtk::glib;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Opening(u64),
    Visible(u32),
    Finishing(u32),
    Cancelling(u32),
}

pub struct Session {
    phase: Cell<Phase>,
    generation: Cell<u64>,
    boundary_task: RefCell<Option<glib::JoinHandle<()>>>,
    opening_task: RefCell<Option<glib::JoinHandle<()>>>,
}

impl Session {
    pub fn new() -> Self {
        Self {
            phase: Cell::new(Phase::Idle),
            generation: Cell::new(0),
            boundary_task: RefCell::new(None),
            opening_task: RefCell::new(None),
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase.get()
    }

    pub fn set_phase(&self, phase: Phase) {
        self.phase.set(phase);
    }

    pub fn begin_opening(&self) -> u64 {
        let generation = self.generation.get().wrapping_add(1);
        self.generation.set(generation);
        self.phase.set(Phase::Opening(generation));
        generation
    }

    pub fn cancel_opening(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
        self.phase.set(Phase::Idle);
    }

    pub fn is_opening(&self, generation: u64) -> bool {
        matches!(self.phase.get(), Phase::Opening(current) if current == generation)
    }

    pub fn active_token(&self) -> Option<u32> {
        match self.phase.get() {
            Phase::Visible(token) | Phase::Finishing(token) | Phase::Cancelling(token) => {
                Some(token)
            }
            Phase::Idle | Phase::Opening(_) => None,
        }
    }

    pub fn track_opening(&self, task: glib::JoinHandle<()>) {
        self.opening_task.replace(Some(task));
    }

    pub fn take_opening_task(&self) -> Option<glib::JoinHandle<()>> {
        self.opening_task.borrow_mut().take()
    }

    pub fn track_boundary(&self, task: glib::JoinHandle<()>) {
        if let Some(previous) = self.boundary_task.replace(Some(task)) {
            previous.abort();
        }
    }

    pub fn abort_boundary_task(&self) {
        if let Some(task) = self.boundary_task.borrow_mut().take() {
            task.abort();
        }
    }

    pub fn abort_opening_task(&self) {
        if let Some(task) = self.opening_task.borrow_mut().take() {
            task.abort();
        }
    }

    pub fn reset(&self) {
        self.phase.set(Phase::Idle);
    }
}
