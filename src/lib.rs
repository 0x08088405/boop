mod error;
pub mod resampler;
pub mod source;
mod stream;

pub use error::Error;
pub use resampler::Resampler;
pub use source::Source;
pub use stream::OutputStream;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sinewaves() {
        // Sinewave generators
        struct Sinewave440();
        impl Source for Sinewave440 {
            fn get_sample(&self, index: usize) -> Option<f32> {
                if index < 66150 {
                    Some(((index as f32) * 440.0 * 2.0 * std::f32::consts::PI / 44100.0).sin())
                } else {
                    None
                }
            }

            fn channel_count(&self) -> usize {
                1
            }

            fn sample_rate(&self) -> usize {
                44100
            }
        }

        struct Sinewave800();
        impl Source for Sinewave800 {
            fn get_sample(&self, index: usize) -> Option<f32> {
                Some((((index / 2) as f32) * 800.0 * 2.0 * std::f32::consts::PI / 22000.0).sin())
            }

            fn channel_count(&self) -> usize {
                2
            }

            fn sample_rate(&self) -> usize {
                22000
            }
        }

        // Set up stream
        let stream = OutputStream::new().unwrap();

        // Play a 440 Hz 1-channel beep which will stop after 1.5 seconds, then wait 1 second
        stream.add_source(Sinewave440());
        std::thread::sleep(std::time::Duration::from_millis(1000));

        // Play an 800 Hz 2-channel beep, then wait 1 second
        stream.add_source(Sinewave800());
        std::thread::sleep(std::time::Duration::from_millis(1000));
    }
}
