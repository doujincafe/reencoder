use anyhow::{Result, anyhow};
use flac_bound::{FlacEncoder, WriteWrapper};
use std::fs::File;
use symphonia::core::{
    audio::SampleBuffer,
    codecs::{DecoderOptions, CODEC_TYPE_NULL},
    errors::Error::DecodeError,
    formats::{FormatOptions, Track},
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::{Hint, ProbeResult},
};

#[derive(Debug)]
struct StreamConfig {
    channels: u32,
    bits_per_sample: Bps,
    sample_rate: u32,
    total_samples_estimate: u64,
}

#[derive(Debug)]
enum Bps {
    _16,
    _24,
    _32
}

fn get_probe(file: &std::path::Path) -> Result<ProbeResult> {
    let src = std::fs::File::open(file)?;
    let mss = MediaSourceStream::new(Box::new(src), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("flac");
    let meta_opts: MetadataOptions = Default::default();
    let fmt_opts: FormatOptions = Default::default();
    Ok(symphonia::default::get_probe().format(&hint, mss, &fmt_opts, &meta_opts)?)
}

fn read_streaminfo(track: &Track) -> Result<StreamConfig> {
    let params = &track.codec_params;

    Ok(StreamConfig {
        channels: params.channels.unwrap().count() as u32,
        bits_per_sample: match params.bits_per_sample.unwrap() {
            16 => Bps::_16,
            24 => Bps::_24,
            32 => Bps::_32,
            _ => return Err(anyhow!("invalid Bps"))
        },
        sample_rate: params.sample_rate.unwrap(),
        total_samples_estimate: params.n_frames.unwrap(),
    })
}

pub fn encode_file(file: &std::path::Path) -> Result<()> {
    let probed = get_probe(file)?;
    let mut format = probed.format;
    let dec_opts: DecoderOptions = Default::default();
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .unwrap();

    let config = read_streaminfo(track)?;

    let mut outf = File::create(format!("{}.{}", file.display(), "tmp"))?;
    let mut outw = WriteWrapper(&mut outf);
    let mut enc = FlacEncoder::new()
        .unwrap()
        .channels(config.channels)
        .bits_per_sample(match config.bits_per_sample {
            Bps::_16 => 16,
            Bps::_24 => 24,
            Bps::_32 => 32
        })
        .sample_rate(config.sample_rate)
        .total_samples_estimate(config.total_samples_estimate)
        .compression_level(8)
        .verify(true)
        .init_write(&mut outw)
        .unwrap();

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &dec_opts)
        .unwrap();
    let track_id = track.id;
    
    let mut sample_buf = None;

    loop {
        let packet = format.next_packet().unwrap();

        if packet.track_id() != track_id {
            continue;
        }

        match decoder.decode(&packet) {
            Ok(audio_buf) => {
                if sample_buf.is_none() {
                    let spec = *audio_buf.spec();

                    let duration = audio_buf.capacity() as u64;
                    sample_buf = Some(SampleBuffer::<i16>::new(duration, spec));

                    /* match config.bits_per_sample {
                        Bps::_16 => sample_buf = Some(SampleBuffer::<i16>::new(duration, spec)),
                        Bps::_24 => sample_buf = Some(SampleBuffer::<i24>::new(duration, spec)),
                        Bps::_32 => sample_buf = Some(SampleBuffer::<i32>::new(duration, spec)),
                    } */
                }

                if let Some(buf) = &mut sample_buf {
                    buf.copy_interleaved_ref(audio_buf);
                    let mut samples = Vec::new();
                    _ = buf.samples().iter().map(|sample| samples.push(*sample as i32)).collect::<Vec<_>>();
                    enc.process_interleaved(samples.as_slice(), samples.iter().len() as u32 / config.channels)
                        .unwrap();
                }
            }
            Err(DecodeError(_)) => (),
            Err(_) => break,
        }
    }

    match enc.finish() {
        Ok(_) => Ok(()),
        Err(enc) => Err(anyhow!("Encoding failed:\t{:?}", enc.state())),
    }
}
