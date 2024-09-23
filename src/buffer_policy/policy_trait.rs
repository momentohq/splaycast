/// What to do when a new item is about to go into the channel.
///
/// This tells the buffer when to remove or retain items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferInstruction {
    /// Keep the tail as-is.
    ///
    /// This instruction causes the buffer to keep the tail item. You can use this
    /// to tell the buffer how much to keep for subscribers.
    Retain,
    /// Remove the tail item.
    ///
    /// This instruction causes the buffer to remove the tail item, which will cause
    /// after_pop() to be called with that tail item.
    /// After after_pop() disposes of the tail item, before_send() is called again to
    /// determine what to do with the new item.
    Pop,
}

/// Determines when the buffer should pop or retain items.
///
/// This trait controls how the internal buffer is managed. If you pop the tail, you may
/// leave some subscribers behind and cause lag messages. However, if you retain forever,
/// you will run out of memory. This trait allows you to control the tradeoff.
///
/// You can look at the `buffer_policy` module for examples of how policies may be written,
/// and implement your own policy as is appropriate for your own channel.
pub trait BufferPolicy<T> {
    /// This method is called to determine how the channel buffer should be managed.
    fn buffer_tail_policy(&mut self, tail_item: &T) -> BufferInstruction;

    /// Called to notify when a new item is committed to the buffer.
    ///
    /// This happens before the item is sent to subscribers. It is called from the synchronous
    /// Engine context, so you get `&mut` access right before it becomes visible to subscribers.
    /// This is the real original item, and alterations here will be visible to subscribers.
    ///
    /// Policies that do bookkeeping on items should do it here. This is called once for each item.
    /// Policies may alter the item in place, e.g., to add a timestamp.
    fn on_before_send(&mut self, new_item: &mut T);

    /// Called to notify when an item is removed from the buffer.
    ///
    /// This happens after the item is removed from the buffer. It is called from the synchronous
    /// Engine context, so you get `&mut` access to the item that was removed. It is a clone of
    /// the original, however.
    ///
    /// Policies that do bookkeeping on items should do it here. This is called once for each item.
    /// Policies may alter the item in place, but remember that this is just a clone of the original.
    fn on_after_pop(&mut self, popped_item: &mut T);
}
