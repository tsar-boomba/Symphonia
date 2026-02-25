use symphonia_codec_aac::{AacDecoder, AdtsFormat, AdtsReader};
use symphonia_core::codecs::audio::{
    AudioCodecParameters, AudioDecoder, AudioDecoderOptions, well_known::CODEC_ID_AAC,
};
use symphonia_core::errors;
use symphonia_core::formats::probe::ProbeableFormat;
use symphonia_core::io::{Cursor, MediaSourceStream};

async fn test_decode(data: Vec<u8>) -> symphonia_core::errors::Result<()> {
    let data = Cursor::new(data);

    let mss = MediaSourceStream::new(Box::new(data), Default::default());

    let mut reader = AdtsFormat::try_probe_new(mss, Default::default()).await?;

    let mut decoder = AacDecoder::try_new(
        AudioCodecParameters::new().for_codec(CODEC_ID_AAC),
        &AudioDecoderOptions::default(),
    )
    .await?;

    loop {
        match reader.next_packet().await? {
            Some(packet) => {
                let _ = decoder.decode(&packet);
            }
            None => break,
        };
    }

    Ok(())
}

#[futures_test::test]
async fn invalid_channels_aac() {
    let file = vec![
        0xff, 0xf1, 0xaf, 0xce, 0x02, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xfb,
        0xaf,
    ];

    let err = test_decode(file).await.unwrap_err();

    assert!(matches!(err, errors::Error::Unsupported(_)));
}
