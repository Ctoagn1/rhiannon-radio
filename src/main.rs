use rs_rtl::{DeviceId, RtlSdr, AsyncReadHandle};
use std::{thread, time::Duration};
use std::thread::sleep;
use std::mem;
use std::sync::{Arc, Mutex};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};

use cpal::{Stream, FromSample, OutputCallbackInfo, Sample, SizedSample, StreamConfig, traits::{DeviceTrait, HostTrait, StreamTrait}
};

use ratatui::layout::{Rect, Constraint, Layout};
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{
    Axis, BarChart, Block, Cell, Chart, Dataset, Widget, 
};

use ratatui::text::{self, Span};

use ratatui::{symbols, Frame, DefaultTerminal};

use num::complex::Complex32;

mod signalproc;
use signalproc::{FmDemod, FirFilterDecimate, AudioState, FreqShift, FftData};

use color_eyre::eyre::Result;
use crossbeam_channel::{unbounded, Receiver, Sender};


fn main() -> Result<()> {
    color_eyre::install()?;
    ratatui::run(|terminal| App::new(Duration::new(0, 20_000_000)).unwrap().run(terminal))
}



fn rtl_handler(reader: AsyncReadHandle, dsp_tx: Sender<Vec<Complex32>>, latest: Arc<Mutex<Vec<Complex32>>>) {

    while let Some(data) = reader.recv() {
        let mut samples = Vec::with_capacity(data.len() / 2);
        
        for chunk in data.chunks_exact(2) {
            let i = (chunk[0] as f32 - 128.0) / 128.0;
            let q = (chunk[1] as f32 - 128.0) / 128.0;
            samples.push(Complex32::new(i, q));
        }
        *latest.lock().unwrap() = samples.clone();
        if dsp_tx.send(samples).is_err() {return};
    }
}
fn fm_demod(rtl_rx: Receiver<Vec<Complex32>>, aud_tx: Sender<Vec<f32>>, cmd_rx: Receiver<DspCommand>) {
    //normalized cutoff of .0416, cutoff of 100KHz/2.4MHz
    //audio normalized cutoff of .0625, 15KHz/240KHz
    //made using calculatorshub.net/electrical/fir-filter-coefficient-calculator/
    let iq_taps = vec![0.000, 0.000, -0.000, -0.000, -0.000, -0.001, -0.001, -0.001, -0.001, -0.001, -0.001, -0.001, -0.001, -0.000, 0.000, 0.001, 0.002, 0.003, 0.003, 0.004, 0.004, 0.004, 0.004, 0.004, 0.002, 0.001, -0.001, -0.003, -0.006, -0.008, -0.010, -0.012, -0.013, -0.014, -0.013, -0.011, -0.007, -0.003, 0.003, 0.011, 0.019, 0.028, 0.037, 0.047, 0.056, 0.064, 0.071, 0.077, 0.081, 0.083, 0.083, 0.081, 0.077, 0.071, 0.064, 0.056, 0.047, 0.037, 0.028, 0.019, 0.011, 0.003, -0.003, -0.007, -0.011, -0.013, -0.014, -0.013, -0.012, -0.010, -0.008, -0.006, -0.003, -0.001, 0.001, 0.002, 0.004, 0.004, 0.004, 0.004, 0.004, 0.003, 0.003, 0.002, 0.001, 0.000, -0.000, -0.001, -0.001, -0.001, -0.001, -0.001, -0.001, -0.001, -0.001, -0.000, -0.000, -0.000, 0.000, 0.000];
    let aud_taps = vec![0.000, 0.000, -0.000, -0.000, -0.001, -0.001, -0.001, -0.001, -0.001, -0.000, 0.000, 0.001, 0.001, 0.002, 0.002, 0.002, 0.002, 0.001, -0.001, -0.002, -0.004, -0.005, -0.005, -0.005, -0.004, -0.001, 0.002, 0.005, 0.008, 0.010, 0.011, 0.010, 0.008, 0.003, -0.003, -0.010, -0.016, -0.022, -0.024, -0.023, -0.017, -0.007, 0.008, 0.026, 0.047, 0.068, 0.088, 0.105, 0.118, 0.124, 0.124, 0.118, 0.105, 0.088, 0.068, 0.047, 0.026, 0.008, -0.007, -0.017, -0.023, -0.024, -0.022, -0.016, -0.010, -0.003, 0.003, 0.008, 0.010, 0.011, 0.010, 0.008, 0.005, 0.002, -0.001, -0.004, -0.005, -0.005, -0.005, -0.004, -0.002, -0.001, 0.001, 0.002, 0.002, 0.002, 0.002, 0.001, 0.001, 0.000, -0.000, -0.001, -0.001, -0.001, -0.001, -0.001, -0.000, -0.000, 0.000, 0.000];

    let iq_decimate = 10;
    let aud_decimate = 5;
    let block_size = 512;

    let mut iq_lowpass = FirFilterDecimate::new(iq_taps, iq_decimate);
    let mut aud_lowpass = FirFilterDecimate::new(aud_taps, aud_decimate);
    let mut freq_shift = FreqShift::new(0.0, 2_400_000);

    let mut demod = FmDemod::new(Complex32::new(1.0, 0.0));

    let mut audio_block: Vec<f32> = Vec::with_capacity(block_size);

    while let Ok(block) = rtl_rx.recv() {
        if let Ok(cmd) = cmd_rx.try_recv(){
            match cmd {
                DspCommand::SetFreq(freq) => freq_shift.change_freq(freq),
            }        
        }
        for sample in block {
            let shift_sample = freq_shift.process(sample);
            if let Some(filt_iq) = iq_lowpass.process(shift_sample){
                let diff = demod.process(filt_iq);
                if let Some(aud_sample) = aud_lowpass.process(diff){
                    audio_block.push(aud_sample);

                    if audio_block.len() == block_size {
                        let full_block = mem::replace(
                            &mut audio_block,
                            Vec::with_capacity(block_size),
                        );
                        if aud_tx.send(full_block).is_err() {return};
                    }
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

pub struct App {
    fft: FftData,
    snapshot: Arc<Mutex<Vec<Complex32>>>,
    exit: bool,
    tick_rate: Duration,
    audio_out: Stream,
    sdr: RtlSdr,

    sdr_cmd_tx: Sender<SdrCommand>,
    dsp_cmd_tx: Sender<DspCommand>,
}

impl App {
    pub fn new(tick_rate: Duration) -> Result<Self> {
        let mut sdr = RtlSdr::open(DeviceId::Index(0))?;

        sdr.set_center_freq(90_300_000)?;
        sdr.set_sample_rate(2_400_000)?;
        sdr.set_gain_manual(496)?;

        let reader = sdr.start_streaming()?;
        let (rtl_tx, rtl_rx) = unbounded();
        let (aud_tx, aud_rx) = unbounded();

        let (sdr_cmd_tx, sdr_cmd_rx) = unbounded();
        let (dsp_cmd_tx, dsp_cmd_rx) = unbounded();

        let fft_snapshot = Arc::new(Mutex::new(Vec::new()));

        let aud: AudioState = AudioState::new(aud_rx);
        let stream = init_audio_stream::<f32>(aud);
        stream.play().unwrap();
        
        let snapclone = fft_snapshot.clone();

        thread::spawn(move || rtl_handler(reader, rtl_tx, snapclone));
        thread::spawn(move || fm_demod(rtl_rx, aud_tx, dsp_cmd_rx));
        
        let fft = FftData::new(90_300_000.0, 2_400_000, 16384);
        let app = App {
            fft,
            snapshot: fft_snapshot.clone(),
            exit: false,
            tick_rate,
            audio_out: stream,
            sdr,
            sdr_cmd_tx,
            dsp_cmd_tx,
        };

        Ok(app)
    }

    pub fn draw(&self, frame: &mut Frame) {
        
        frame.render_widget(self, frame.area());
    }

    pub fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        while !self.exit {
            terminal.draw(|frame| self.draw(frame))?;
            self.fft.update(&self.snapshot);
            self.handle_events();
            thread::sleep(self.tick_rate);
        }
        Ok(())
    }


    fn handle_events(&mut self) -> Result<()> {
        if !event::poll(Duration::from_millis(5)).is_ok_and(|x| x) {return Ok(())}
        match event::read()? {
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => {
                self.handle_key_event(key_event);
            }
            _ => {}
        };
        Ok(())
    }

    fn exit(&mut self){
        self.exit = true;
    }

    pub 

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        match key_event.code {
            KeyCode::Char('q') => self.exit(),
            KeyCode::Char('j') => {
                self.fft.tuned_freq -= 1_000.0;
                self.dsp_cmd_tx.send(DspCommand::SetFreq(self.fft.tuned_freq as f32));
            }
            KeyCode::Char('k') => {
                self.fft.tuned_freq += 1_000.0;
                self.dsp_cmd_tx.send(DspCommand::SetFreq(self.fft.tuned_freq as f32));
            }
            _ => {}
        }
    }

    fn render_spectrum(&self, area: Rect, buf: &mut Buffer) {

        let dataset = Dataset::default()
                .marker(symbols::Marker::Braille)
                .style(Style::default().fg(Color::LightMagenta))
                .data(&self.fft.data);

        let center = self.fft.center_freq;
        let half = self.fft.sample_rate as f64 / 2.0 ;

        let x_axis = Axis::default()
            .title(format_freq(self.fft.center_freq + self.fft.tuned_freq))
            .bounds([center - half, center + half])
            .labels([
                format_freq(center - half),
                format_freq(center - (half/2.0)),
                format_freq(center),
                format_freq(center + (half/2.0)), 
                format_freq(center + half),
            ]);


        let y_axis = Axis::default()
            .title("dBFS")
            .bounds([-60.0, -20.0])
            .labels(["-60", "-50", "-40", "-30", "-20"]);
        
        let block = Block::bordered().title(Span::styled("Spectrum Visualizer",
            Style::default()
            .fg(Color::LightRed)
            .add_modifier(Modifier::BOLD)
        ));

        Chart::new(vec![dataset]).block(block).x_axis(x_axis).y_axis(y_axis).render(area, buf);
        let tuned_freq = self.fft.tuned_freq + self.fft.center_freq;
        let ratio = (tuned_freq - (center-half)) / (self.fft.sample_rate as f64);
        let x = (area.width as f64 * ratio) as u16;
        
        for y in area.top()..area.bottom() {
            buf[(x, y)].set_symbol("│").set_fg(Color::Red);
        }
    }

}

impl Widget for &App{

    fn render(self, area: Rect, buf: &mut Buffer) {
        let [top, bottom] = Layout::vertical([Constraint::Ratio(75, 25); 2]).areas(area);
        let [bottom_right, bottom_left] = Layout::horizontal([Constraint::Fill(1); 2]).areas(bottom);
        self.render_spectrum(top, buf);

    }
}

fn format_freq(freq: f64) -> String {
    if freq >= 1e9 {
        format!("{:.3} GHz", freq / 1e9)
    } else if freq >= 1e6 {
        format!("{:.3} MHz", freq / 1e6)
    } else if freq >= 1e3 {
        format!("{:.3} kHz", freq / 1e3)
    } else {
        format!("{:.0} Hz", freq)
    }
}

enum SdrCommand {
    On,
    Off,
    SetFreq(u32),
    SetGain(i32)
}

enum DspCommand {
    SetFreq(f32),
}

