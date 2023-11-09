mod shared;
mod splaycast;
mod splaycast_engine;
mod splaycast_receiver;

pub enum SplaycastMessage<T> {
    Entry { item: T },
    Lagged { count: usize },
}

impl<T> std::fmt::Debug for SplaycastMessage<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Entry { item: _ } => f.debug_struct("Entry").finish_non_exhaustive(),
            Self::Lagged { count } => f.debug_struct("Lagged").field("count", count).finish(),
        }
    }
}

pub use splaycast::Splaycast;
pub use splaycast_engine::SplaycastEngine;
pub use splaycast_receiver::SplaycastReceiver;

#[derive(Clone, Debug)]
pub(crate) struct SplaycastEntry<T> {
    pub id: u64,
    pub item: T,
}

impl<T> SplaycastEntry<T> {
    pub fn id(&self) -> u64 {
        self.id
    }
}
