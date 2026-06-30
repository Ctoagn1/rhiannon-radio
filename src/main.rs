use rs_rtl::{DeviceId, RtlSdr, AsyncReadHandle};
use std::{error::Error, sync::mpsc::{self, Receiver, Sender}, thread};
use std::mem;

use cpal::{Stream, FromSample, OutputCallbackInfo, Sample, SizedSample, StreamConfig, traits::{DeviceTrait, HostTrait, StreamTrait}
};

use num::complex::Complex32;

mod signalproc;
use signalproc::{FmDemod, FirFilterDecimate, AudioState};

fn main() -> Result<(), Box<dyn Error>> {

    let mut sdr = RtlSdr::open(DeviceId::Index(0))?;

    sdr.set_center_freq(90_300_000)?;
    sdr.set_sample_rate(2_400_000)?;
    sdr.set_gain_manual(496)?;

    let reader = sdr.start_streaming()?;
    let (rtl_tx, rtl_rx) = mpsc::channel();
    let (aud_tx, aud_rx) = mpsc::channel();

    let aud: AudioState = AudioState::new(aud_rx);
    let stream = init_audio_stream::<f32>(aud);
    stream.play().unwrap();

    let readhandle = thread::spawn(move || read_rtl(reader, rtl_tx));
    let prochandle = thread::spawn(move || fm_demod(rtl_rx, aud_tx));
    
    readhandle.join().unwrap();
    prochandle.join().unwrap();

    Ok(())
}


fn read_rtl(reader: AsyncReadHandle, tx: Sender<Vec<Complex32>>) {

    while let Some(data) = reader.recv() {
        let mut samples = Vec::with_capacity(data.len() / 2);
        
        for chunk in data.chunks_exact(2) {
            let i = (chunk[0] as f32 - 128.0) / 128.0;
            let q = (chunk[1] as f32 - 128.0) / 128.0;
            samples.push(Complex32::new(i, q));
        }
        tx.send(samples).unwrap();
    }
}

fn fm_demod(rtl_rx: Receiver<Vec<Complex32>>, aud_tx: Sender<Vec<f32>>) {

    let taps = vec![0.000, 0.000, 0.001, 0.001, 0.001, 0.001, 0.001, 0.001, 0.002, 0.002, 0.002, 0.003, 0.003, 0.004, 0.004, 0.005, 0.005, 0.006, 0.007, 0.008, 0.008, 0.009, 0.010, 0.011, 0.012, 0.013, 0.013, 0.014, 0.015, 0.016, 0.016, 0.017, 0.018, 0.018, 0.019, 0.019, 0.019, 0.020, 0.020, 0.020, 0.020, 0.020, 0.020, 0.019, 0.019, 0.019, 0.018, 0.018, 0.017, 0.016, 0.016, 0.015, 0.014, 0.013, 0.013, 0.012, 0.011, 0.010, 0.009, 0.008, 0.008, 0.007, 0.006, 0.005, 0.005, 0.004, 0.004, 0.003, 0.003, 0.002, 0.002, 0.002, 0.001, 0.001, 0.001, 0.001, 0.001, 0.001, 0.000, 0.000];

    let decimate = 50;
    let block_size = 512;

    let mut lowpass = FirFilterDecimate::new(taps, decimate);
    let mut demod = FmDemod::new(Complex32::new(1.0, 0.0));

    let mut audio_block: Vec<f32> = Vec::with_capacity(block_size);

    while let Ok(block) = rtl_rx.recv() {
        for sample in block {
            let diff = demod.process(sample);
            if let Some(sample) = lowpass.process(diff){
                audio_block.push(sample);

                if audio_block.len() == block_size {
                    let full_block = mem::replace(
                        &mut audio_block,
                        Vec::with_capacity(block_size),
                    );
                    aud_tx.send(full_block).unwrap();
                }
            }
        }
    }
}

fn init_audio_stream<T>(mut aud: AudioState) -> Stream
where
    T: SizedSample + FromSample<f32>
{

    let host = cpal::default_host();
    let device = host.default_output_device().expect("No output devices available");

    let mut supported_configs_range = device.supported_output_configs()
        .expect("Error while querying configs");
    let supported_config = supported_configs_range.next()
        .expect("no supported config?!")
        .with_sample_rate(48_000);

    let err_fn = |err| eprintln!("Output audio stream error: {}", err);
    let config: StreamConfig = supported_config.into();
    let channels = config.channels as usize;

    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _: &OutputCallbackInfo| {
            let mut next_sample = || aud.get_next_sample();
            write_audio(data, channels, &mut next_sample)
        },
        err_fn,
        None,
    );
    stream.unwrap()

}

fn write_audio<T>(output: &mut [T], channels: usize, next_sample: &mut dyn FnMut() -> f32)
where 
    T: Sample + FromSample<f32>,
{
    for frame in output.chunks_mut(channels) {
        let value: T = T::from_sample(next_sample());
        for sample in frame.iter_mut() {
            *sample = value;
        }
    }
}
