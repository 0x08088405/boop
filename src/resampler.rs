/// Filter size used by the polyphase resampler - change this for quality/performance tradeoff
const FILTER_SIZE: u32 = 60;

pub trait Resampler {
    fn new(source_rate: u32, dest_rate: u32) -> Self;
    fn resample(&self, input: &[f32], channels: usize) -> Box<[f32]>;
}

pub struct Polyphase {
    from: u32,
    to: u32,
    left_offset: u64,
    kaiser_values: Box<[f64]>,
}

impl Resampler for Polyphase {
    fn new(source_rate: u32, dest_rate: u32) -> Self {
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
                    bessel_i0(18.87726 * (1.0 - k.powi(2)).sqrt()) / 14594424.752156679
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

        Self { from, to, left_offset: u64::from(left_offset), kaiser_values }
    }

    fn resample(&self, input: &[f32], channels: usize) -> Box<[f32]> {
        // 0 channels is a nonsensical request and would result in a division by zero.
        assert!(channels != 0);

        // Ensure input.len() is an exact multiple of the channel count.
        assert_eq!((input.len() / channels) * channels, input.len());

        let output_count = ((input.len() as f64) * (f64::from(self.to) / f64::from(self.from))).ceil() as u64;
        let from = u64::from(self.from);
        let to = u64::from(self.to);

        (0..output_count)
            .map(|i| {
                // Here, we calculate which input sample to start at and which set of kaiser values to use.
                // We first calculate an upscaled sample index ("start"), then take both its division and modulo
                // with our target sample rate. The int-division gives us a sample index in input data, and
                // the modulo gives us our kaiser offset.
                let start = self.left_offset + (from * i / channels as u64);
                // Putting these two calculations together means the compiler will do them with a single IDIV.
                let kaiser_index = start % to;
                let input_index = start / to;

                // Tells us which channel we're currently looking at in the output data.
                // We should only be using input data from the same channel.
                let channel = i as usize % channels;

                // Check if the range we need to access is entirely in-bounds
                if let Some(i) = input.get(0..((input_index + 1) as usize * channels)) {
                    // Multiply this set of input data by the relevant set of kaiser values and add them all together
                    i.iter()
                        .copied()
                        .rev()
                        .skip(channels - channel - 1)
                        .step_by(channels)
                        .zip(self.kaiser_values.iter().skip(kaiser_index as usize).step_by(to as usize))
                        .map(|(s, k)| f64::from(s) * k)
                        .sum::<f64>() as f32
                } else {
                    // The range of input data we want is partially past the end of the input data.
                    // Do similar to the above, but iterate from the end of the sound and skip `n` kaiser values
                    // where `n` is the number of samples we went past the end.
                    // The range being entirely OOB should never happen, but if it does, this will output 0.0 (silence).
                    let skip = input_index + 1 - (input.len() / channels) as u64;
                    input
                        .iter()
                        .copied()
                        .rev()
                        .skip(channels - channel - 1)
                        .step_by(channels)
                        .zip(self.kaiser_values.iter().skip((kaiser_index + (to * skip)) as usize).step_by(to as usize))
                        .map(|(s, k)| f64::from(s) * k)
                        .sum::<f64>() as f32
                }
            })
            .collect::<Vec<_>>()
            .into()
    }
}
