use crate::{Error, Source};
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    BuildStreamError, PlayStreamError, SampleFormat, SupportedStreamConfigsError,
};
use std::sync::{Arc, Mutex};

// Initial capacity for the Vec of audio sources in an OutputStream
const SOURCES_INIT_CAPACITY: usize = 8;

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
            Err(SupportedStreamConfigsError::DeviceNotAvailable) => return Err(Error::DeviceNotAvailable),
            Err(SupportedStreamConfigsError::InvalidArgument) => return Err(Error::InvalidArgument),
            Err(SupportedStreamConfigsError::BackendSpecific { err }) => return Err(Error::CPALError(err)),
        };
        let supported_config = match supported_configs_range.next() {
            Some(c) => c,
            None => return Err(Error::DeviceNotUsable),
        }
        .with_max_sample_rate();

        let sources: Arc<Mutex<Vec<ActiveSource>>> = Arc::new(Mutex::new(Vec::with_capacity(SOURCES_INIT_CAPACITY)));
        let closure_sources = sources.clone();

        let _sample_rate = supported_config.sample_rate().0; // TODO: probably store this or expose it somewhere
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
                sources.retain_mut(|ActiveSource { source, sample_index }| {
                    let source_channel_count = source.channel_count();

                    if source_channel_count == output_channel_count {
                        // Firstly, if the input and output channel counts are the same, pass straight through.
                        for out_sample in output_samples.iter_mut() {
                            if let Some(s) = source.get_sample(*sample_index) {
                                *out_sample += s;
                                *sample_index += 1;
                            } else {
                                return false
                            }
                        }
                        true
                    } else if source_channel_count == 1 {
                        if let Some(s) = source.get_sample(*sample_index) {
                            // Next, if the input is 1-channel, duplicate the next sample across all output channels.
                            output_samples.iter_mut().for_each(|x| {
                                *x += s;
                            });
                            *sample_index += 1;
                            true
                        } else {
                            false
                        }
                    } else {
                        // Different multi-channel counts. What do we do here!?
                        todo!("multi-channel mixing")
                    }
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

        Ok(OutputStream { _stream: stream, sources })
    }

    /// Adds an audio source to the output stream. The source will be played until it ends.
    pub fn add_source(&self, source: impl Source + Send + Sync + 'static) {
        let sources = &mut *self.sources.lock().unwrap();
        sources.push(ActiveSource { source: Box::new(source), sample_index: 0 });
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
