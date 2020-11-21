use super::Source;

#[derive(Clone, Debug)]
pub struct WavPlayer {
    file: Vec<u8>,
    channels: usize,
    sample_rate: usize,
    sample_bytes: usize,
    next_sample_offset: usize,
    format: Format,
    length: usize,
}

#[derive(Clone, Copy, Debug)]
pub enum Error {
    /// This does not appear to be a .wav file
    InvalidFile,

    /// The audio data in this file is malformed
    MalformedData,

    /// The audio data in this file is encoded in a way we don't support
    UnknownFormat,
}

#[derive(Clone, Copy, Debug)]
pub enum Format {
    U8,
    I16,
    I24,
    I32,
    F32,
}

impl WavPlayer {
    pub fn new(file: impl Into<Vec<u8>>) -> Result<Self, Error> {
        let mut file = file.into();
        if file.len() < 36 || file[0..4] != [b'R', b'I', b'F', b'F'] || file[8..12] != [b'W', b'A', b'V', b'E'] {
            return Err(Error::InvalidFile)
        }

        let audio_format = i16::from_le_bytes([file[20], file[21]]);
        let channels = u16::from_le_bytes([file[22], file[23]]);
        let sample_rate = u32::from_le_bytes([file[24], file[25], file[26], file[27]]);
        let sample_bits = u16::from_le_bytes([file[34], file[35]]);

        let mut data_start: usize = 36;
        let data_len = loop {
            if file.len() < data_start + 8 {
                return Err(Error::InvalidFile)
            }
            let is_data_chunk = file[data_start..(data_start + 4)] == [b'd', b'a', b't', b'a'];
            let data_len = u32::from_le_bytes([
                file[data_start + 4],
                file[data_start + 5],
                file[data_start + 6],
                file[data_start + 7],
            ]) as usize;
            data_start += 8;
            if is_data_chunk {
                break data_len
            } else {
                data_start += data_len;
            }
        };

        let expected_file_length = data_len + data_start;
        if expected_file_length > file.len() {
            return Err(Error::MalformedData)
        } else {
            file.truncate(expected_file_length);
        }

        let format = match (audio_format, sample_bits) {
            (1, 8) => Format::U8,
            (1, 16) => Format::I16,
            (1, 24) => Format::I24,
            (1, 32) => Format::I32,
            (3, 32) => Format::F32,
            _ => return Err(Error::UnknownFormat),
        };

        let sample_bytes = usize::from(sample_bits / 8);

        Ok(Self {
            file,
            channels: channels.into(),
            sample_rate: sample_rate as usize,
            sample_bytes,
            next_sample_offset: data_start,
            format,
            length: data_len / sample_bytes,
        })
    }

    /// Returns the total number of samples in this wav file
    pub fn length(&self) -> usize {
        self.length
    }

    /// Returns the sample rate of this wav file (eg. 44100)
    pub fn sample_rate(&self) -> usize {
        self.sample_rate
    }
}

impl Source for WavPlayer {
    fn write_samples(&mut self, buffer: &mut [f32]) -> usize {
        use std::convert::TryInto;

        if let Some(i) = self.file.get(self.next_sample_offset..) {
            let output_iter = buffer.iter_mut();

            let samples_written;
            match self.format {
                Format::U8 => {
                    let iter = output_iter.zip(i.iter().copied());
                    samples_written = iter.len();
                    iter.for_each(|(out, b)| *out = get_sample_u8(b));
                },
                Format::I16 => {
                    let iter =
                        output_iter.zip(i.chunks_exact(2).map(|x| <&[u8] as TryInto<&[u8; 2]>>::try_into(x).unwrap()));
                    samples_written = iter.len();
                    iter.for_each(|(out, b)| *out = get_sample_i16(b));
                },
                Format::I24 => {
                    let iter =
                        output_iter.zip(i.chunks_exact(3).map(|x| <&[u8] as TryInto<&[u8; 3]>>::try_into(x).unwrap()));
                    samples_written = iter.len();
                    iter.for_each(|(out, b)| *out = get_sample_i24(b));
                },
                Format::I32 => {
                    let iter =
                        output_iter.zip(i.chunks_exact(4).map(|x| <&[u8] as TryInto<&[u8; 4]>>::try_into(x).unwrap()));
                    samples_written = iter.len();
                    iter.for_each(|(out, b)| *out = get_sample_i32(b));
                },
                Format::F32 => {
                    let iter =
                        output_iter.zip(i.chunks_exact(4).map(|x| <&[u8] as TryInto<&[u8; 4]>>::try_into(x).unwrap()));
                    samples_written = iter.len();
                    iter.for_each(|(out, b)| *out = get_sample_f32(b));
                },
            }

            self.next_sample_offset += samples_written * self.sample_bytes;
            samples_written
        } else {
            0
        }
    }

    fn channel_count(&self) -> usize {
        self.channels
    }
}

#[inline(always)]
fn get_sample_u8(data: u8) -> f32 {
    let sample = i16::from(data) - 0x80;
    f32::from(sample) / f32::from(i8::MAX)
}

#[inline(always)]
fn get_sample_i16(data: &[u8; 2]) -> f32 {
    let sample = i16::from_le_bytes(*data);
    f32::from(sample) / f32::from(i16::MAX)
}

#[inline(always)]
fn get_sample_i24(data: &[u8; 3]) -> f32 {
    let sample = i32::from_le_bytes([data[0], data[1], data[2], 0]);
    (sample as f32) / 8388608.0 // 2^23, or the imaginary i24::MAX
}

#[inline(always)]
fn get_sample_i32(data: &[u8; 4]) -> f32 {
    let sample = i32::from_le_bytes(*data);
    (f64::from(sample) / f64::from(i32::MAX)) as f32
}

#[inline(always)]
fn get_sample_f32(data: &[u8; 4]) -> f32 {
    f32::from_le_bytes(*data)
}
