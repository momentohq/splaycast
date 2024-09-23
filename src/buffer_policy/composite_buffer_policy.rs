use super::{BufferInstruction, BufferPolicy};

/// A buffer policy for joining buffer policies.
///
/// Composite policies only retain items that all policies agree to retain.
/// If any policy says to pop, the item is popped.
///
/// The upper policy is checked first. If it says to pop, the item is popped.
#[derive(Debug, Clone, Copy)]
pub struct CompositeBufferPolicy<T, U> {
    pub(super) upper: T,
    pub(super) lower: U,
}

impl<T, PUpper, PLower> BufferPolicy<T> for CompositeBufferPolicy<PUpper, PLower>
where
    PUpper: BufferPolicy<T>,
    PLower: BufferPolicy<T>,
{
    fn buffer_tail_policy(&mut self, tail_item: &T) -> BufferInstruction {
        match self.upper.buffer_tail_policy(tail_item) {
            BufferInstruction::Retain => {
                log::debug!("Upper policy retained tail - checking lower policy");
                match self.lower.buffer_tail_policy(tail_item) {
                    BufferInstruction::Retain => {
                        log::debug!("Lower policy retained tail - composite policy retains tail");
                        BufferInstruction::Retain
                    }
                    BufferInstruction::Pop => {
                        log::debug!("Lower policy pops tail - composite policy pops tail");
                        BufferInstruction::Pop
                    }
                }
            }
            BufferInstruction::Pop => {
                log::debug!("Upper policy pops tail - composite policy pops tail");
                BufferInstruction::Pop
            }
        }
    }

    fn on_before_send(&mut self, new_item: &mut T) {
        log::debug!("notifying policies of new item");
        self.upper.on_before_send(new_item);
        self.lower.on_before_send(new_item);
    }

    fn on_after_pop(&mut self, popped_item: &mut T) {
        log::debug!("notifying policies of popped item");
        self.upper.on_after_pop(popped_item);
        self.lower.on_after_pop(popped_item);
    }
}

/// Extension trait for building composite buffer policies.
pub trait BufferPolicyExtension<T, PLower>
where
    PLower: BufferPolicy<T>,
    Self: Sized,
{
    /// Wrap this buffer policy above a lower-priority buffer policy.
    ///
    /// Composite policies only retain items that all policies agree to retain.
    /// If any policy says to pop, the item is popped.
    fn wrap(self, lower: PLower) -> CompositeBufferPolicy<Self, PLower>;
}

impl<T, PUpper, PLower> BufferPolicyExtension<T, PLower> for PUpper
where
    PUpper: BufferPolicy<T>,
    PLower: BufferPolicy<T>,
{
    fn wrap(self, lower: PLower) -> CompositeBufferPolicy<PUpper, PLower> {
        CompositeBufferPolicy { upper: self, lower }
    }
}
