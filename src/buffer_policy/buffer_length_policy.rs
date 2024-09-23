use super::{BufferInstruction, BufferPolicy};

/// A buffer policy that limits the buffer to a certain length.
#[derive(Debug, Clone, Copy)]
pub struct BufferLengthPolicy {
    limit: usize,
    count: usize,
}

impl BufferLengthPolicy {
    /// Create a new buffer length policy.
    pub fn new(limit: usize) -> Self {
        Self { limit, count: 0 }
    }
}

impl<T> BufferPolicy<T> for BufferLengthPolicy {
    fn buffer_tail_policy(&mut self, _tail_item: &T) -> BufferInstruction {
        if self.limit <= self.count {
            log::debug!("Popping item due to length limit");
            BufferInstruction::Pop
        } else {
            log::debug!("Retaining tail due to low length");
            BufferInstruction::Retain
        }
    }

    fn on_before_send(&mut self, _new_item: &mut T) {
        self.count += 1;
        log::debug!("length increased: new_length: {}", self.count);
    }

    fn on_after_pop(&mut self, _popped_item: &mut T) {
        self.count -= 1;
        log::debug!("length decreased: new_length: {}", self.count);
    }
}

#[cfg(test)]
mod test {
    use crate::buffer_policy::{BufferInstruction, BufferLengthPolicy, BufferPolicy};

    #[test]
    fn test() {
        let mut policy = BufferLengthPolicy::new(2);

        assert_eq!(policy.buffer_tail_policy(&0), BufferInstruction::Retain);
        policy.on_before_send(&mut 0);

        assert_eq!(policy.buffer_tail_policy(&0), BufferInstruction::Retain);
        policy.on_before_send(&mut 0);

        assert_eq!(policy.buffer_tail_policy(&0), BufferInstruction::Pop);
        policy.on_after_pop(&mut 0);
        policy.on_before_send(&mut 0);

        assert_eq!(policy.buffer_tail_policy(&0), BufferInstruction::Pop);
        policy.on_after_pop(&mut 0);

        assert_eq!(policy.buffer_tail_policy(&0), BufferInstruction::Retain);
    }
}
