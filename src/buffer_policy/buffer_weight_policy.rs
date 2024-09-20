use super::{BufferInstruction, BufferPolicy};

/// A buffer policy that limits the buffer to a certain weight.
#[derive(Debug, Clone, Copy)]
pub struct BufferWeightPolicy<T, F> {
    weight_limit: usize,
    weight: usize,
    get_weight: F,
    _phantom: std::marker::PhantomData<T>,
}

impl<T, F: Fn(&T) -> usize> BufferWeightPolicy<T, F> {
    /// Create a new buffer weight policy.
    ///
    /// Weights retrieved from the items are compared to the weight limit. When
    /// the buffer is heavier than weight_limit, the tail is popped.
    ///
    /// The weight limit is soft.
    pub fn new(weight_limit: usize, get_weight: F) -> Self {
        Self {
            weight_limit,
            weight: 0,
            get_weight,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<T, F: Fn(&T) -> usize> BufferPolicy<T> for BufferWeightPolicy<T, F> {
    fn buffer_tail_policy(&mut self, _tail_item: &T) -> BufferInstruction {
        if self.weight_limit < self.weight {
            log::debug!("Popping item due to weight limit");
            BufferInstruction::Pop
        } else {
            log::debug!("Retaining tail due to low weight");
            BufferInstruction::Retain
        }
    }

    fn on_before_send(&mut self, new_item: &mut T) {
        self.weight = self.weight.saturating_add((self.get_weight)(new_item));
        log::debug!("weight increased: new_weight: {}", self.weight);
    }

    fn on_after_pop(&mut self, popped_item: &mut T) {
        self.weight = self.weight.saturating_sub((self.get_weight)(popped_item));
        log::debug!("weight decreased: new_weight: {}", self.weight);
    }
}

#[cfg(test)]
mod test {
    use crate::buffer_policy::{BufferInstruction, BufferPolicy, BufferWeightPolicy};

    #[test]
    fn test() {
        // just treat the new item as a weight
        let mut policy = BufferWeightPolicy::new(2, |item: &usize| *item);

        policy.on_before_send(&mut 0);
        assert_eq!(policy.buffer_tail_policy(&0), BufferInstruction::Retain);

        policy.on_before_send(&mut 1);
        assert_eq!(policy.buffer_tail_policy(&1), BufferInstruction::Retain);

        policy.on_before_send(&mut 2);
        assert_eq!(policy.buffer_tail_policy(&2), BufferInstruction::Pop);

        policy.on_after_pop(&mut 1);
        assert_eq!(policy.buffer_tail_policy(&3), BufferInstruction::Retain);

        policy.on_before_send(&mut 1);
        assert_eq!(policy.buffer_tail_policy(&4), BufferInstruction::Pop);
    }
}
