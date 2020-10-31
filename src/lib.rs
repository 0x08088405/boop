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

/// An audio source. Contains an f32 generator and metadata.
struct Source {
    pub generator: Box<dyn FnMut() -> Option<f32> + Send + Sync>,
    pub channel_count: usize,
    pub sample_rate: usize,
}

/// An audio output stream which plays audio sources. Capable of mixing multiple sources at once.
pub struct OutputStream {
    _stream: cpal::Stream,
    sources: Arc<Mutex<Vec<Source>>>,
}

// Sets up and returns an OutputStream
pub fn setup() -> Result<OutputStream, Error> {
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
        Err(SupportedStreamConfigsError::InvalidArgument) => return Err(Error::InvalidArgument),
        Err(SupportedStreamConfigsError::BackendSpecific { err }) => {
            return Err(Error::CPALError(err))
        }
    };
    let supported_config = match supported_configs_range.next() {
        Some(c) => c,
        None => return Err(Error::DeviceNotUsable),
    }
    .with_max_sample_rate();

    let sources = Arc::new(Mutex::new(Vec::with_capacity(SOURCES_INIT_CAPACITY)));
    let closure_sources = sources.clone();

    let _sample_rate = supported_config.sample_rate().0; // TODO: interpolation
    let channel_count: u16 = supported_config.channels();

    let write_f32 = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
        let output_channel_count = usize::from(channel_count);

        // Zero all the output samples
        data.iter_mut().for_each(|x| *x = 0.0);

        // Iterate slices of output data so that we're writing one sample per channel at a time
        let sources: &mut Vec<Source> = &mut *closure_sources.lock().unwrap();
        for output_samples in data.chunks_exact_mut(output_channel_count) {
            // Go through all our sources and mix them into the output buffer
            // Note: retain() is used here so that we can mix while also removing any
            // empty generators from the list in one pass.
            sources.retain_mut(|source| {
                if source.channel_count == output_channel_count {
                    // Firstly, if the input and output channel counts are the same, pass straight through.
                    for out_sample in output_samples.iter_mut() {
                        if let Some(in_sample) = source.generator.as_mut()() {
                            *out_sample += in_sample;
                        } else {
                            return false;
                        }
                    }
                } else if source.channel_count == 1 {
                    // Next, if the input is 1-channel, duplicate the next sample across all output channels.
                    if let Some(in_sample) = source.generator.as_mut()() {
                        output_samples.iter_mut().for_each(|x| *x += in_sample);
                    } else {
                        return false;
                    }
                } else {
                    // Different multi-channel counts. What do we do here!?
                    todo!("multi-channel mixing")
                }

                true
            });
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

impl OutputStream {
    /// Adds an audio source to the output stream. The source will be played from until it ends.
    pub fn add_source(
        &self,
        generator: Box<dyn FnMut() -> Option<f32> + Send + Sync>,
        channel_count: usize,
        sample_rate: usize,
    ) {
        let sources = &mut *self.sources.lock().unwrap();
        sources.push(Source {
            generator,
            channel_count,
            sample_rate,
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
    fn sinewave() {
        // Sinewave generators
        let mut i: usize = 0;
        let sinewave440_1chan = move || -> Option<f32> {
            if i < 72000 {
                let f = ((i as f32) * 440.0 * 2.0 * std::f32::consts::PI / 48000.0).sin();
                i = i.wrapping_add(1);
                Some(f)
            } else {
                None
            }
        };
        let mut i: usize = 0;
        let mut b = true;
        let sinewave800_2chan = move || -> Option<f32> {
            let f = ((i as f32) * 800.0 * 2.0 * std::f32::consts::PI / 48000.0).sin();
            if b {
                b = false;
            } else {
                i = i.wrapping_add(1);
                b = true;
            }
            Some(f)
        };

        // Set up stream
        let stream = setup().unwrap();

        // Play a 440 Hz 1-channel beep which will stop after 1.5 seconds, then wait 1 second
        stream.add_source(Box::new(sinewave440_1chan), 1, 48000);
        std::thread::sleep(std::time::Duration::from_millis(1000));

        // Play an 800 Hz 2-channel beep, then wait 1 second
        stream.add_source(Box::new(sinewave800_2chan), 2, 48000);
        std::thread::sleep(std::time::Duration::from_millis(1000));
    }
}
