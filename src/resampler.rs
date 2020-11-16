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
            if b == 0 {
                a
            } else {
                gcd(b, a % b)
            }
        }

        #[inline]
        fn kaiser_order(rejection: f64, transition: f64) -> u64 {
            if rejection > 21.0 {
                ((rejection - 7.95) / (2.285 * 2.0 * std::f64::consts::PI * transition)).ceil()
                    as u64
            } else {
                (5.79 / (2.0 * std::f64::consts::PI * transition)).ceil() as u64
            }
        }

        fn sinc_filter(left: u64, gain: f64, cutoff: f64, i: u64) -> f64 {
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
                            + y * (3.0899424
                                + y * (1.2067492
                                    + y * (0.2659732 + y * (0.360768e-1 + y * 0.45813e-2)))))
                } else {
                    let y = 3.75 / ax;
                    (ax.exp() / ax.sqrt())
                        * (0.39894228
                            + y * (0.1328592e-1
                                + y * (0.225319e-2
                                    + y * (-0.157565e-2
                                        + y * (0.916281e-2
                                            + y * (-0.2057706e-1
                                                + y * (0.2635537e-1
                                                    + y * (-0.1647633e-1 + y * 0.392377e-2))))))))
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

            let left = left as f64;
            let x = (i as f64) - left;
            kaiser(x / left) * 2.0 * gain * cutoff * sinc(2.0 * cutoff * x)
        }

        let gcd = gcd(source_rate, dest_rate);
        let from = source_rate / gcd;
        let to = dest_rate / gcd;

        let downscale_factor = f64::from(to);
        let cutoff = 0.475 / downscale_factor;
        let width = 0.05 / downscale_factor;
        let left_offset = (kaiser_order(180.0, width) + 1) / 2;

        let kaiser_values = {
            let value_count = left_offset * 2 + 1;
            let mut v = Vec::with_capacity(value_count as usize);
            for i in 0..value_count {
                v.push(sinc_filter(left_offset, downscale_factor, cutoff, i))
            }
            v.into_boxed_slice()
        };

        Self {
            from,
            to,
            left_offset,
            kaiser_values,
        }
    }

    fn resample(&self, input: &[f32]) -> Box<[f32]> {
        let kaiser_order = self.kaiser_values.len() as u64;
        let output_count = ((input.len() as f64) * (f64::from(self.to) / f64::from(self.from))).ceil() as u64;
        let from = u64::from(self.from);
        let to = u64::from(self.to);
        let mut output: Vec<f32> = Vec::with_capacity(output_count as usize);

        for i in 0..output_count {
            let start = self.left_offset + (from * i);
            let mut kaiser_index = start % to;
            let mut input_index = start / to;
            let mut r = 0.0f64;

            if kaiser_index < kaiser_order {
                let mut filter_length = (kaiser_order - kaiser_index + to - 1) / to;

                if input_index >= input.len() as u64 {
                    let skip = filter_length.min(input_index - (input.len() as u64 + 1));
                    kaiser_index += to * skip;
                    input_index -= skip;
                    filter_length -= skip;
                }

                for s in input[0..=input_index as usize].iter().copied().rev().take(filter_length as usize) {
                    r += self.kaiser_values[kaiser_index as usize] * f64::from(s);
                    kaiser_index += to;
                }
            }

            output.push(r as f32);
        }

        output.into()
    }
}
