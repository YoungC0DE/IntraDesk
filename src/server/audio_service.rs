// both soundio and cpal use wasapi on windows and coreaudio on mac, they do not support loopback.
// libpulseaudio support loopback because pulseaudio is a standalone audio service with some
// configuration, but need to install the library and start the service on OS, not a good choice.
// windows: https://docs.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording
// mac: https://github.com/mattingalls/Soundflower
// https://docs.microsoft.com/en-us/windows/win32/api/audioclient/nn-audioclient-iaudioclient
// https://github.com/ExistentialAudio/BlackHole

// if pactl not work, please run
// sudo apt-get --purge --reinstall install pulseaudio
// https://askubuntu.com/questions/403416/how-to-listen-live-sounds-from-input-from-external-sound-card
// https://wiki.debian.org/audio-loopback
// https://github.com/krruzic/pulsectl

use super::*;
use hbb_common::anyhow::anyhow;
use magnum_opus::{Channels::*, Encoder};
use std::sync::atomic::{AtomicBool, Ordering};

pub const NAME: &'static str = "audio";
pub const AUDIO_DATA_SIZE_U8: usize = 960 * 4; // 10ms in 48000 stereo
static RESTARTING: AtomicBool = AtomicBool::new(false);

pub fn new() -> GenericService {
    let svc = EmptyExtraFieldService::new(NAME.to_owned(), true);
    GenericService::repeat::<cpal_impl::State, _, _>(&svc.clone(), 33, cpal_impl::run);
    svc.sp
}


#[inline]
fn get_audio_input() -> String {
    Config::get_option("audio-input")
}

pub fn restart() {
    log::info!("restart the audio service, freezing now...");
    if RESTARTING.load(Ordering::SeqCst) {
        return;
    }
    RESTARTING.store(true, Ordering::SeqCst);
}



mod audio_capture;
mod audio_capture_error;
mod audio_capture_queue;

mod cpal_impl {
    use self::service::{Reset, ServiceSwap};
    use super::audio_capture_error::CaptureErrorHandler;
    use super::*;
    use cpal::{
        traits::{DeviceTrait, HostTrait, StreamTrait},
        Device, Host, InputCallbackInfo, SupportedStreamConfig,
    };

    lazy_static::lazy_static! {
        static ref HOST: Host = cpal::default_host();
    }

    const AUDIO_PACKETS_PER_SECOND: usize = 100;


    #[derive(Default)]
    pub struct State {
        stream: Option<ActiveCaptureStream>,
    }

    struct ActiveCaptureStream {
        stream: Option<Box<dyn StreamTrait>>,
        format: Arc<Message>,
        _encoder_worker: audio_capture_queue::CaptureEncoderWorker,
        errors: CaptureErrorHandler,
    }

    impl Drop for ActiveCaptureStream {
        fn drop(&mut self) {
            self.stream.take();
        }
    }

    impl super::service::Reset for State {
        fn reset(&mut self) {
            self.stream.take();
        }
    }

    fn run_restart(sp: EmptyExtraFieldService, state: &mut State) -> ResultType<()> {
        state.reset();
        sp.snapshot(|_sps: ServiceSwap<_>| Ok(()))?;
        match &state.stream {
            None => {
                state.stream = Some(play(&sp)?);
            }
            _ => {}
        }
        if let Some(stream) = &state.stream {
            sp.send_shared(stream.format.clone());
        }
        RESTARTING.store(false, Ordering::SeqCst);
        Ok(())
    }

    fn run_serv_snapshot(sp: EmptyExtraFieldService, state: &mut State) -> ResultType<()> {
        sp.snapshot(|sps| {
            match &state.stream {
                None => {
                    state.stream = Some(play(&sp)?);
                }
                _ => {}
            }
            if let Some(stream) = &state.stream {
                sps.send_shared(stream.format.clone());
            }
            Ok(())
        })?;
        Ok(())
    }

    pub fn run(sp: EmptyExtraFieldService, state: &mut State) -> ResultType<()> {
        if let Some(stream) = &state.stream {
            if stream.errors.needs_restart() {
                // Recreate on the service thread, outside the capture callbacks.
                log::warn!("Recreating audio capture stream after an error");
                super::restart();
            }
        }
        if !RESTARTING.load(Ordering::SeqCst) {
            run_serv_snapshot(sp, state)
        } else {
            run_restart(sp, state)
        }
    }

    #[derive(Clone, Copy)]
    struct CaptureFrameProcessorConfig {
        input_rate: u32,
        output_rate: u32,
        device_channel: u16,
        encode_channel: u16,
    }

    struct CaptureFrameProcessor {
        config: CaptureFrameProcessorConfig,
        resampler: Option<crate::audio_resampler::FixedFrameAudioResampler>,
        sender: audio_capture_queue::CapturePcmSender,
        rechannel_buffer: Vec<f32>,
    }

    struct CaptureStreamOutput {
        sender: audio_capture_queue::CapturePcmSender,
        sample_rate: u32,
        encode_channel: magnum_opus::Channels,
    }

    impl CaptureFrameProcessor {
        fn new(
            config: CaptureFrameProcessorConfig,
            sender: audio_capture_queue::CapturePcmSender,
        ) -> ResultType<Self> {
            let resampler = if config.input_rate == config.output_rate {
                None
            } else {
                let output_frames = config.output_rate as usize / AUDIO_PACKETS_PER_SECOND;
                Some(crate::audio_resampler::FixedFrameAudioResampler::new(
                    crate::audio_resampler::AudioResamplerConfig {
                        input_rate: config.input_rate,
                        output_rate: config.output_rate,
                        channels: config.device_channel,
                    },
                    output_frames,
                )?)
            };
            Ok(Self {
                config,
                resampler,
                sender,
                rechannel_buffer: Vec::with_capacity(
                    capture_packet_layout(config.output_rate, config.encode_channel)?.1,
                ),
            })
        }

        fn process(&mut self, data: &[f32]) -> ResultType<()> {
            let config = self.config;
            let sender = &mut self.sender;
            let rechannel_buffer = &mut self.rechannel_buffer;
            let mut send_packet = |packet: &[f32]| {
                let packet =
                    audio_capture::rechannel(packet, config.device_channel, rechannel_buffer);
                sender.submit(packet);
            };
            if let Some(resampler) = self.resampler.as_mut() {
                resampler.process_with(data, send_packet).with_context(|| {
                    format!(
                        "Failed to resample captured audio from {} Hz to {} Hz",
                        config.input_rate, config.output_rate
                    )
                })?;
            } else {
                send_packet(data);
            }
            Ok(())
        }
    }

    fn capture_packet_layout(sample_rate: u32, channels: u16) -> ResultType<(usize, usize)> {
        if sample_rate < AUDIO_PACKETS_PER_SECOND as u32 || channels == 0 {
            bail!("Invalid audio capture layout: sample_rate={sample_rate}, channels={channels}");
        }
        let frames = sample_rate as usize / AUDIO_PACKETS_PER_SECOND;
        let samples = frames.checked_mul(channels as usize).with_context(|| {
            format!(
                "Audio capture frame size overflow: sample_rate={sample_rate}, channels={channels}"
            )
        })?;
        Ok((frames, samples))
    }


    fn get_device() -> ResultType<(Device, SupportedStreamConfig)> {
        let audio_input = super::get_audio_input();
        if !audio_input.is_empty() {
            return get_audio_input(&audio_input);
        }
        // The pinned CPAL uses event-driven WASAPI loopback here. Windows versions
        // before Windows 10 1703 do not signal capture events, so system audio does
        // not work on Win7. #16095 kept the same CPAL revision and loopback path;
        // this limitation predates that PR.
        // Ordinary microphone input is supported on Win7 and uses the branch above.
        // #16095 added its callback-to-encoder wake dependency; see CapturePcmSender::wake
        // for the new scheduling risk, whose audible impact on Win7 is unmeasured.
        // https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording
        // https://learn.microsoft.com/en-us/windows/win32/coreaudio/capturesharedeventdriven
        let device = HOST
            .default_output_device()
            .with_context(|| "Failed to get default output device for loopback")?;
        log::info!(
            "Default output device: {}",
            device.name().unwrap_or("".to_owned())
        );
        let format = device
            .default_output_config()
            .map_err(|e| anyhow!(e))
            .with_context(|| "Failed to get default output format")?;
        log::info!("Default output format: {:?}", format);
        Ok((device, format))
    }


    fn get_audio_input(audio_input: &str) -> ResultType<(Device, SupportedStreamConfig)> {
        let mut device = None;
        if device.is_none() && !audio_input.is_empty() {
            for d in HOST
                .devices()
                .with_context(|| "Failed to get audio devices")?
            {
                if d.name().unwrap_or("".to_owned()) == audio_input {
                    device = Some(d);
                    break;
                }
            }
        }
        let device = device.unwrap_or(
            HOST.default_input_device()
                .with_context(|| "Failed to get default input device for loopback")?,
        );
        log::info!("Input device: {}", device.name().unwrap_or("".to_owned()));
        let format = device
            .default_input_config()
            .map_err(|e| anyhow!(e))
            .with_context(|| "Failed to get default input format")?;
        log::info!("Default input format: {:?}", format);
        Ok((device, format))
    }

    fn play(sp: &GenericService) -> ResultType<ActiveCaptureStream> {
        use cpal::SampleFormat::*;
        let (device, config) = get_device()?;
        let sp = sp.clone();
        // Sample rate must be one of 8000, 12000, 16000, 24000, or 48000.
        let sample_rate_0 = config.sample_rate().0;
        let sample_rate = if sample_rate_0 < 12000 {
            8000
        } else if sample_rate_0 < 16000 {
            12000
        } else if sample_rate_0 < 24000 {
            16000
        } else if sample_rate_0 < 48000 {
            24000
        } else {
            48000
        };
        let ch = if config.channels() > 1 { Stereo } else { Mono };
        let max_channels = config.channels().max(ch as u16);
        let (_, max_packet_samples) = capture_packet_layout(sample_rate, max_channels)?;
        let encoder_config = audio_capture_queue::CaptureEncoderConfig {
            sample_rate,
            encode_channel: ch,
            max_packet_samples,
        };
        let (sender, encoder_worker) =
            audio_capture_queue::start_capture_encoder(encoder_config, sp)?;
        let output = CaptureStreamOutput {
            sender,
            sample_rate,
            encode_channel: ch,
        };
        let (stream, errors) = match config.sample_format() {
            I8 => build_input_stream::<i8>(device, &config, output)?,
            I16 => build_input_stream::<i16>(device, &config, output)?,
            I32 => build_input_stream::<i32>(device, &config, output)?,
            I64 => build_input_stream::<i64>(device, &config, output)?,
            U8 => build_input_stream::<u8>(device, &config, output)?,
            U16 => build_input_stream::<u16>(device, &config, output)?,
            U32 => build_input_stream::<u32>(device, &config, output)?,
            U64 => build_input_stream::<u64>(device, &config, output)?,
            F32 => build_input_stream::<f32>(device, &config, output)?,
            F64 => build_input_stream::<f64>(device, &config, output)?,
            f => bail!("unsupported audio format: {:?}", f),
        };
        stream.play()?;
        Ok(ActiveCaptureStream {
            stream: Some(Box::new(stream)),
            format: Arc::new(create_format_msg(sample_rate, ch as _)),
            _encoder_worker: encoder_worker,
            errors,
        })
    }

    fn convert_input_samples<T>(data: &[T]) -> impl Iterator<Item = f32> + '_
    where
        T: cpal::SizedSample,
        f32: cpal::FromSample<T>,
    {
        data.iter()
            .map(|sample| <f32 as cpal::FromSample<T>>::from_sample_(*sample))
    }


    fn build_input_stream<T>(
        device: cpal::Device,
        config: &cpal::SupportedStreamConfig,
        output: CaptureStreamOutput,
    ) -> ResultType<(cpal::Stream, CaptureErrorHandler)>
    where
        T: cpal::SizedSample,
        f32: cpal::FromSample<T>,
    {
        let errors = CaptureErrorHandler::default();
        let callback_errors = errors.clone();
        let err_fn = move |err| callback_errors.handle(err);
        let processor_errors = errors.clone();
        let sample_rate_0 = config.sample_rate().0;
        log::debug!("Audio sample rate : {}", output.sample_rate);
        let device_channel = config.channels();
        let (_, capture_frame_samples) = capture_packet_layout(sample_rate_0, device_channel)?;
        let mut frame = audio_capture::CaptureFrameBuffer::new(capture_frame_samples)?;
        let processor_config = CaptureFrameProcessorConfig {
            input_rate: sample_rate_0,
            output_rate: output.sample_rate,
            device_channel,
            encode_channel: output.encode_channel as _,
        };
        let mut processor = CaptureFrameProcessor::new(processor_config, output.sender)?;
        let timeout = None;
        let stream = device.build_input_stream(
            &config.config(),
            move |data: &[T], _: &InputCallbackInfo| {
                if processor_errors.needs_restart() {
                    return;
                }
                frame.process(convert_input_samples(data), |frame| {
                    processor_errors.process_frame(|| processor.process(frame));
                });
            },
            err_fn,
            timeout,
        )?;
        Ok((stream, errors))
    }
}

fn create_format_msg(sample_rate: u32, channels: u16) -> Message {
    let format = AudioFormat {
        sample_rate,
        channels: channels as _,
        ..Default::default()
    };
    let mut misc = Misc::new();
    misc.set_audio_format(format);
    let mut msg = Message::new();
    msg.set_misc(misc);
    msg
}

// Use a per-encoder counter for the Noise(Zero) Gate Attack Time.
// every audio data length is set to 480
// MAX_AUDIO_ZERO_COUNT=800 is similar as Gate Attack Time 3~5s(Linux) || 6~8s(Windows)
const MAX_AUDIO_ZERO_COUNT: u16 = 800;

struct AudioEncoder {
    encoder: Encoder,
    zero_count: u16,
}

impl AudioEncoder {
    fn new(encoder: Encoder) -> Self {
        Self {
            encoder,
            zero_count: 0,
        }
    }

    fn should_encode(&mut self, data: &[f32]) -> bool {
        if data.iter().filter(|x| **x != 0.).next().is_some() {
            self.zero_count = 0;
        } else if self.zero_count > MAX_AUDIO_ZERO_COUNT {
            if self.zero_count == MAX_AUDIO_ZERO_COUNT + 1 {
                log::debug!("Audio Zero Gate Attack");
                self.zero_count += 1;
            }
            return false;
        } else {
            self.zero_count += 1;
        }
        true
    }
}

fn send_f32(data: &[f32], encoder: &mut AudioEncoder, sp: &GenericService) {
    if !encoder.should_encode(data) {
        return;
    }

    match encoder.encoder.encode_vec_float(data, data.len() * 6) {
        Ok(data) => {
            let mut msg_out = Message::new();
            msg_out.set_audio_frame(AudioFrame {
                data: data.into(),
                ..Default::default()
            });
            sp.send(msg_out);
        }
        Err(error) => log::warn!("Failed to encode audio frame: {error:?}"),
    }
}
