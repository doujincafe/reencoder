use anyhow::{Result, anyhow};
use flac_bound::{FlacEncoder, WriteWrapper};
use md5::{Digest, Md5};
use metaflac::Tag;
use std::fs::File;

#[derive(Debug)]
struct StreamConfig {
    channels: u32,
    bits_per_sample: u32,
    sample_rate: u32,
    total_samples_estimate: u64,
}

pub fn encode_file(file: &std::path::Path) -> Result<()> {
    let mut reader = claxon::FlacReader::open(file)?;
    let config = StreamConfig {
        channels: reader.streaminfo().channels,
        bits_per_sample: reader.streaminfo().bits_per_sample,
        sample_rate: reader.streaminfo().sample_rate,
        total_samples_estimate: reader.streaminfo().samples.unwrap(),
    };
    let tempname = format!("{}.{}", file.display(), "tmp");
    let mut outf = File::create(&tempname)?;
    let mut outw = WriteWrapper(&mut outf);
    let mut enc = FlacEncoder::new()
        .unwrap()
        .channels(config.channels)
        .bits_per_sample(config.bits_per_sample)
        .sample_rate(config.sample_rate)
        .total_samples_estimate(config.total_samples_estimate)
        .compression_level(8)
        .verify(false)
        .init_write(&mut outw)
        .unwrap();

    let mut hasher = Md5::new();
    let mut bytes = Vec::new();

    for samples in reader
        .samples()
        .map(|sample| sample.unwrap())
        .collect::<Vec<_>>()
        .chunks(4096)
    {
        enc.process_interleaved(samples, 2048).unwrap();
        let _ = samples
            .iter()
            .map(|sample| {
                for byte in sample.to_le_bytes() {
                    bytes.push(byte)
                }
            })
            .collect::<Vec<_>>();
        hasher.update(&bytes);
        bytes.clear();
    }

    match enc.finish() {
        Ok(_) => {}
        Err(enc) => return Err(anyhow!("Encoding failed:\t{:?}", enc.state())),
    }

    /* let source_tags = Tag::read_from_path(file)?;
    let mut target_tags = Tag::read_from_path(tempname)?;


    for block in source_tags.blocks() {
        todo!()
    } */

    let mut tags = Tag::read_from_path(tempname)?;
    let mut streaminfo = tags.get_streaminfo().unwrap().clone();
    streaminfo.md5 = hasher.finalize()[..].to_vec();
    tags.set_streaminfo(streaminfo);
    tags.save()?;

    Ok(())
}
