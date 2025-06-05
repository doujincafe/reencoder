use anyhow::{Result, anyhow};
use claxon::FlacReader;
use flac_bound::{FlacEncoder, WriteWrapper};
use i24::i24;
use md5::{Digest, Md5};
use metaflac::{Block, Tag};
use std::fs::File;

struct StreamConfig {
    channels: u32,
    bits_per_sample: Bps,
    sample_rate: u32,
    total_samples_estimate: u64,
}

enum Bps {
    _16,
    _24,
    _32,
}

impl Bps {
    fn new(num: u32) -> Result<Self> {
        match num {
            16 => Ok(Bps::_16),
            24 => Ok(Bps::_24),
            32 => Ok(Bps::_32),
            _ => Err(anyhow!("Invalid BPS")),
        }
    }

    fn value(&self) -> u32 {
        match self {
            Bps::_16 => 16,
            Bps::_24 => 24,
            Bps::_32 => 32,
        }
    }
}

fn process_samples_i16(
    hasher: &mut impl Digest,
    mut reader: FlacReader<File>,
    enc: &mut FlacEncoder,
    config: &StreamConfig,
) -> Result<()> {
    for samples in reader
        .samples()
        .map(|sample| sample.unwrap())
        .collect::<Vec<_>>()
        .chunks(4096 * usize::try_from(config.channels).unwrap())
    {
        enc.process_interleaved(samples, u32::try_from(samples.len()).unwrap() / config.channels)
            .unwrap();
        let _ = samples
            .iter()
            .map(|sample| hasher.update((i16::try_from(*sample)).unwrap().to_le_bytes()))
            .collect::<Vec<_>>();
    }
    Ok(())
}

fn process_samples_i24(
    hasher: &mut impl Digest,
    mut reader: FlacReader<File>,
    enc: &mut FlacEncoder,
    config: &StreamConfig,
) -> Result<()> {
    for samples in reader
        .samples()
        .map(|sample| sample.unwrap())
        .collect::<Vec<_>>()
        .chunks(4096 * usize::try_from(config.channels).unwrap())
    {
        enc.process_interleaved(samples, u32::try_from(samples.len()).unwrap() / config.channels)
            .unwrap();
        let _ = samples
            .iter()
            .map(|sample| hasher.update((i24::try_from(*sample)).unwrap().to_le_bytes()))
            .collect::<Vec<_>>();
    }
    Ok(())
}

/* fn process_samples_i32(
    hasher: &mut impl Digest,
    mut reader: FlacReader<File>,
    enc: &mut FlacEncoder,
    config: &StreamConfig,
) -> Result<()> {
    for samples in reader
        .samples()
        .map(|sample| sample.unwrap())
        .collect::<Vec<_>>()
        .chunks(4096 * usize::try_from(config.channels).unwrap())
    {
        enc.process_interleaved(samples, u32::try_from(samples.len()).unwrap() / config.channels)
            .unwrap();
        let _ = samples
            .iter()
            .map(|sample| hasher.update(sample.to_le_bytes()))
            .collect::<Vec<_>>();
    }
    for sample in reader.samples() {
        sample?;
    }
    Ok(())
} */

pub fn encode_file(file: &std::path::Path) -> Result<()> {
    let reader = claxon::FlacReader::open(file)?;
    let config = StreamConfig {
        channels: reader.streaminfo().channels,
        bits_per_sample: Bps::new(reader.streaminfo().bits_per_sample)?,
        sample_rate: reader.streaminfo().sample_rate,
        total_samples_estimate: reader.streaminfo().samples.unwrap(),
    };

    let tempname = format!("{}.{}", file.display(), "tmp");

    let mut outf = File::create(&tempname)?;
    let mut outw = WriteWrapper(&mut outf);
    let mut enc = FlacEncoder::new()
        .unwrap()
        .channels(config.channels)
        .bits_per_sample(config.bits_per_sample.value())
        .sample_rate(config.sample_rate)
        .total_samples_estimate(config.total_samples_estimate)
        .compression_level(8)
        .verify(false)
        .init_write(&mut outw)
        .unwrap();

    let mut hasher = Md5::new();

    match config.bits_per_sample {
        Bps::_16 => process_samples_i16(&mut hasher, reader, &mut enc, &config)?,
        Bps::_24 => process_samples_i24(&mut hasher, reader, &mut enc, &config)?,
        /* Bps::_32 => process_samples_i32(&mut hasher, reader, &mut enc, &config)?, */
        Bps::_32 => unimplemented!("32bit flac isnt supported by claxon")
    };

    if let Err(enc) = enc.finish() {
        return Err(anyhow!("Encoding failed:\t{:?}", enc.state()));
    }

    let tags = Tag::read_from_path(file)?;
    let mut output = Tag::read_from_path(&tempname)?;
    let mut streaminfo = tags.get_streaminfo().unwrap().clone();

    streaminfo.md5 = hasher.finalize()[..].to_vec();
    output.set_streaminfo(streaminfo);

    for block in tags.blocks() {
        match block {
            Block::VorbisComment(comment) => {
                for (key, val) in comment.comments.clone() {
                    output.set_vorbis(key, val);
                }
            }
            Block::StreamInfo(_) => {}
            _ => output.push_block(block.clone()),
        }
    }

    output.write_to_path(tempname)?;

    Ok(())
}
