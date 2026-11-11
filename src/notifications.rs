use std::collections::VecDeque;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotificationKind {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct Notification {
    pub kind: NotificationKind,
    pub message: String,
}

impl Notification {
    pub fn info(message: impl Into<String>) -> Self {
        Self {
            kind: NotificationKind::Info,
            message: message.into(),
        }
    }

    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            kind: NotificationKind::Warning,
            message: message.into(),
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            kind: NotificationKind::Error,
            message: message.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Queue
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct NotificationQueue {
    pub items: VecDeque<Notification>,
}

impl NotificationQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, n: Notification) {
        self.items.push_back(n);
    }

    pub fn push_back(&mut self, n: Notification) {
        self.push(n);
    }

    pub fn dismiss(&mut self, index: usize) {
        self.items.remove(index);
    }
}
