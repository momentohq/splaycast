use std::time::{Duration, Instant};

use super::{BufferInstruction, BufferPolicy};

/// A buffer policy that limits the buffer to a certain age.
#[derive(Debug, Clone, Copy)]
pub struct BufferAgePolicy<T, F> {
    age_limit: Duration,
    get_timestamp: F,
    _phantom: std::marker::PhantomData<T>,
}

impl<T, F: Fn(&T) -> Instant> BufferAgePolicy<T, F> {
    /// Create a new buffer age policy.
    ///
    /// Timestamps retrieved from the items are compared to the age limit. When
    /// the tail is older than age_limit, it is popped.
    pub fn new(age_limit: Duration, get_timestamp: F) -> Self {
        Self {
            age_limit,
            get_timestamp,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<T, F: Fn(&T) -> Instant> BufferPolicy<T> for BufferAgePolicy<T, F> {
    fn buffer_tail_policy(&mut self, tail_item: &T) -> BufferInstruction {
        if self.age_limit < (self.get_timestamp)(tail_item).elapsed() {
            log::debug!("Popping item due to age limit");
            BufferInstruction::Pop
        } else {
            log::debug!("Retaining tail due to low age");
            BufferInstruction::Retain
        }
    }

    fn on_before_send(&mut self, _new_item: &mut T) {
        // No bookkeeping needed.
    }

    fn on_after_pop(&mut self, _popped_item: &mut T) {
        // No bookkeeping needed.
    }
}

#[cfg(test)]
mod test {
    use std::time::{Duration, Instant};

    use crate::buffer_policy::{BufferAgePolicy, BufferInstruction, BufferPolicy};

    #[test]
    fn test() {
        let time = Instant::now();
        let mut policy = BufferAgePolicy::new(Duration::from_secs(1), |_: &usize| time);

        assert_eq!(policy.buffer_tail_policy(&0), BufferInstruction::Retain);

        let time = time - Duration::from_secs(2);
        let mut policy = BufferAgePolicy::new(Duration::from_secs(1), |_: &usize| time);

        assert_eq!(policy.buffer_tail_policy(&0), BufferInstruction::Pop);
    }
}
