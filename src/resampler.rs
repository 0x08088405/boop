pub trait Resampler {
    fn new(source_rate: u32, dest_rate: u32) -> Self;
    fn resample(&self, input: &[f32], output: &mut [f32]) -> usize;
}

pub struct Polyphase {
    from: u32,
    to: u32,
    left_offset: u32,
    cutoff: f64,
}

impl Resampler for Polyphase {
    fn new(source_rate: u32, dest_rate: u32) -> Self {
        #[inline]
        fn gcd(a: u32, b: u32) -> u32 {
            if b == 0 {
                a
            } else {
                gcd(b, a % b)
            }
        }

        #[inline]
        fn kaiser_order(rejection: f64, transition: f64) -> u32 {
            if rejection > 21.0 {
                ((rejection - 7.95) / (2.285 * 2.0 * std::f64::consts::PI * transition)).ceil()
                    as u32
            } else {
                (5.79 / (2.0 * std::f64::consts::PI * transition)).ceil() as u32
            }
        }

        let gcd = gcd(source_rate, dest_rate);
        let from = source_rate / gcd;
        let to = dest_rate / gcd;

        let downscale_factor = f64::from(from.max(to));
        let cutoff = 0.475 / downscale_factor;
        let width = 0.05 / downscale_factor;
        let left_offset = (kaiser_order(180.0, width) + 1) / 2;

        Self {
            from,
            to,
            left_offset,
            cutoff,
        }
    }

    fn resample(&self, input: &[f32], output: &mut [f32]) -> usize {
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

            let x = f64::from(i) - f64::from(left);
            kaiser(x / f64::from(left)) * 2.0 * gain * cutoff * sinc(2.0 * cutoff * x)
        }

        let m = self.left_offset * 2 + 1;

        for (i, out_sample) in output.iter_mut().enumerate() {
            let start = self.left_offset + (self.from * i as u32);
            let mut j_f = start % self.to;
            let mut j_s = start / self.to;
            let mut r = 0.0f64;

            if j_f < m {
                // Pretty sure this is the C bound check. And leaves the output sample as 0.0 otherwise
                let mut filter_length = (m - j_f + self.to - 1) / self.to;

                if j_s + 1 > input.len() as u32 {
                    let skip = filter_length.min(j_s + 1 - input.len() as u32);
                    j_f += self.to * skip;
                    j_s -= skip;
                    filter_length -= skip;
                }

                for (_, s) in (0..filter_length).zip(input[0..=j_s as usize].iter().copied().rev())
                {
                    r += sinc_filter(self.left_offset, f64::from(self.to), self.cutoff, j_f)
                        * f64::from(s);
                    j_f += self.to;
                }
            }

            *out_sample = r as f32;
        }

        output.len()
    }
}
