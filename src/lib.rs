mod error;
pub mod resampler;
pub mod source;
mod stream;

pub use error::Error;
pub use resampler::Resampler;
pub use source::Source;
pub use stream::OutputStream;

/// A basic sound-playing object. When fed to an output stream, will play the samples it contains until it has no more.
/// If the samples have a different sample rate than the output stream, the output will sound sped up or slowed down.
/// Use a resampler (such as boop::resampler::Polyphase, or implement your own) to resample it at the correct rate.
pub struct Player {
    pub samples: Box<[f32]>,
    pub channels: usize,
}

impl Source for Player {
    fn get_sample(&self, index: usize) -> Option<f32> {
        self.samples.get(index).copied()
    }

    fn channel_count(&self) -> usize {
        self.channels
    }

    fn sample_rate(&self) -> usize {
        48000 // This function will probably be removed soon and isn't currently used
    }
}
