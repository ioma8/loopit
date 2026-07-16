use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

const CALLBACK_BUFFER_SIZE: u32 = 32;

pub struct Audio {
    input_stream: Option<cpal::Stream>,
    output_stream: Option<cpal::Stream>,
    input_device: cpal::Device,
    output_device: cpal::Device,
    pub input_config: cpal::StreamConfig,
    pub output_config: cpal::StreamConfig,
}

impl Audio {
    pub fn new() -> Self {
        let host = cpal::default_host();

        let input_device = host
            .default_input_device()
            .expect("no input device available");

        let output_device = host
            .default_output_device()
            .expect("no output device available");

        let input_supported_config = input_device
            .default_input_config()
            .expect("failed to get default input config");

        let output_supported_config = output_device
            .default_output_config()
            .expect("failed to get default output config");

        println!("Input config: {:?}", input_supported_config);
        println!("Output config: {:?}", output_supported_config);

        if input_supported_config.sample_format() != cpal::SampleFormat::F32
            || output_supported_config.sample_format() != cpal::SampleFormat::F32
        {
            panic!(
                "this example expects f32 input/output, got input={:?}, output={:?}",
                input_supported_config.sample_format(),
                output_supported_config.sample_format()
            );
        }

        let mut input_config = input_supported_config.config();
        let mut output_config = output_supported_config.config();

        input_config.buffer_size = cpal::BufferSize::Fixed(CALLBACK_BUFFER_SIZE);
        output_config.buffer_size = cpal::BufferSize::Fixed(CALLBACK_BUFFER_SIZE);

        Self {
            input_stream: None,
            output_stream: None,
            input_device,
            output_device,
            input_config,
            output_config,
        }
    }

    pub fn set_callbacks(
        &mut self,
        input_callback: impl FnMut(&[f32], &cpal::InputCallbackInfo) + Send + 'static,
        output_callback: impl FnMut(&mut [f32], &cpal::OutputCallbackInfo) + Send + 'static,
    ) {
        let input_config = self.input_config.clone();
        let output_config = self.output_config.clone();

        self.input_stream = Some(
            self.input_device
                .build_input_stream(
                    input_config,
                    input_callback,
                    move |err| {
                        eprintln!("input stream error: {err}");
                    },
                    None,
                )
                .expect("failed to build input stream"),
        );

        self.output_stream = Some(
            self.output_device
                .build_output_stream(
                    output_config,
                    output_callback,
                    move |err| {
                        eprintln!("output stream error: {err}");
                    },
                    None,
                )
                .expect("failed to build output stream"),
        );
    }

    pub fn play(&self) {
        self.input_stream
            .as_ref()
            .expect("input stream is not initialized")
            .play()
            .expect("failed to start input audio stream");
        self.output_stream
            .as_ref()
            .expect("output stream is not initialized")
            .play()
            .expect("failed to start output audio stream");
    }
}
