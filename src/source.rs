/// An audio source. Anything implementing this trait may be played to an output stream.
pub trait Source {
    /// Retrieves the nth sample from the audio data represented by this Source object.
    /// If the input data has more than one channel, samples for each channel are expected to be interleaved.
    /// This function should return None if index is out-of-bounds (ie. is past the end of the input data.) However,
    /// input data may be generated endlessly by a Source object, in which case this function should never return None.
    fn get_sample(&self, index: usize) -> Option<f32>;

    /// Returns the number of channels in this Source object's audio data.
    fn channel_count(&self) -> usize;

    /// Returns the sample rate of this Source object's audio data.
    /// For example, a value of 44100 indicates that 44100 samples should be played per second.
    fn sample_rate(&self) -> usize;
}
