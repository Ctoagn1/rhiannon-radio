use rs_rtl::{AsyncReadHandle, DeviceId, RtlSdr};
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
    Axis, Block, Clear, Chart, Dataset, Widget, Paragraph, Borders 
};
use std::fs::OpenOptions;
use std::io::Write;


use ratatui::text::Span;

use ratatui::{symbols, Frame, DefaultTerminal};

use num::complex::Complex32;

mod signalproc;
use signalproc::{FmDemod, FirFilterDecimate, AudioState, FreqShift, FreqShiftReal, FftData, RdsSampler };

use color_eyre::eyre::Result;
use crossbeam_channel::{unbounded, Receiver, Sender};



fn main() -> Result<()> {
    color_eyre::install()?;
    ratatui::run(|terminal| App::new(Duration::new(0, 20_000_000)).unwrap().run(terminal))
}



fn rtl_handler(cmd_rx: Receiver<SdrCommand>, dsp_tx: Sender<Vec<Complex32>>, latest: Arc<Mutex<Vec<Complex32>>>) {
    let mut running = false;
    let mut sdr: Option<RtlSdr> = None;
    let mut reader: Option<AsyncReadHandle> = None;

    
    loop {
        if running{
            if let Some(rdr) = reader.as_mut() {
                if let Some(data) = rdr.recv() {
                    let mut samples = Vec::with_capacity(data.len() / 2);
                    
                    for chunk in data.chunks_exact(2) {
                        let i = (chunk[0] as f32 - 128.0) / 128.0;
                        let q = (chunk[1] as f32 - 128.0) / 128.0;
                        samples.push(Complex32::new(i, q));
                    }
                    *latest.lock().unwrap() = samples.clone();
                    if dsp_tx.send(samples).is_err() {return};
                }
                else{
                    reader = None;
                    sdr = None;
                    running = false;
                }
            }
        }



        let cmd = cmd_rx.try_recv();
        match cmd {
            Err(_) => {
                if !running {sleep(Duration::from_millis(1))};

            }
            Ok(SdrCommand::Off) => {
                running = false;
            }
            Ok(SdrCommand::On) => {
                if reader.is_none() {
                   if let Ok(mut usdr) = RtlSdr::open(DeviceId::Index(0)) {
                        usdr.set_center_freq(90_300_000).unwrap();
                        usdr.set_sample_rate(2_400_000).unwrap();
                        usdr.set_gain_manual(496).unwrap();

                        let r = usdr.start_streaming().unwrap();

                        reader = Some(r);
                        sdr = Some(usdr);
                    }
                }

                if reader.is_some() {
                    running = true;
                }

            }

            Ok(SdrCommand::SetTuneFreq(freq)) => {
                if let Some(sdr) = sdr.as_mut() {
                    sdr.set_center_freq(freq);
                }
            }
            Ok(SdrCommand::SetGain(gain)) => {
                if let Some(sdr) = sdr.as_mut() {
                    sdr.set_gain_manual(gain);
                }
            }

        }

    }
}

fn rds_handler(signal_rx: Receiver<Vec<f32>>) {


    let mut sampler = RdsSampler::new(12_000.0);
    let mut shift = FreqShiftReal::new(57_000.0, 240_000);
    //0.05 normalized cutoff, 60 -> 3
    let rds_taps_3 = vec![0.000, 0.000, 0.000, 0.001, 0.001, 0.001, 0.001, 0.001, 0.000, 0.000, -0.000, -0.001, -0.001, -0.002, -0.002, -0.002, -0.003, -0.002, -0.002, -0.001, 0.001, 0.002, 0.004, 0.005, 0.006, 0.007, 0.007, 0.006, 0.004, 0.002, -0.002, -0.006, -0.010, -0.013, -0.016, -0.018, -0.018, -0.016, -0.011, -0.004, 0.005, 0.016, 0.028, 0.042, 0.056, 0.069, 0.080, 0.090, 0.096, 0.100, 0.100, 0.096, 0.090, 0.080, 0.069, 0.056, 0.042, 0.028, 0.016, 0.005, -0.004, -0.011, -0.016, -0.018, -0.018, -0.016, -0.013, -0.010, -0.006, -0.002, 0.002, 0.004, 0.006, 0.007, 0.007, 0.006, 0.005, 0.004, 0.002, 0.001, -0.001, -0.002, -0.002, -0.003, -0.002, -0.002, -0.002, -0.001, -0.001, -0.000, 0.000, 0.000, 0.001, 0.001, 0.001, 0.001, 0.001, 0.000, 0.000, 0.000];
    //cutoff of 30 kHz, .125 normalized
    let rds_taps_60 = vec![0.000, 0.000, -0.000, -0.001, -0.001, -0.000, 0.000, 0.001, 0.001, 0.000, -0.001, -0.001, -0.002, -0.001, 0.001, 0.002, 0.003, 0.001, -0.001, -0.004, -0.004, -0.002, 0.002, 0.005, 0.006, 0.003, -0.003, -0.008, -0.009, -0.004, 0.004, 0.011, 0.013, 0.006, -0.006, -0.017, -0.018, -0.008, 0.009, 0.025, 0.028, 0.013, -0.015, -0.043, -0.052, -0.027, 0.034, 0.117, 0.196, 0.244, 0.244, 0.196, 0.117, 0.034, -0.027, -0.052, -0.043, -0.015, 0.013, 0.028, 0.025, 0.009, -0.008, -0.018, -0.017, -0.006, 0.006, 0.013, 0.011, 0.004, -0.004, -0.009, -0.008, -0.003, 0.003, 0.006, 0.005, 0.002, -0.002, -0.004, -0.004, -0.001, 0.001, 0.003, 0.002, 0.001, -0.001, -0.002, -0.001, -0.001, 0.000, 0.001, 0.001, 0.000, -0.000, -0.001, -0.001, -0.000, 0.000, 0.000];
    let mut rds_filter_3 = FirFilterDecimate::<f32>::new(rds_taps_3, 5);
    let mut rds_filter_60 = FirFilterDecimate::new(rds_taps_60, 4);

    while let Ok(signal_block) = signal_rx.recv() {
        let shifted_signal_block = shift.process(signal_block);
        let reduced_signal_block = rds_filter_60.process(shifted_signal_block);
        let filtered_signal = rds_filter_3.process(reduced_signal_block);

        let bin_data = sampler.process(filtered_signal);
        let diff_bin_data = sampler.diff_demod(bin_data);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open("rds_bits.log")
            .unwrap();
        for i in &diff_bin_data{
            write!(file, "{}", if *i {1} else {0}).unwrap();
        }
        

    }
}

fn fm_demod(rtl_rx: Receiver<Vec<Complex32>>, aud_tx: Sender<Vec<f32>>, cmd_rx: Receiver<DspCommand>, rds_tx: Sender<Vec<f32>>) {
    //normalized cutoff of .0416, cutoff of 100KHz/2.4MHz
    //audio normalized cutoff of .0625, 15KHz/240KHz
    //made using calculatorshub.net/electrical/fir-filter-coefficient-calculator/
    let iq_taps = vec![0.000, 0.000, -0.000, -0.000, -0.000, -0.001, -0.001, -0.001, -0.001, -0.001, -0.001, -0.001, -0.001, -0.000, 0.000, 0.001, 0.002, 0.003, 0.003, 0.004, 0.004, 0.004, 0.004, 0.004, 0.002, 0.001, -0.001, -0.003, -0.006, -0.008, -0.010, -0.012, -0.013, -0.014, -0.013, -0.011, -0.007, -0.003, 0.003, 0.011, 0.019, 0.028, 0.037, 0.047, 0.056, 0.064, 0.071, 0.077, 0.081, 0.083, 0.083, 0.081, 0.077, 0.071, 0.064, 0.056, 0.047, 0.037, 0.028, 0.019, 0.011, 0.003, -0.003, -0.007, -0.011, -0.013, -0.014, -0.013, -0.012, -0.010, -0.008, -0.006, -0.003, -0.001, 0.001, 0.002, 0.004, 0.004, 0.004, 0.004, 0.004, 0.003, 0.003, 0.002, 0.001, 0.000, -0.000, -0.001, -0.001, -0.001, -0.001, -0.001, -0.001, -0.001, -0.001, -0.000, -0.000, -0.000, 0.000, 0.000];
    let aud_taps = vec![0.000, 0.000, -0.000, -0.000, -0.001, -0.001, -0.001, -0.001, -0.001, -0.000, 0.000, 0.001, 0.001, 0.002, 0.002, 0.002, 0.002, 0.001, -0.001, -0.002, -0.004, -0.005, -0.005, -0.005, -0.004, -0.001, 0.002, 0.005, 0.008, 0.010, 0.011, 0.010, 0.008, 0.003, -0.003, -0.010, -0.016, -0.022, -0.024, -0.023, -0.017, -0.007, 0.008, 0.026, 0.047, 0.068, 0.088, 0.105, 0.118, 0.124, 0.124, 0.118, 0.105, 0.088, 0.068, 0.047, 0.026, 0.008, -0.007, -0.017, -0.023, -0.024, -0.022, -0.016, -0.010, -0.003, 0.003, 0.008, 0.010, 0.011, 0.010, 0.008, 0.005, 0.002, -0.001, -0.004, -0.005, -0.005, -0.005, -0.004, -0.002, -0.001, 0.001, 0.002, 0.002, 0.002, 0.002, 0.001, 0.001, 0.000, -0.000, -0.001, -0.001, -0.001, -0.001, -0.001, -0.000, -0.000, 0.000, 0.000];
    let iq_decimate = 10;
    let aud_decimate = 5;
    let block_size = 512;
    let mut rds_on = false;

    let mut iq_lowpass = FirFilterDecimate::new(iq_taps, iq_decimate);
    let mut aud_lowpass = FirFilterDecimate::new(aud_taps, aud_decimate);

    let mut freq_shift = FreqShift::new(0.0, 2_400_000);
    let mut demod = FmDemod::new(Complex32::new(1.0, 0.0));


    while let Ok(block) = rtl_rx.recv() {
        if let Ok(cmd) = cmd_rx.try_recv(){
            match cmd {
                DspCommand::SetTuneFreq(freq) => freq_shift.change_freq(freq),
                DspCommand::RdsOn => {rds_on = true;},
                DspCommand::RdsOff => {rds_on = false;},
            }        
        }
        let shifted_samples = freq_shift.process(block);
        let filtered_iq = iq_lowpass.process(shifted_samples);
        let demod_samples = demod.process(filtered_iq);
        if rds_on {
            rds_tx.send(demod_samples.clone());
        }
        let aud_samples = aud_lowpass.process(demod_samples);
        aud_tx.send(aud_samples);

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

    sdr_cmd_tx: Sender<SdrCommand>,
    dsp_cmd_tx: Sender<DspCommand>,
    input_mode: InputMode
}

impl App {
    pub fn new(tick_rate: Duration) -> Result<Self> {

        let (rtl_tx, rtl_rx) = unbounded();
        let (aud_tx, aud_rx) = unbounded();

        let (sdr_cmd_tx, sdr_cmd_rx) = unbounded();
        let (dsp_cmd_tx, dsp_cmd_rx) = unbounded();
        let (rds_tx, rdx_rs) = unbounded();
        let fft_snapshot = Arc::new(Mutex::new(Vec::new()));

        let aud: AudioState = AudioState::new(aud_rx);
        let stream = init_audio_stream::<f32>(aud);
        stream.play().unwrap();
        
        let snapclone = fft_snapshot.clone();

        thread::spawn(move || rtl_handler(sdr_cmd_rx, rtl_tx, snapclone));
        thread::spawn(move || fm_demod(rtl_rx, aud_tx, dsp_cmd_rx, rds_tx));
        thread::spawn(move || rds_handler(rdx_rs));
        let fft = FftData::new(90_300_000.0, 2_400_000, 16384);
        let app = App {
            fft,
            snapshot: fft_snapshot.clone(),
            exit: false,
            tick_rate,
            audio_out: stream,
            sdr_cmd_tx,
            dsp_cmd_tx,
            input_mode: InputMode::NormalMode
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

    fn handle_key_event(&mut self, key_event: KeyEvent) {
        match self.input_mode {
            InputMode::NormalMode => {
                match key_event.code {
                    KeyCode::Char('q') => self.exit(),
                    KeyCode::Char('j') => {
                        self.fft.freq_offset -= 1_000.0;
                        self.dsp_cmd_tx.send(DspCommand::SetTuneFreq(self.fft.freq_offset as f32));
                        self.check_recenter();
                    }
                    KeyCode::Char('k') => {
                        self.fft.freq_offset += 1_000.0;
                        self.dsp_cmd_tx.send(DspCommand::SetTuneFreq(self.fft.freq_offset as f32));
                        self.check_recenter();
                    }
                    KeyCode::Char('c') => {
                        self.sdr_cmd_tx.send(SdrCommand::On);
                        self.sdr_cmd_tx.send(SdrCommand::SetTuneFreq(self.fft.center_freq as u32));
                    }
                    KeyCode::Char('d') => {
                        self.sdr_cmd_tx.send(SdrCommand::Off);
                    }
                    KeyCode::Char('t') => {
                        self.input_mode = InputMode::FreqInput(String::new())
                    }
                    _ => {}
                }
            }
            InputMode::FreqInput(ref mut buffer) => {
                match key_event.code {
                    KeyCode::Char(c) if c.is_ascii_digit() => {
                        buffer.push(c);
                     }

                    KeyCode::Char('.') => {
                        if !buffer.contains('.') {
                            buffer.push('.');
                        }
                    }
                    KeyCode::Backspace => {
                        buffer.pop();
                    }
                    KeyCode::Esc => {
                        self.input_mode = InputMode::NormalMode;
                    }
                    KeyCode::Char('t') => {
                        self.input_mode = InputMode::NormalMode;
                    }

                    KeyCode::Enter => {
                        if let Ok(freq) = parse_frequency(&buffer) {
                            self.sdr_cmd_tx.send(SdrCommand::SetTuneFreq(freq as u32));
                            self.dsp_cmd_tx.send(DspCommand::SetTuneFreq(0.0));
                            self.fft.center_freq = freq;
                            self.fft.freq_offset = 0.0;
                        }

                        

                        self.input_mode = InputMode::NormalMode;
                    }
                    _ => {}
                }
            }
        }
    }

    fn check_recenter(&mut self) {
        let span = self.fft.sample_rate as f64 / 4.0;
        if self.fft.freq_offset > span {
            self.fft.center_freq += span;     
            self.fft.freq_offset -= span;

            self.sdr_cmd_tx.send(SdrCommand::SetTuneFreq(self.fft.center_freq as u32));
            self.dsp_cmd_tx.send(DspCommand::SetTuneFreq(self.fft.freq_offset as f32));
        }

        if self.fft.freq_offset < -span {
            self.fft.center_freq -= span;    
            self.fft.freq_offset += span;

            self.sdr_cmd_tx.send(SdrCommand::SetTuneFreq(self.fft.center_freq as u32));
            self.dsp_cmd_tx.send(DspCommand::SetTuneFreq(self.fft.freq_offset as f32));
        }

        
    }

    fn render_command_box(&self, area: Rect, buf: &mut Buffer) {
        let p = Paragraph::new(
            "
            Connect: <c>
            Disconnect: <d>
            Tune Down <j>
            Tune Up <k>
            Set Frequency <t>
            Quit <q>"
        )
        .style(Style::default().fg(Color::White))
        .block(Block::default()
            .borders(Borders::ALL)
            .title("Commands")
        )
        .render(area, buf);

        
    }
    fn render_popup(&self, area: Rect, buf: &mut Buffer, freq_str: &String) {
        let text = vec![
            freq_str.as_str().into(),
            "ESC to quit, Enter to set".into(),
            "Accepts decimals as MHz, i.e 90.1 = 90100000".into(),
        ];

        Clear.render(area, buf);
        Paragraph::new(text).block(Block::bordered().title("Set Frequency").style(Color::LightRed)).render(area, buf);
        
    }

    fn render_spectrum(&self, area: Rect, buf: &mut Buffer) {

        let dataset = Dataset::default()
                .marker(symbols::Marker::Braille)
                .style(Style::default().fg(Color::LightMagenta))
                .data(&self.fft.data);

        let center = self.fft.center_freq;
        let half = self.fft.sample_rate as f64 / 2.0 ;

        let x_axis = Axis::default()
            .title(format_freq(self.fft.center_freq + self.fft.freq_offset))
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
        let tuned_freq = self.fft.freq_offset + self.fft.center_freq;
        let ratio = (tuned_freq - (center-half)) / (self.fft.sample_rate as f64);
        let x = (area.width as f64 * ratio) as u16 + area.width / 15 ; // area.width / 15 for offset of chart in buffer
        
        
        for y in area.top()..(area.bottom() - (area.bottom() - area.top()) / 4) {
            buf[(x, y)].set_symbol("│").set_fg(Color::Red);
        }
    }

}

impl Widget for &App{

    fn render(self, area: Rect, buf: &mut Buffer) {
        let [top, bottom] = Layout::vertical([Constraint::Ratio(75, 25); 2]).areas(area);
        let [bottom_right, bottom_left] = Layout::horizontal([Constraint::Fill(1); 2]).areas(bottom);
        let centered_area = area.centered(Constraint::Percentage(60), Constraint::Percentage(20));

        self.render_spectrum(top, buf);
        self.render_command_box(bottom_right, buf);
        if let InputMode::FreqInput(freq_buf) = &self.input_mode {
            self.render_popup(centered_area, buf, freq_buf);
        }

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
    SetTuneFreq(u32),
    SetGain(i32)
}

enum DspCommand {
    SetTuneFreq(f32),
    RdsOn,
    RdsOff,
}

enum InputMode {
    NormalMode,
    FreqInput(String),
}

fn parse_frequency(freq: &String) -> Result<f64> {
    let num_freq: f64;
    if freq.contains('.'){
        let mfreq = freq.parse::<f64>()?;
        if mfreq < 1000.0 {
            num_freq = mfreq * 1_000_000.0;
        } else {
            num_freq = mfreq;
        }
    }
    else {
        let nfreq = freq.parse::<u32>()?;
        num_freq = nfreq as f64;
    }
    Ok(num_freq)
}
