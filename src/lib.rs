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

// Currently sets up an output stream and plays 0.5 seconds of 440Hz beep
pub fn sinewave() -> Result<(), Error> {
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

    let sample_rate = supported_config.sample_rate().0;
    let channel_count: u16 = supported_config.channels();
    let hz = 440.0;

    let mut i: usize = 0;
    let write_sine_f32 = move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
        let mut channel = 0;
        for sample in data.iter_mut() {
            let f = ((i as f32) * hz * 2.0 * std::f32::consts::PI / (sample_rate as f32)).sin();
            *sample = Sample::from(&f);
            channel += 1;
            if channel == channel_count {
                i = i.wrapping_add(1);
                channel = 0;
            }
        }
    };

    let write_sine_i16 = move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
        let mut channel = 0;
        for sample in data.iter_mut() {
            let f = ((i as f64) * f64::from(hz) * 2.0 * std::f64::consts::PI
                / (sample_rate as f64))
                .sin();
            let s = (f * f64::from(std::i16::MAX)) as i16;
            *sample = Sample::from(&s);
            channel += 1;
            if channel == channel_count {
                i = i.wrapping_add(1);
                channel = 0;
            }
        }
    };

    let write_sine_u16 = move |data: &mut [u16], _: &cpal::OutputCallbackInfo| {
        let mut channel = 0;
        for sample in data.iter_mut() {
            let f = ((i as f64) * f64::from(hz) * 2.0 * std::f64::consts::PI
                / (sample_rate as f64))
                .sin();
            let s = ((f * f64::from(std::i16::MAX)) + f64::from(std::i16::MAX)) as u16;
            *sample = Sample::from(&s);
            channel += 1;
            if channel == channel_count {
                i = i.wrapping_add(1);
                channel = 0;
            }
        }
    };

    let sample_format = supported_config.sample_format();
    let config = supported_config.into();
    let stream = match match sample_format {
        SampleFormat::F32 => device.build_output_stream(&config, write_sine_f32, err_fn),
        SampleFormat::I16 => device.build_output_stream(&config, write_sine_i16, err_fn),
        SampleFormat::U16 => device.build_output_stream(&config, write_sine_u16, err_fn),
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

    std::thread::sleep(std::time::Duration::from_millis(500));

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        sinewave().unwrap();
    }
}
