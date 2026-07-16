use cpal::traits::{DeviceTrait, StreamTrait};

const CALLBACK_BUFFER_SIZE: u32 = 32;

pub struct CpalSetup {
    input_stream: cpal::Stream,
    output_stream: cpal::Stream,
}

impl CpalSetup {
    pub fn new(
        input_device: &cpal::Device,
        output_device: &cpal::Device,
        input_supported_config: &cpal::SupportedStreamConfig,
        output_supported_config: &cpal::SupportedStreamConfig,
        input_callback: impl FnMut(&[f32], &cpal::InputCallbackInfo) + Send + 'static,
        output_callback: impl FnMut(&mut [f32], &cpal::OutputCallbackInfo) + Send + 'static,
    ) -> Self {
        let mut input_config = input_supported_config.config();
        let mut output_config = output_supported_config.config();

        input_config.buffer_size = cpal::BufferSize::Fixed(CALLBACK_BUFFER_SIZE);
        output_config.buffer_size = cpal::BufferSize::Fixed(CALLBACK_BUFFER_SIZE);

        let input_stream = input_device
            .build_input_stream(
                input_config,
                input_callback,
                move |err| {
                    eprintln!("input stream error: {err}");
                },
                None,
            )
            .expect("failed to build input stream");

        let output_stream = output_device
            .build_output_stream(
                output_config,
                output_callback,
                move |err| {
                    eprintln!("output stream error: {err}");
                },
                None,
            )
            .expect("failed to build output stream");

        Self {
            input_stream,
            output_stream,
        }
    }

    pub fn play(&self) {
        self.input_stream
            .play()
            .expect("failed to start input audio stream");
        self.output_stream
            .play()
            .expect("failed to start output audio stream");
    }
}
