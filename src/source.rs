pub mod wav;

/// An audio source. Anything implementing this trait may be played to an output stream.
pub trait Source {
    /// Writes the next set of samples to an output buffer
    /// TODO: doc this better
    fn write_samples(&mut self, buffer: &mut [f32]) -> usize;

    /// Returns the number of channels in this Source object's audio data.
    fn channel_count(&self) -> usize;
}
