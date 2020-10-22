use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BuildStreamError, PlayStreamError, Sample, SampleFormat, SupportedStreamConfigsError};

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

// Currently sets up an output stream and plays 0.5 seconds of silence to it
pub fn silence() -> Result<(), Error> {
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

    let sample_format = supported_config.sample_format();
    let config = supported_config.into();
    let stream = match match sample_format {
        SampleFormat::F32 => device.build_output_stream(&config, write_silence::<f32>, err_fn),
        SampleFormat::I16 => device.build_output_stream(&config, write_silence::<i16>, err_fn),
        SampleFormat::U16 => device.build_output_stream(&config, write_silence::<u16>, err_fn),
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

    fn write_silence<T: Sample>(data: &mut [T], _: &cpal::OutputCallbackInfo) {
        for sample in data.iter_mut() {
            *sample = Sample::from(&0.0);
        }
    }

    std::thread::sleep(std::time::Duration::from_millis(500));

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        silence().unwrap();
    }
}
