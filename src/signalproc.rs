use std::ops::{AddAssign, Mul};
use crossbeam_channel::{Receiver};
use num::{Zero, complex::Complex32, traits::ConstZero};
use rustfft::{FftPlanner, Fft};
use std::sync::{Arc, Mutex};

pub struct FirFilterDecimate<T> {
    coeffs: Vec<f32>,
    history: Vec<T>,
    pos: usize,
    decimation: usize,
    d_count: usize,
}

impl<T> FirFilterDecimate<T>
where 
    T: Copy + Zero + AddAssign + Mul<f32, Output = T>,
{
    pub fn new(coeffs: Vec<f32>, decimation: usize) -> Self {
        let len = coeffs.len();
        Self {
            coeffs,
            history: vec![T::zero(); len*2],
            pos: 0,
            decimation,
            d_count: 0,
        }
    }

    pub fn process(&mut self, input_vec: Vec<T>) -> Vec<T> {
        let mut out: Vec<T> = Vec::with_capacity(input_vec.len() / self.decimation + 1);
        for input in input_vec{
            self.history[self.pos] = input;
            self.history[self.pos + self.coeffs.len()] = input;

 
            if self.d_count == 0 {

                let mut acc: T = T::zero();
                let mut idx = self.pos;

                for x in &self.coeffs {
                    acc += self.history[idx] * *x;

                    if idx == 0 {
                        idx = self.history.len() - 1;
                    } else {
                        idx -= 1;
                    }
                }
                out.push(acc);
            }
            self.pos = if self.pos == 0 { self.coeffs.len() - 1 } else { self.pos - 1 };
            self.d_count = (self.d_count + 1) % self.decimation;
        }
        out
    }
}

pub struct FmDemod {
    prev: Complex32,
}

impl FmDemod {
    pub fn new(start: Complex32) -> Self {
        FmDemod { prev: start }
    }

    pub fn process(&mut self, inputs: Vec<Complex32>) -> Vec<f32> {
        inputs.iter().map(|input| {
                let diff = (input * self.prev.conj()).arg();
                self.prev = *input;
                diff
            }
            )
            .collect()
    }
}


pub struct AudioState {
    current_block: Vec<f32>,
    idx: usize,
    rx: Receiver<Vec<f32>>,
}

impl AudioState {

    pub fn new(rx: Receiver<Vec<f32>>) -> Self {
        AudioState {
            current_block: Vec::new(),
            idx: 0,
            rx,
        }
    }

    pub fn get_next_sample(&mut self) -> f32 {
        if self.idx >= self.current_block.len() {
            let next = self.rx.try_recv();
            if next.is_err() {
                self.current_block = vec![0.0; 100];
            }
            else{
                self.current_block = next.unwrap();
            }
            self.idx = 0;
        }

        let sample = self.current_block[self.idx];
        self.idx += 1;
        sample
    }
}
pub struct FreqShift {
    signal_freq: f32,
    sample_rate: usize,
    osc: Complex32,
    rotation: Complex32,
}

impl FreqShift {
    pub fn new(signal_freq: f32, sample_rate: usize) -> Self {
        let phase_inc = std::f32::consts::TAU * signal_freq / sample_rate as f32;
        FreqShift {
            signal_freq, 
            sample_rate, 
            osc: Complex32::new(1.0, 0.0),
            rotation: Complex32::new(phase_inc.cos(), -phase_inc.sin()),
        }
    }
    pub fn change_freq(&mut self, new_signal_freq: f32){
        self.signal_freq = new_signal_freq;
        let phase_inc = std::f32::consts::TAU * self.signal_freq / self.sample_rate as f32;
        self.rotation = Complex32::new(phase_inc.cos(), -phase_inc.sin());
    }
    pub fn process(&mut self, samples: Vec<Complex32>) -> Vec<Complex32> {
        let mut out = vec![Complex32::ZERO; samples.len()];
        let mut out_idx = 0;
        for s in samples {
            self.osc *= self.rotation;
            out[out_idx] = s * self.osc;
            out_idx += 1;
        }
        out
    }
}

pub struct FreqShiftReal {
    signal_freq: f32,
    sample_rate: usize,
    osc: f32,
    rotation: f32,
}

impl FreqShiftReal {
    pub fn new(signal_freq: f32, sample_rate: usize) -> Self {
        let phase_inc = std::f32::consts::TAU * signal_freq / sample_rate as f32;
        FreqShiftReal {
            signal_freq, 
            sample_rate, 
            osc: 0.0,
            rotation: phase_inc,
        }
    }
    pub fn change_freq(&mut self, new_signal_freq: f32){
        self.signal_freq = new_signal_freq;
        let phase_inc = std::f32::consts::TAU * self.signal_freq / self.sample_rate as f32;
        self.rotation = phase_inc;
    }
    pub fn process(&mut self, samples: Vec<f32>) -> Vec<f32> {
        let mut out = vec![0.0; samples.len()];
        let mut out_idx = 0;
        for s in samples {
            self.osc = (self.osc + self.rotation) % std::f32::consts::TAU;
            out[out_idx] = s * self.osc.cos();
            out_idx += 1;
        }
        out
    }
}

pub struct FftData {
    pub data: Vec<(f64, f64)>,
    pub center_freq: f64,
    pub freq_offset: f64,
    pub sample_rate: usize,


    fft: Arc<dyn Fft<f32>>,
    fft_planner: FftPlanner<f32>,
    fft_size: usize,
}
impl FftData {

    pub fn new(center_freq: f64, sample_rate: usize, fft_size: usize) -> Self {
        let mut fft_planner = FftPlanner::new();
        let fft = fft_planner.plan_fft_forward(fft_size);
        FftData {
            data: Vec::new(),
            center_freq,
            freq_offset: 0.0,
            sample_rate,
            fft,
            fft_planner,
            fft_size,
        }
    }

    pub fn update(&mut self, sample_rx: &Arc<Mutex<Vec<Complex32>>>) {
        let sample_block = sample_rx.lock().unwrap();

        let bin_width = self.sample_rate as f64 / self.fft_size as f64;
        let half = self.fft_size / 2;

        let mut padded_block;
        if sample_block.len() < self.fft_size {
            padded_block = vec![Complex32::ZERO; self.fft_size];
            padded_block[..sample_block.len()].copy_from_slice(&sample_block);
        }
        else {
            padded_block = sample_block.clone();
        }
        let window: Vec<f32> = (0..self.fft_size)
            .map(|i| {
                0.5 * (1.0 - (std::f32::consts::TAU * i as f32 / (self.fft_size as f32 - 1.0)).cos())
            })
            .collect();
        
        padded_block = (0..self.fft_size)
            .map(|i| padded_block[i] * window[i])
            .collect();

        self.fft.process(&mut padded_block);

        let norm_factor = 1.0 / self.fft_size as f64;
        self.data = (0..self.fft_size)
            .map(|i| {
                let idx = (i + half) % self.fft_size;

                let freq = self.center_freq + (i as f64 - half as f64) * bin_width;

                let amp = padded_block[idx].norm() as f64 * norm_factor;
                let mag = 20.0 * amp.max(1e-12).log10();

                (freq, mag)
            })
            .collect();
    }
    
    pub fn change_size(&mut self, new_size: usize) {
        self.fft = self.fft_planner.plan_fft_forward(new_size);
        self.fft_size = new_size;
    }
}

pub struct RdsSampler {
    samples_per_symbol: f32,
    phase: f32
}

impl RdsSampler {
    pub fn new(sample_rate: f32) -> Self{
        let symbol_rate = 1187.5;
        RdsSampler{samples_per_symbol: sample_rate / symbol_rate, phase: 0.0}
    }

    pub fn process(&mut self, signal: Vec<f32>) -> Vec<bool> {
        let mut out = Vec::with_capacity(signal.len() / self.samples_per_symbol as usize + 1);

        while (self.phase as usize) < signal.len() {
            let idx = self.phase as usize;
            let bit = signal[idx] > 0.0;
            out.push(bit);

            self.phase += self.samples_per_symbol;
        }
        self.phase -= signal.len() as f32;
        out
    }
    pub fn diff_demod(&mut self, samples: Vec<bool>) -> Vec<bool> {
        let mut prev = false;
        samples.iter().map(|x| {
            let out: bool = x ^ prev;
            prev = *x;
            out
        })
        .collect()
    }

}
