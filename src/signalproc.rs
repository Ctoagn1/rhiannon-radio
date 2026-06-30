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
            history: vec![T::zero(); len],
            pos: 0,
            decimation,
            d_count: 0,
        }
    }

    pub fn process(&mut self, input: T) -> Option<T> {
        self.history[self.pos] = input;

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
        self.pos = (self.pos + 1) % self.history.len();
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