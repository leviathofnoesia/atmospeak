//! Bounded PCM16 WAV ingress with sample-identical downmix and resampling.

use crate::decoder::SAMPLE_RATE;
use anyhow::{Context, Result, bail};
use std::{
    fs::File,
    io::{BufReader, Read, Seek},
    path::Path,
};

const FRAME_SAMPLES: usize = SAMPLE_RATE / 50;

pub struct WavInput<R> {
    reader: hound::WavReader<R>,
}

impl WavInput<BufReader<File>> {
    pub fn open(path: &Path) -> Result<Self> {
        let file =
            File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
        let length = file.metadata()?.len();
        Self::new(BufReader::with_capacity(8 * 1024, file))?
            .validate_extent(length)
            .with_context(|| format!("invalid WAV {}", path.display()))
    }
}

impl<R: Read + Seek> WavInput<R> {
    /// Check the declared payload before model startup, without reading or
    /// allocating the whole WAV. Reuse the buffer when returning to its header.
    pub fn validate_extent(mut self, source_length: u64) -> Result<Self> {
        self.reader.seek(self.reader.duration())?;
        let mut input = self.reader.into_inner();
        if input.stream_position()? > source_length {
            bail!("WAV data chunk extends past the end of the source");
        }
        input.rewind()?;
        Self::new(input)
    }
}

impl<R: Read> WavInput<R> {
    pub fn new(input: R) -> Result<Self> {
        let reader = hound::WavReader::new(input)?;
        let spec = reader.spec();
        if spec.sample_format != hound::SampleFormat::Int || spec.bits_per_sample != 16 {
            bail!("WAV must contain PCM16 audio");
        }
        if spec.channels == 0 || spec.sample_rate == 0 {
            bail!("WAV has invalid format metadata");
        }
        Ok(Self { reader })
    }

    /// Decode every declared input sample, forwarding fixed 20 ms mono frames.
    pub fn for_each_frame(mut self, mut process: impl FnMut(&[f32]) -> Result<()>) -> Result<()> {
        let spec = self.reader.spec();
        let frames = self.reader.duration() as usize;
        let channels = spec.channels as usize;
        let rate = spec.sample_rate as usize;
        let mut samples = self.reader.samples::<i16>();
        let mut output = [0.0_f32; FRAME_SAMPLES];
        let mut used = 0;
        let mut emit = |sample: f32| -> Result<()> {
            output[used] = sample;
            used += 1;
            if used == output.len() {
                process(&output)?;
                used = 0;
            }
            Ok(())
        };
        if rate == SAMPLE_RATE {
            for _ in 0..frames {
                emit(read_mono(&mut samples, channels)?)?;
            }
        } else if frames > 0 {
            let mut a = read_mono(&mut samples, channels)?;
            let mut read = 1;
            let mut b = if frames > 1 {
                read += 1;
                read_mono(&mut samples, channels)?
            } else {
                a
            };
            let mut left_index = 0;
            let output_len = ((frames as u128 * SAMPLE_RATE as u128) / rate as u128) as usize;
            for index in 0..output_len {
                let source = index as f64 * rate as f64 / SAMPLE_RATE as f64;
                let left = source.floor() as usize;
                let fraction = (source - left as f64) as f32;
                while left_index < left.min(frames - 1) {
                    a = b;
                    left_index += 1;
                    b = if read < frames {
                        read += 1;
                        read_mono(&mut samples, channels)?
                    } else {
                        a
                    };
                }
                emit(a + (b - a) * fraction)?;
            }
            // Downsampling can leave input samples that no output refers to.
            // Read them too: a truncated source must never look complete.
            for _ in read..frames {
                read_mono(&mut samples, channels)?;
            }
        }
        if used > 0 {
            process(&output[..used])?;
        }
        Ok(())
    }
}

fn read_mono(
    samples: &mut impl Iterator<Item = hound::Result<i16>>,
    channels: usize,
) -> Result<f32> {
    let mut sum = 0.0_f32;
    for _ in 0..channels {
        let sample = samples
            .next()
            .context("WAV ended before its declared sample count")??;
        sum += sample as f32 / 32768.0;
    }
    Ok(sum / channels as f32)
}
