use crate::{Error, Mixer, Source};
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    BuildStreamError, PlayStreamError, SampleFormat, SupportedStreamConfigsError,
};
use std::sync::{Arc, Mutex};

/// An audio output stream which plays audio sources. Capable of mixing multiple sources at once.
pub struct OutputStream {
    _stream: cpal::Stream,
    source: Arc<Mutex<Mixer>>,
}

impl OutputStream {
    // Sets up and returns an OutputStream
    pub fn new() -> Result<Self, Error> {
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

        let _sample_rate = supported_config.sample_rate().0; // TODO: probably store this or expose it somewhere
        let channel_count: u16 = supported_config.channels();

        let source: Arc<Mutex<Mixer>> = Arc::new(Mutex::new(Mixer::new(channel_count.into())));
        let closure_source = source.clone();

        let write_f32 = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            closure_source.lock().unwrap().write_samples(data);
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

        Ok(OutputStream { _stream: stream, source })
    }

    /// Adds an audio source to the output stream. The source will be played until it ends.
    pub fn add_source(&self, source: impl Source + Send + Sync + 'static) {
        self.source.lock().unwrap().add_source(source);
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
