use crate::dsp::complex::{ComplexFloat, Complex32};
use crate::Lazy;
use alloc::boxed::Box;
use cmsis_dsp::transform::{Direction, FloatFft, OutputOrder};

macro_rules! fft_twiddle_table {
    ($bi:expr, $name:ident) => {
        static $name: Lazy<[Complex32; (1 << $bi) >> 1]> = Lazy::new(|| {
            const N: usize = 1 << $bi;
            let mut table = [Default::default(); N >> 1];
            let theta = core::f64::consts::PI / (N >> 1) as f64;
            for (k, t) in table.iter_mut().enumerate() {
                let angle = theta * k as f64;
                *t = Complex32::new(angle.cos() as f32, -angle.sin() as f32);
            }
            table
        });
    };
}

fft_twiddle_table!(13, FFT_TWIDDLE_TABLE_8192);
fft_twiddle_table!(14, FFT_TWIDDLE_TABLE_16384);
fft_twiddle_table!(15, FFT_TWIDDLE_TABLE_32768);
fft_twiddle_table!(16, FFT_TWIDDLE_TABLE_65536);

fn fft_twiddle_factors(n: usize) -> &'static [Complex32] {
    match n {
        8192 => FFT_TWIDDLE_TABLE_8192.as_ref(),
        16384 => FFT_TWIDDLE_TABLE_16384.as_ref(),
        32768 => FFT_TWIDDLE_TABLE_32768.as_ref(),
        65536 => FFT_TWIDDLE_TABLE_65536.as_ref(),
        _ => panic!("unsupported fft size {}", n),
    }
}

/// Compute the bit-reversal permutation table for a size-n FFT.
fn make_perm(n: usize) -> Box<[u16]> {
    let n16 = n as u16;
    let shift = n16.leading_zeros() + 1;
    (0..n16).map(|i| i.reverse_bits() >> shift).collect()
}

/// Recursive forward butterfly transform for n >= 4096.
/// Assumes `x` is already in bit-reversed order for size `n`.
/// Base case at n == 4096: delegates to hardware FFT, which handles its own
/// internal bit-reversal — this cancels the reversal already applied, so the
/// result is the correct forward FFT of the natural-order 4096-element block.
fn transform_fwd(x: &mut [Complex32], n: usize) {
    debug_assert!(n >= 4096 && n.is_power_of_two());

    if n == 4096 {
        FloatFft::new(4096u16)
            .unwrap()
            .run(x, Direction::Forward, OutputOrder::Standard);
        return;
    }

    let n_half = n >> 1;
    let (even, odd) = x.split_at_mut(n_half);

    transform_fwd(even, n_half);
    transform_fwd(odd, n_half);

    let twiddle = fft_twiddle_factors(n);

    for ((e, o), w) in even
        .chunks_exact_mut(2)
        .zip(odd.chunks_exact_mut(2))
        .zip(twiddle.chunks_exact(2))
    {
        let p0 = e[0];
        let q0 = o[0] * w[0];
        e[0] = p0 + q0;
        o[0] = p0 - q0;

        let p1 = e[1];
        let q1 = o[1] * w[1];
        e[1] = p1 + q1;
        o[1] = p1 - q1;
    }
}

/// Complex forward FFT. Hardware-accelerated for all power-of-two sizes up to
/// 65536, using a hardware 4096-point base case for sizes above 4096.
pub struct Fft {
    size: u16,
    /// Bit-reversal permutation for sizes > 4096. None for hardware-only sizes.
    perm: Option<Box<[u16]>>,
}

impl Fft {
    pub fn new(n: usize) -> Self {
        assert!(n.is_power_of_two(), "FFT size must be a power of two");
        assert!(n <= usize::from(u16::MAX), "FFT size too large");
        Self {
            size: n as u16,
            perm: if n > 4096 { Some(make_perm(n)) } else { None },
        }
    }

    pub fn size(&self) -> usize {
        usize::from(self.size)
    }

    /// In-place forward FFT.
    pub fn fft_inplace(&mut self, x: &mut [Complex32]) {
        let n = self.size();
        assert_eq!(x.len(), n);

        if n <= 4096 {
            FloatFft::new(self.size)
                .unwrap()
                .run(x, Direction::Forward, OutputOrder::Standard);
            return;
        }

        // Bit-reversal permutation for the full size.
        let perm = self.perm.as_ref().unwrap();
        for (i, &j) in perm.iter().enumerate() {
            let j = usize::from(j);
            if i < j {
                x.swap(i, j);
            }
        }

        transform_fwd(x, n);
    }

    /// Out-of-place forward FFT.
    pub fn fft(&mut self, x: &[Complex32], y: &mut [Complex32]) {
        let n = self.size();
        assert_eq!(x.len(), n);
        assert_eq!(y.len(), n);

        if n <= 4096 {
            y.copy_from_slice(x);
            FloatFft::new(self.size)
                .unwrap()
                .run(y, Direction::Forward, OutputOrder::Standard);
            return;
        }

        // Bit-reversal copy: write x into y in bit-reversed order.
        let perm = self.perm.as_ref().unwrap();
        for (&pi, dst) in perm.iter().zip(y.iter_mut()) {
            *dst = x[usize::from(pi)];
        }

        transform_fwd(y, n);
    }
}

/// Complex inverse FFT. Hardware-accelerated for all power-of-two sizes up to
/// 65536. For sizes above 4096, uses the conjugate trick so the forward
/// `transform_fwd` (with hardware 4096-point base cases) computes the inverse:
///   IFFT(x) = conj(FFT(conj(x))) / N
pub struct Ifft {
    size: u16,
    perm: Option<Box<[u16]>>,
}

impl Ifft {
    pub fn new(n: usize) -> Self {
        assert!(n.is_power_of_two(), "IFFT size must be a power of two");
        assert!(n <= usize::from(u16::MAX), "IFFT size too large");
        Self {
            size: n as u16,
            perm: if n > 4096 { Some(make_perm(n)) } else { None },
        }
    }

    pub fn size(&self) -> usize {
        usize::from(self.size)
    }

    /// In-place inverse FFT.
    pub fn ifft_inplace(&mut self, x: &mut [Complex32]) {
        let n = self.size();
        assert_eq!(x.len(), n);

        if n <= 4096 {
            FloatFft::new(self.size)
                .unwrap()
                .run(x, Direction::Inverse, OutputOrder::Standard);
            return;
        }

        // Conjugate trick: swap re/im during bit-reversal (equivalent to conjugating
        // the input), run the forward transform, then swap re/im and scale (equivalent
        // to conjugating the output and dividing by N).
        let perm = self.perm.as_ref().unwrap();
        for (i, &j) in perm.iter().enumerate() {
            let j = usize::from(j);
            if i <= j {
                let xi = x[i];
                let xj = x[j];
                // Swap re/im on both elements while doing bit-reversal.
                x[i] = Complex32::new(xj.im, xj.re);
                x[j] = Complex32::new(xi.im, xi.re);
            }
        }

        transform_fwd(x, n);

        let c = 1.0 / n as f32;
        for v in x.iter_mut() {
            *v = Complex32::new(c * v.im, c * v.re);
        }
    }

    /// Out-of-place inverse FFT.
    pub fn ifft(&mut self, x: &[Complex32], y: &mut [Complex32]) {
        let n = self.size();
        assert_eq!(x.len(), n);
        assert_eq!(y.len(), n);

        if n <= 4096 {
            y.copy_from_slice(x);
            FloatFft::new(self.size)
                .unwrap()
                .run(y, Direction::Inverse, OutputOrder::Standard);
            return;
        }

        // Bit-reversal copy with re/im swap (conjugate).
        let perm = self.perm.as_ref().unwrap();
        for (&pi, dst) in perm.iter().zip(y.iter_mut()) {
            let src = x[usize::from(pi)];
            *dst = Complex32::new(src.im, src.re);
        }

        transform_fwd(y, n);

        let c = 1.0 / n as f32;
        for v in y.iter_mut() {
            *v = Complex32::new(c * v.im, c * v.re);
        }
    }
}
