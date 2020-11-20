use crate::Source;

/// Filter size used by the polyphase resampler - change this for quality/performance tradeoff
const FILTER_SIZE: u32 = 60;

/// Implementation of a PQF resampler. Construct with: Resampler::new(source, source_rate, dest_rate)
/// Once constructed, it will behave as a Source object which outputs samples at the target sample rate.
pub struct Resampler<S>
where
    S: Source,
{
    source: S,
    from: u32,
    to: u32,
    left_offset: u64,
    kaiser_values: Box<[f64]>,
    filter_1: Box<[f32]>,
    filter_2: Box<[f32]>,

    // The size of the entire filter including both buffers
    whole_filter_size: usize,

    // The size of each individual buffer
    buffer_size: usize,

    // How many input samples were already discarded before the start of the current filter
    input_offset: u64,

    // How many output samples have been written so far
    output_count: usize,

    // The last valid sample in the filter, if the source ended and wasn't able to fill the entire buffer
    last_sample: Option<usize>,
}

impl<S: Source> Resampler<S> {
    pub fn new(mut source: S, source_rate: u32, dest_rate: u32) -> Self {
        assert!(source_rate != 0);
        assert!(dest_rate != 0);

        #[inline]
        fn gcd(a: u32, b: u32) -> u32 {
            if b == 0 { a } else { gcd(b, a % b) }
        }

        fn sinc_filter(left: u32, gain: f64, cutoff: f64, i: u32) -> f64 {
            #[inline]
            fn sinc(x: f64) -> f64 {
                if x == 0.0 {
                    1.0
                } else {
                    let x_pi = x * std::f64::consts::PI;
                    x_pi.sin() / x_pi
                }
            }

            #[inline]
            fn bessel_i0(x: f64) -> f64 {
                // Just trust me on this one
                let ax = x.abs();
                if ax < 3.75 {
                    let y = (x / 3.75).powi(2);
                    1.0 + y
                        * (3.5156229
                            + y * (3.0899424 + y * (1.2067492 + y * (0.2659732 + y * (0.360768e-1 + y * 0.45813e-2)))))
                } else {
                    let y = 3.75 / ax;
                    (ax.exp() / ax.sqrt())
                        * (0.39894228
                            + y * (0.1328592e-1
                                + y * (0.225319e-2
                                    + y * (-0.157565e-2
                                        + y * (0.916281e-2
                                            + y * (-0.2057706e-1
                                                + y * (0.2635537e-1 + y * (-0.1647633e-1 + y * 0.392377e-2))))))))
                }
            }

            #[inline]
            fn kaiser(k: f64) -> f64 {
                if k < -1.0 || k > 1.0 {
                    0.0
                } else {
                    // 18.87726 is the Kaiser beta value for a rejection of 180 dB.
                    // The magic number at the end is bessel_i0(18.87726)
                    // Since our bessel_i0 is a loose approximation, I've manually raised it a bit to prevent clipping
                    // It was previously 14594424.752156679
                    bessel_i0(18.87726 * (1.0 - k.powi(2)).sqrt()) / 14642294.465343751
                }
            }

            let left = f64::from(left);
            let x = f64::from(i) - left;
            kaiser(x / left) * 2.0 * gain * cutoff * sinc(2.0 * cutoff * x)
        }

        let gcd = gcd(source_rate, dest_rate);
        let from = source_rate / gcd;
        let to = dest_rate / gcd;

        let downscale_factor = f64::from(to);
        let cutoff = 0.475 / downscale_factor;
        let left_offset = (FILTER_SIZE / 2) * to;

        let kaiser_values = (0..(FILTER_SIZE * to))
            .map(|i| sinc_filter(left_offset, downscale_factor, cutoff, i))
            .collect::<Vec<_>>()
            .into_boxed_slice();

        let filter_samples = FILTER_SIZE as usize * source.channel_count();
        let mut filter_1 = Vec::with_capacity(filter_samples);
        let mut filter_2 = Vec::with_capacity(filter_samples);

        unsafe {
            filter_1.set_len(filter_samples);
            filter_2.set_len(filter_samples);
        }

        let last_sample = {
            let len = source.write_samples(&mut filter_1);
            if len == filter_samples {
                let len = source.write_samples(&mut filter_2);
                if len == filter_samples { None } else { Some(len) }
            } else {
                Some(len)
            }
        };

        Self {
            source,
            from,
            to,
            left_offset: u64::from(left_offset),
            kaiser_values,
            filter_1: filter_1.into_boxed_slice(),
            filter_2: filter_2.into_boxed_slice(),
            whole_filter_size: filter_samples * 2,
            buffer_size: filter_samples,
            input_offset: 0,
            output_count: 0,
            last_sample,
        }
    }
}

impl<S: Source> Source for Resampler<S> {
    fn write_samples(&mut self, buffer: &mut [f32]) -> usize {
        let from = u64::from(self.from);
        let to = u64::from(self.to);
        let channels = self.source.channel_count();

        for (i, s) in buffer.iter_mut().enumerate() {
            // Tells us which channel we're currently looking at in the output data.
            // We should only be using input data from the same channel.
            let channel = self.output_count % channels;

            // Here, we calculate which input sample to start at and which set of kaiser values to use.
            // We first calculate an upscaled sample index ("start"), then take both its division and modulo
            // with our target sample rate. The int-division gives us a sample index in input data, and
            // the modulo gives us our kaiser offset.
            let start = self.left_offset + (from * (self.output_count as u64 / channels as u64));
            let kaiser_index = start % to;
            let input_index = start / to;

            // input_index doesn't respect multi-channel tracks and ignores our filter setup, so now we'll
            // translate it into a sample in our filter.
            let mut sample_index = (input_index * channels as u64) + channel as u64 - self.input_offset;

            // sample_index is where we start counting backwards, so if it's beyond the length of our two filters
            // added together, then we need new data.
            // However, don't try to get new data if the source has already been emptied (ie. we have a last_sample).
            while (sample_index >= self.whole_filter_size as u64) && self.last_sample.is_none() {
                // Read new samples into filter 1, which is now fully depleted, so it's fine to overwrite it.
                let len = self.source.write_samples(&mut self.filter_1);
                // Handle our source being empty
                if len != self.filter_1.len() {
                    self.last_sample = Some(self.buffer_size + len);
                }
                // Swap filters 1 and 2. Now the new samples are in filter_2. Turbofish here guarantees O(1) ptr swap
                std::mem::swap::<Box<_>>(&mut self.filter_1, &mut self.filter_2);
                // And finally set our sample index back and input offset forward appropriately.
                let sample_count = self.buffer_size as u64;
                sample_index -= sample_count;
                self.input_offset += sample_count;
            }

            // If we are past the end of our audio, exit early and indicate how much of the buffer we filled
            // This does leave off the last few samples of input audio. Worth fixing? Probably not.
            if let Some(end) = self.last_sample {
                if sample_index as usize >= end {
                    return i
                }
            }

            // Multiply this set of input data by the relevant set of kaiser values and add them all together
            *s = self
                .filter_2
                .iter()
                .rev()
                .chain(self.filter_1.iter().rev())
                .copied()
                .skip(self.filter_1.len() * 2 - sample_index as usize - 1)
                .step_by(channels)
                .zip(self.kaiser_values.iter().skip(kaiser_index as usize).step_by(to as usize))
                .map(|(s, k)| f64::from(s) * k)
                .sum::<f64>() as f32;

            self.output_count += 1;
        }

        buffer.len()
    }

    fn channel_count(&self) -> usize {
        self.source.channel_count()
    }
}
