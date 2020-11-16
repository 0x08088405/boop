/// Filter size used by the polyphase resampler - change this for quality/performance tradeoff
const FILTER_SIZE: u32 = 240;

pub trait Resampler {
    fn new(source_rate: u32, dest_rate: u32) -> Self;
    fn resample(&self, input: &[f32]) -> Box<[f32]>;
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

        let kaiser_values = {
            let value_count = FILTER_SIZE * to;
            let mut v = Vec::with_capacity(value_count as usize);
            for i in 0..value_count {
                v.push(sinc_filter(left_offset, downscale_factor, cutoff, i))
            }
            v.into_boxed_slice()
        };

        Self { from, to, left_offset: u64::from(left_offset), kaiser_values }
    }

    fn resample(&self, input: &[f32]) -> Box<[f32]> {
        let output_count = ((input.len() as f64) * (f64::from(self.to) / f64::from(self.from))).ceil() as u64;
        let from = u64::from(self.from);
        let to = u64::from(self.to);
        let mut output: Vec<f32> = Vec::with_capacity(output_count as usize);

        for i in 0..output_count {
            // Here, we calculate which input sample to start at and which set of kaiser values to use.
            // We first calculate an upscaled sample index ("start"), then take both its division and modulo
            // with our target sample rate. The int-division gives us a sample index in input data, and
            // the modulo gives us our kaiser offset.
            let start = self.left_offset + (from * i);
            // Putting these two calculations together means the compiler will do them with a single IDIV.
            let kaiser_index = start % to;
            let input_index = start / to;

            // Check if the range we need to access is entirely in-bounds
            let sample = if let Some(i) = input.get(0..=input_index as usize) {
                // Multiply this set of input data by the relevant set of kaiser values and add them all together
                i.iter()
                    .copied()
                    .rev()
                    .zip(self.kaiser_values.iter().skip(kaiser_index as usize).step_by(to as usize))
                    .map(|(s, k)| f64::from(s) * k)
                    .sum::<f64>()
            } else {
                // The range of input data we want is partially past the end of the input data.
                // Do similar to the above, but iterate from the end of the sound and skip `n` kaiser values
                // where `n` is the number of samples we went past the end.
                // The range being entirely OOB should never happen, but if it does, this will output 0.0 (silence).
                let skip = input_index + 1 - input.len() as u64;
                input
                    .iter()
                    .copied()
                    .rev()
                    .zip(self.kaiser_values.iter().skip((kaiser_index + (to * skip)) as usize).step_by(to as usize))
                    .map(|(s, k)| f64::from(s) * k)
                    .sum::<f64>()
            };
            output.push(sample as f32);
        }

        output.into()
    }
}
