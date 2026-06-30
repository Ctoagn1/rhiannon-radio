use std::{ops::{AddAssign, Mul},  sync::mpsc::Receiver};

use num::{Zero, complex::Complex32};


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

    pub fn process(&mut self, input: T) -> Option<T> {
        self.history[self.pos] = input;
        self.history[self.pos + self.coeffs.len()] = input;

        let mut out: Option<T> = None;

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
            out = Some(acc);
        }
        self.pos = if self.pos == 0 { self.coeffs.len() - 1 } else { self.pos - 1 };
        self.d_count = (self.d_count + 1) % self.decimation;
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

    pub fn process(&mut self, input: Complex32) -> f32 {
        let diff = (input * self.prev.conj()).arg();
        self.prev = input;
        diff
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
            self.current_block = self.rx.recv().unwrap();
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
        self.rotation = Complex32::new(phase_inc.cos(), phase_inc.sin());
    }
    pub fn process(&mut self, sample: Complex32) -> Complex32 {
        self.osc *= self.rotation;
        sample * self.osc
    }
}