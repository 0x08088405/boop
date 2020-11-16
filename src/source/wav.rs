use super::Source;

pub struct WavPlayer {
    file: Vec<u8>,
    channels: usize,
    sample_rate: usize,
    block_align: usize,
    data_start: usize,
    data_end: usize,
    sample_getter: fn(&[u8], usize) -> Option<f32>,
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

impl WavPlayer {
    pub fn new(file: impl Into<Vec<u8>>) -> Result<Self, Error> {
        let mut file = file.into();
        if file.len() < 36
            || file[0..4] != [b'R', b'I', b'F', b'F']
            || file[8..12] != [b'W', b'A', b'V', b'E']
        {
            return Err(Error::InvalidFile);
        }

        let audio_format = i16::from_le_bytes([file[20], file[21]]);
        let channels = u16::from_le_bytes([file[22], file[23]]);
        let sample_rate = u32::from_le_bytes([file[24], file[25], file[26], file[27]]);
        let block_align = u16::from_le_bytes([file[32], file[33]]);
        let sample_bits = u16::from_le_bytes([file[34], file[35]]);

        let mut data_start: usize = 36;
        let data_len = loop {
            if file.len() < data_start + 8 {
                return Err(Error::InvalidFile);
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
                break data_len;
            } else {
                data_start += data_len;
            }
        };

        let expected_file_length = data_len + data_start;
        if expected_file_length > file.len() {
            return Err(Error::MalformedData);
        } else {
            file.truncate(expected_file_length);
        }

        let sample_getter = match (audio_format, sample_bits) {
            (1, 8) => get_sample_u8,
            (1, 16) => get_sample_i16,
            (1, 24) => get_sample_i24,
            (1, 32) => get_sample_i32,
            (3, 32) => get_sample_f32,
            _ => return Err(Error::UnknownFormat),
        };

        Ok(Self {
            file,
            channels: channels.into(),
            sample_rate: sample_rate as usize,
            block_align: block_align.into(),
            data_start,
            data_end: data_start + data_len,
            sample_getter,
        })
    }
}

impl Source for WavPlayer {
    fn get_sample(&self, index: usize) -> Option<f32> {
        let offset = index * self.block_align;
        (self.sample_getter)(&self.file[self.data_start..self.data_end], offset)
    }

    fn channel_count(&self) -> usize {
        self.channels
    }

    fn sample_rate(&self) -> usize {
        self.sample_rate
    }
}

fn get_sample_u8(data: &[u8], offset: usize) -> Option<f32> {
    let sample = data.get(offset).copied().map(i16::from)? - 0x80;
    Some(f32::from(sample) / f32::from(i8::MAX))
}

fn get_sample_i16(data: &[u8], offset: usize) -> Option<f32> {
    let sample = i16::from_le_bytes([data.get(offset).copied()?, data.get(offset + 1).copied()?]);
    Some(f32::from(sample) / f32::from(i16::MAX))
}

fn get_sample_i24(data: &[u8], offset: usize) -> Option<f32> {
    let sample = i32::from_le_bytes([
        data.get(offset).copied()?,
        data.get(offset + 1).copied()?,
        data.get(offset + 2).copied()?,
        0,
    ]);
    Some((sample as f32) / 8388608.0)
}

fn get_sample_i32(data: &[u8], offset: usize) -> Option<f32> {
    let sample = i32::from_le_bytes([
        data.get(offset).copied()?,
        data.get(offset + 1).copied()?,
        data.get(offset + 2).copied()?,
        data.get(offset + 3).copied()?,
    ]);
    Some((f64::from(sample) / f64::from(i32::MAX)) as f32)
}

fn get_sample_f32(data: &[u8], offset: usize) -> Option<f32> {
    let sample = f32::from_le_bytes([
        data.get(offset).copied()?,
        data.get(offset + 1).copied()?,
        data.get(offset + 2).copied()?,
        data.get(offset + 3).copied()?,
    ]);
    Some(sample)
}
