use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    BuildStreamError, PlayStreamError, SampleFormat, SupportedStreamConfigsError,
};
use std::sync::{Arc, Mutex};

// Initial capacity for the Vec of audio sources in an OutputStream
const SOURCES_INIT_CAPACITY: usize = 8;

#[derive(Debug)]
pub enum Error {
    /// "catch-all" error type returned by CPAL in cases of unknown or unexpected errors
    CPALError(cpal::BackendSpecificError),

    /// The device no longer exists (ie. it has been disabled or unplugged)
    DeviceNotAvailable,

    /// The device doesn't support any of the playback configurations we can use
    DeviceNotUsable,

    /// An invalid argument was provided somewhere in the CPAL backend
    InvalidArgument,

    /// There is no output device available
    NoOutputDevice,

    /// Occurs if adding a new Stream ID would cause an integer overflow.
    StreamIdOverflow,
}

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

/// A source that is currently being played from.
/// Stores a Source object and metadata about how much of it has already been played.
struct ActiveSource {
    /// Source object being played from.
    pub source: Box<dyn Source + Send + Sync>,

    /// The number of samples that have been played so far.
    /// Note: this is the number of samples played at OUTPUT sample rate, ie. the sample rate of the OutputStream which
    /// owns this object, NOT the sample rate of the Source. This is to make resampling easier.
    pub sample_index: usize,
}

/// An audio output stream which plays audio sources. Capable of mixing multiple sources at once.
pub struct OutputStream {
    _stream: cpal::Stream,
    sources: Arc<Mutex<Vec<ActiveSource>>>,
}

impl OutputStream {
    // Sets up and returns an OutputStream
    pub fn new() -> Result<OutputStream, Error> {
        let err_fn = |err| eprintln!("an error occurred on the output audio stream: {}", err);

        let host = cpal::default_host();
        let device = match host.default_output_device() {
            Some(d) => d,
            None => return Err(Error::NoOutputDevice),
        };

        let mut supported_configs_range = match device.supported_output_configs() {
            Ok(r) => r,
            Err(SupportedStreamConfigsError::DeviceNotAvailable) => {
                return Err(Error::DeviceNotAvailable)
            }
            Err(SupportedStreamConfigsError::InvalidArgument) => {
                return Err(Error::InvalidArgument)
            }
            Err(SupportedStreamConfigsError::BackendSpecific { err }) => {
                return Err(Error::CPALError(err))
            }
        };
        let supported_config = match supported_configs_range.next() {
            Some(c) => c,
            None => return Err(Error::DeviceNotUsable),
        }
        .with_max_sample_rate();

        let sources: Arc<Mutex<Vec<ActiveSource>>> =
            Arc::new(Mutex::new(Vec::with_capacity(SOURCES_INIT_CAPACITY)));
        let closure_sources = sources.clone();

        let sample_rate = supported_config.sample_rate().0;
        let channel_count: u16 = supported_config.channels();

        let write_f32 = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            let output_channel_count = usize::from(channel_count);

            // Zero all the output samples
            data.iter_mut().for_each(|x| *x = 0.0);

            // Iterate slices of output data so that we're writing one sample per channel at a time
            let sources = &mut *closure_sources.lock().unwrap();
            for output_samples in data.chunks_exact_mut(output_channel_count) {
                // Go through all our sources and mix them into the output buffer
                // Note: retain() is used here so that we can mix while also removing any
                // empty generators from the list in one pass.
                sources.retain_mut(
                    |ActiveSource {
                         source,
                         sample_index,
                     }| {
                        let source_channel_count = source.channel_count();

                        // Calculate the actual poin in the source data that we want to interpolate
                        let source_point = ((source.sample_rate() as f64) / f64::from(sample_rate))
                            * (*sample_index / source_channel_count) as f64;
                        let idx_before_source_point = source_point.floor() as usize;

                        // If we're past the end of the audio data, indicate it by returning false
                        if source
                            .get_sample((idx_before_source_point + 1) * source_channel_count)
                            .is_none()
                        {
                            return false;
                        }

                        if source_channel_count == output_channel_count {
                            // Firstly, if the input and output channel counts are the same, pass straight through.
                            for (i, out_sample) in output_samples.iter_mut().enumerate() {
                                let start_sample = source
                                    .get_sample(
                                        (idx_before_source_point * source_channel_count) + i,
                                    )
                                    .unwrap_or_default();
                                let end_sample = source
                                    .get_sample(
                                        (idx_before_source_point * source_channel_count)
                                            + source_channel_count
                                            + i,
                                    )
                                    .unwrap_or_default();

                                *out_sample += start_sample
                                    + ((end_sample - start_sample) * source_point.fract() as f32);
                                *sample_index += 1;
                            }
                        } else if source_channel_count == 1 {
                            // Next, if the input is 1-channel, duplicate the next sample across all output channels.
                            let start_sample = source
                                .get_sample(idx_before_source_point)
                                .unwrap_or_default();
                            let end_sample = source
                                .get_sample(idx_before_source_point + 1)
                                .unwrap_or_default();

                            output_samples.iter_mut().for_each(|x| {
                                *x += start_sample
                                    + ((end_sample - start_sample) * source_point.fract() as f32)
                            });
                            *sample_index += 1;
                        } else {
                            // Different multi-channel counts. What do we do here!?
                            todo!("multi-channel mixing")
                        }

                        true
                    },
                );
            }
        };

        let write_i16 = move |_data: &mut [i16], _: &cpal::OutputCallbackInfo| todo!("write_i16");

        let write_u16 = move |_data: &mut [u16], _: &cpal::OutputCallbackInfo| todo!("write_u16");

        let sample_format = supported_config.sample_format();
        let config = supported_config.into();
        let stream = match match sample_format {
            SampleFormat::F32 => device.build_output_stream(&config, write_f32, err_fn),
            SampleFormat::I16 => device.build_output_stream(&config, write_i16, err_fn),
            SampleFormat::U16 => device.build_output_stream(&config, write_u16, err_fn),
        } {
            Ok(s) => s,
            Err(BuildStreamError::DeviceNotAvailable) => return Err(Error::DeviceNotAvailable),
            Err(BuildStreamError::StreamConfigNotSupported) => return Err(Error::DeviceNotUsable),
            Err(BuildStreamError::InvalidArgument) => return Err(Error::InvalidArgument),
            Err(BuildStreamError::StreamIdOverflow) => return Err(Error::StreamIdOverflow),
            Err(BuildStreamError::BackendSpecific { err }) => return Err(Error::CPALError(err)),
        };

        match stream.play() {
            Err(PlayStreamError::DeviceNotAvailable) => return Err(Error::DeviceNotAvailable),
            Err(PlayStreamError::BackendSpecific { err }) => return Err(Error::CPALError(err)),
            _ => (),
        }

        Ok(OutputStream {
            _stream: stream,
            sources,
        })
    }

    /// Adds an audio source to the output stream. The source will be played until it ends.
    pub fn add_source(&self, source: impl Source + Send + Sync + 'static) {
        let sources = &mut *self.sources.lock().unwrap();
        sources.push(ActiveSource {
            source: Box::new(source),
            sample_index: 0,
        });
    }
}

trait RetainMut<T> {
    fn retain_mut(&mut self, f: impl FnMut(&mut T) -> bool);
}

impl<T> RetainMut<T> for Vec<T> {
    fn retain_mut(&mut self, mut f: impl FnMut(&mut T) -> bool) {
        let len = self.len();
        let mut del = 0;
        {
            let v = &mut **self;

            for i in 0..len {
                if !f(&mut v[i]) {
                    del += 1;
                } else if del > 0 {
                    v.swap(i - del, i);
                }
            }
        }
        if del > 0 {
            self.truncate(len - del);
        }
    }
}

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
