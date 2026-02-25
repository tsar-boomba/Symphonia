// Symphonia
// Copyright (c) 2019-2022 The Project Symphonia Developers.
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ID3v2 frame readers.

use core::pin::Pin;
use core::str;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use hashbrown::HashMap;
use symphonia_core::Lazy;
use symphonia_core::errors::{Result, decode_error};
use symphonia_core::io::{BufReader, FiniteStream, ReadBytes};
use symphonia_core::meta::RawTagSubField;
use symphonia_core::meta::{Chapter, RawValue, Tag, Visual};

use log::warn;
use smallvec::SmallVec;

use crate::id3v2::sub_fields::*;
use crate::id3v2::unsync::{decode_unsynchronisation, read_syncsafe_leq32};

mod readers;

use readers::*;

// The following is a list of all standardized ID3v2.x frames for all ID3v2 major versions and their
// implementation status ("S" column) in Symphonia.
//
// ID3v2.2 uses 3 character frame identifiers as opposed to the 4 character identifiers used in
// subsequent versions. This table may be used to map equivalent frames between the two versions.
//
// All ID3v2.3 frames are officially part of ID3v2.4 with the exception of those marked "n/a".
// However, it is likely that ID3v2.3-only frames appear in some real-world ID3v2.4 tags.
//
// -   ----   ----   ----   --------------------   -------------------------------------------------
// S   v2.2   v2.3   v2.4   StandardTag            Description
// -   ----   ----   ----   --------------------   -------------------------------------------------
// x   CRA    AENC                                 Audio encryption
// x   CRM                                         Encrypted meta frame
// x   PIC    APIC          n/a                    Attached picture
//                   ASPI   n/a                    Audio seek point index
// x          ATXT                                 Audio text frame
// x          CHAP          n/a                    Chapter
// x   COM    COMM          Comment                Comments
// x          COMR                                 Commercial frame
// x          CTOC          n/a                    Table of contents
// x          ENCR                                 Encryption method registration
// x   EQU    EQUA                                 Equalisation
// x                 EQU2                          Equalisation (2)
// x   ETC    ETCO                                 Event timing codes
// x   GEO    GEOB                                 General encapsulated object
// x          GRID                                 Group identification registration
// x   IPL    IPLS   TIPL   many                   Involved people list
// x   LNK    LINK                                 Linked information
// x   MCI    MCDI          CdToc                  Music CD identifier
//     MLL    MLLT          n/a                    MPEG location lookup table
// x          OWNE                                 Ownership frame
// x          PRIV                                 Private frame
// x   CNT    PCNT          PlayCounter            Play counter
// x   POP    POPM          Rating                 Popularimeter
// x          POSS                                 Position synchronisation frame
// x   BUF    RBUF                                 Recommended buffer size
// x   RVA    RVAD                                 Relative volume adjustment
// x                 RVA2                          Relative volume adjustment (2)
// x   REV    RVRB                                 Reverb
// x                 SEEK                          Seek frame
// x                 SIGN                          Signature frame
// x   SLT    SYLT                                 Synchronized lyric/text
// x   STC    SYTC                                 Synchronized tempo codes
// x   TAL    TALB          Album                  Album/Movie/Show title
// x   TBP    TBPM          Bpm                    BPM (beats per minute)
// x   TCM    TCOM          Composer               Composer
// x   TCO    TCON          Genre                  Content type
// x   TCR    TCOP          Copyright              Copyright message
// x   TDA    TDAT          Date                   Date
// x                 TDEN   EncodingDate           Encoding time
// x   TDY    TDLY                                 Playlist delay
// x                 TDOR   OriginalDate           Original release time
// x                 TDRC   Date                   Recording date
// x                 TDRL   ReleaseDate            Release time
// x                 TDTG   TaggingDate            Tagging time
// x   TEN    TENC          EncodedBy              Encoded by
// x   TXT    TEXT          Writer                 Lyricist/Text writer
// x   TFT    TFLT                                 File type
// x   TIM    TIME    n/a   Date                   Time
// x   TT1    TIT1          Grouping               Content group description
// x   TT2    TIT2          TrackTitle             Title/songname/content description
// x   TT3    TIT3          TrackSubtitle          Subtitle/Description refinement
// x   TKE    TKEY          InitialKey             Initial key
// x   TLA    TLAN          Language               Language(s)
// x   TLE    TLEN                                 Length
// x                 TMCL                          Musician credits list
// x   TMT    TMED          MediaFormat            Media type
// x                 TMOO   Mood                   Mood
// x   TOT    TOAL          OriginalAlbum          Original album/movie/show title
// x   TOF    TOFN          OriginalFile           Original filename
// x   TOL    TOLY          OriginalWriter         Original lyricist(s)/text writer(s)
// x   TOA    TOPE          OriginalArtist         Original artist(s)/performer(s)
// x   TOR    TORY   n/a    OriginalDate           Original release year
// x          TOWN          Owner                  File owner/licensee
// x   TP1    TPE1          Artist                 Lead performer(s)/Soloist(s)
// x   TP2    TPE2          AlbumArtist            Band/orchestra/accompaniment
// x   TP3    TPE3          Performer              Conductor/performer refinement
// x   TP4    TPE4          Remixer                Interpreted, remixed, or otherwise modified by
// x   TPA    TPOS          TrackNumber            Part of a set
// x                 TPRO   ProductionCopyright    Produced notice (production copyright)
// x   TPB    TPUB          Label                  Publisher
// x   TRK    TRCK          TrackNumber            Track number/Position in set
// x   TRD    TRDA   n/a    Date                   Recording dates
// x          TRSN          InternetRadioName      Internet radio station name
// x          TRSO          InternetRadioOwner     Internet radio station owner
// x                 TSOA   SortAlbum              Album sort order
// x                 TSOP   SortArtist             Performer sort order
// x                 TSOT   SortTrackTitle         Title sort order
// x   TSI    TSIZ   n/a                           Size
// x   TRC    TSRC          IdentIsrc              ISRC (international standard recording code)
// x   TSS    TSSE          Encoder                Software/Hardware and settings used for encoding
// x                 TSST   DiscSubtitle           Disc/Set subtitle
// x   TYE    TYER   n/a    Date                   Year
// x   TXX    TXXX                                 User defined text information frame
// x   UFI    UFID                                 Unique file identifier
// x          USER          TermsOfUse             Terms of use
// x   ULT    USLT          Lyrics                 Unsychronized lyric/text transcription
// x   WCM    WCOM          UrlPurchase            Commercial information
// x   WCP    WCOP          UrlCopyright           Copyright/Legal information
// x   WAF    WOAF          UrlOfficial            Official audio file webpage
// x   WAR    WOAR          UrlArtist              Official artist/performer webpage
// x   WAS    WOAS          UrlSource              Official audio source webpage
// x          WORS          UrlInternetRadio       Official internet radio station homepage
// x          WPAY          UrlPayment             Payment
// x   WPB    WPUB          UrlLabel               Publishers official webpage
// x   WXX    WXXX          Url                    User defined URL link frame
// x          GRP1          Grouping               (Apple iTunes) Grouping
// x          MVNM          MovementName           (Apple iTunes) Movement name
// x          MVIN          MovementNumber         (Apple iTunes) Movement number
// x   PCS    PCST          Podcast                (Apple iTunes) Podcast flag
// x          TCAT          PodcastCategory        (Apple iTunes) Podcast category
// x          TDES          PodcastDescription     (Apple iTunes) Podcast description
// x          TGID          IdentPodcast           (Apple iTunes) Podcast identifier
// x          TKWD          PodcastKeywords        (Apple iTunes) Podcast keywords
// x          WFED          UrlPodcast             (Apple iTunes) Podcast url
// x   TST                  SortTrackTitle         (Apple iTunes) Title sort order
// x   TSP                  SortArtist             (Apple iTunes) Artist order order
// x   TSA                  SortAlbum              (Apple iTunes) Album sort order
// x   TS2    TSO2          SortAlbumArtist        (Apple iTunes) Album artist sort order
// x   TSC    TSOC          SortComposer           (Apple iTunes) Composer sort order
// x   TCP    TCMP          Compilation            (Apple iTunes) Compilation flag
//
// Information on these frames can be found at:
//
//     ID3v2.2: http://id3.org/id3v2-00
//     ID3v2.3: http://id3.org/d3v2.3.0
//     ID3v2.4: http://id3.org/id3v2.4.0-frames

/// An ID3v2 chapter.
#[derive(Clone, Debug)]
pub struct Id3v2Chapter {
    // The chapter identifier.
    pub id: String,
    /// A counter indicating the order the chapter frame was read.
    pub read_order: usize,
    /// The chapter contents.
    pub chapter: Chapter,
}

/// An ID3v2 table of contents describes different sections and chapters of an audio stream.
#[derive(Clone, Debug, Default)]
pub struct Id3v2TableOfContents {
    /// The table of contents identifier.
    pub id: String,
    /// Indicates if this is the top-level table of contents frame. Only one table of contents
    /// frame should be marked top-level, and not be a child of any other frame.
    pub top_level: bool,
    /// Indicates if the entries should be played as a continuous ordered sequence or played
    /// individually.
    ///
    /// TODO: It is not clear if this is useful.
    #[allow(dead_code)]
    pub ordered: bool,
    /// The identifiers of the items that belong to this table of contents. These may identify
    /// a chapter or another table of contents.
    pub items: Vec<String>,
    /// The tags associated with this table of contents.
    pub tags: Vec<Tag>,
    /// The visuals associated with this table of contents.
    pub visuals: Vec<Visual>,
}

/// The result of parsing a frame.
pub enum FrameResult {
    /// The frame was skipped.
    Skipped,
    /// Padding was encountered instead of a frame. The remainder of the ID3v2 tag may be skipped.
    Padding,
    /// A frame was parsed and yielded a single `Tag`.
    Tag(Tag),
    /// A frame was parsed and yielded a single `Visual`.
    Visual(Visual),
    /// A frame was parsed and yielded many `Tag`s.
    MultipleTags(SmallVec<[Tag; 2]>),
    /// A frame was parsed and yielded a chapter.
    Chapter(Id3v2Chapter),
    /// A frame was parsed and yielded a table of contents.
    TableOfContents(Id3v2TableOfContents),
}

/// Gets the minimum frame size for a major version of an ID3v2.
pub fn min_frame_size(major_version: u8) -> u64 {
    match major_version {
        2 => 6,
        3 | 4 => 10,
        _ => unreachable!("id2v3: unexpected version"),
    }
}

static LEGACY_FRAME_MAP: Lazy<HashMap<&'static [u8; 3], &'static [u8; 4]>> = Lazy::new(|| {
    let mut m = HashMap::new();
    m.insert(b"BUF", b"RBUF");
    m.insert(b"CNT", b"PCNT");
    m.insert(b"COM", b"COMM");
    m.insert(b"CRA", b"AENC");
    m.insert(b"CRM", b"CRM_"); // Maps to pseudo-ID3v2.4 frame.
    m.insert(b"EQU", b"EQUA");
    m.insert(b"ETC", b"ETCO");
    m.insert(b"GEO", b"GEOB");
    m.insert(b"IPL", b"IPLS");
    m.insert(b"LNK", b"LINK");
    m.insert(b"MCI", b"MCDI");
    m.insert(b"MLL", b"MLLT");
    m.insert(b"PCS", b"PCST");
    m.insert(b"PIC", b"APIC");
    m.insert(b"POP", b"POPM");
    m.insert(b"REV", b"RVRB");
    m.insert(b"RVA", b"RVAD");
    m.insert(b"SLT", b"SYLT");
    m.insert(b"STC", b"SYTC");
    m.insert(b"TAL", b"TALB");
    m.insert(b"TBP", b"TBPM");
    m.insert(b"TCM", b"TCOM");
    m.insert(b"TCO", b"TCON");
    m.insert(b"TCP", b"TCMP");
    m.insert(b"TCR", b"TCOP");
    m.insert(b"TDA", b"TDAT");
    m.insert(b"TDY", b"TDLY");
    m.insert(b"TEN", b"TENC");
    m.insert(b"TFT", b"TFLT");
    m.insert(b"TIM", b"TIME");
    m.insert(b"TKE", b"TKEY");
    m.insert(b"TLA", b"TLAN");
    m.insert(b"TLE", b"TLEN");
    m.insert(b"TMT", b"TMED");
    m.insert(b"TOA", b"TOPE");
    m.insert(b"TOF", b"TOFN");
    m.insert(b"TOL", b"TOLY");
    m.insert(b"TOR", b"TORY");
    m.insert(b"TOT", b"TOAL");
    m.insert(b"TP1", b"TPE1");
    m.insert(b"TP2", b"TPE2");
    m.insert(b"TP3", b"TPE3");
    m.insert(b"TP4", b"TPE4");
    m.insert(b"TPA", b"TPOS");
    m.insert(b"TPB", b"TPUB");
    m.insert(b"TRC", b"TSRC");
    m.insert(b"TRD", b"TRDA");
    m.insert(b"TRK", b"TRCK");
    m.insert(b"TS2", b"TSO2");
    m.insert(b"TSA", b"TSOA");
    m.insert(b"TSC", b"TSOC");
    m.insert(b"TSI", b"TSIZ");
    m.insert(b"TSP", b"TSOP");
    m.insert(b"TSS", b"TSSE");
    m.insert(b"TST", b"TSOT");
    m.insert(b"TT1", b"TIT1");
    m.insert(b"TT2", b"TIT2");
    m.insert(b"TT3", b"TIT3");
    m.insert(b"TXT", b"TEXT");
    m.insert(b"TXX", b"TXXX");
    m.insert(b"TYE", b"TYER");
    m.insert(b"UFI", b"UFID");
    m.insert(b"ULT", b"USLT");
    m.insert(b"WAF", b"WOAF");
    m.insert(b"WAR", b"WOAR");
    m.insert(b"WAS", b"WOAS");
    m.insert(b"WCM", b"WCOM");
    m.insert(b"WCP", b"WCOP");
    m.insert(b"WPB", b"WPUB");
    m.insert(b"WXX", b"WXXX");
    m
});

/// Validates that a frame id only contains uppercase letters (A-Z), and digits (0-9).
fn validate_frame_id(id: &[u8]) -> bool {
    // Only frame IDs with 3 or 4 characters are valid.
    if id.len() != 4 && id.len() != 3 {
        return false;
    }

    id.iter().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

/// Gets a slice of ASCII bytes as a string slice.
///
/// Assumes the bytes are valid ASCII characters. Panics otherwise.
fn from_ascii(id: &[u8]) -> &str {
    core::str::from_utf8(id).expect("ascii only")
}

/// Find a frame reader and optional raw tag parser for legacy ID3v2.2 frames by finding an
/// equivalent modern ID3v2.3+ frame reader.
fn legacy_id_to_modern(id: [u8; 3]) -> [u8; 4] {
    match LEGACY_FRAME_MAP.get(&id) {
        Some(id) => **id,
        _ => [0, 0, 0, 0],
    }
}

/// Read an ID3v2.2 frame.
pub fn read_id3v2p2_frame<B: ReadBytes>(
    reader: &mut B,
) -> Pin<Box<dyn Future<Output = Result<FrameResult>> + Send + '_>> {
    Box::pin(async {
        let id = reader.read_triple_bytes().await?;

        // Check if the frame id contains valid characters. If it does not, then assume the rest of the
        // tag is padding. As per the specification, padding should be all 0s, but there are some tags
        // which don't obey the specification.
        if !validate_frame_id(&id) {
            // As per the specification, padding should be all 0s, but there are some tags which don't
            // obey the specification.
            if id != [0, 0, 0] {
                warn!("padding bytes not zero");
            }

            return Ok(FrameResult::Padding);
        }

        let size = u64::from(reader.read_be_u24().await?);

        // Find a reader for the frame.
        let id = legacy_id_to_modern(id);

        // A frame must be atleast 1 byte as per the specification.
        if size == 0 {
            warn!("'{}' was skipped because it has a size of 0", from_ascii(&id));
            return Ok(FrameResult::Skipped);
        }

        // Read the frame body into a frame buffer.
        let data = reader.read_boxed_slice_exact(size as usize).await?;

        // An error while reading the frame from the frame buffer is not fatal.
        match read_frame(BufReader::new(&data), &id, 2).await {
            Ok(result) => Ok(result),
            Err(err) => {
                // On error, skip the frame.
                warn!("{err}");
                Ok(FrameResult::Skipped)
            }
        }
    })
}

/// Read an ID3v2.3 frame.
pub fn read_id3v2p3_frame<B: ReadBytes>(
    reader: &mut B,
) -> Pin<Box<dyn Future<Output = Result<FrameResult>> + Send + '_>> {
    Box::pin(async {
        let id = reader.read_quad_bytes().await?;

        // Check if the frame id contains valid characters. If it does not, then assume the rest of the
        // tag is padding. As per the specification, padding should be all 0s, but there are some tags
        // which don't obey the specification.
        if !validate_frame_id(&id) {
            // As per the specification, padding should be all 0s, but there are some tags which don't
            // obey the specification.
            if id != [0, 0, 0, 0] {
                warn!("padding bytes not zero");
            }

            return Ok(FrameResult::Padding);
        }

        // The size of the frame after encryption, compression, and unsynchronisation.
        let size = reader.read_be_u32().await?;

        // Frame-specific flags.
        let flags = reader.read_be_u16().await?;

        // Unused flag bits must be cleared.
        if flags & 0x1f1f != 0x0 {
            return decode_error("id3v2: unused flag bits are not cleared");
        }

        // Frame-specific flags that are important for reading.
        let is_compressed = flags & 0x80 != 0;
        let is_encrypted = flags & 0x40 != 0;
        let is_grouped = flags & 0x20 != 0;

        // When some flags are set, the frame header is extended with additional fields. Calculate the
        // size of these fields.
        let flag_data_size = if is_compressed { 4 } else { 0 } // 4-byte decompressed size.
        + if is_encrypted { 1 } else { 0 } // 1-byte encryption ID.
        + if is_grouped { 1 } else { 0 }; // 1-byte group ID.

        // If the frame size is too small for the extended header, there is a fatal framing error.
        if size < flag_data_size {
            return decode_error("id3v2: the frame is too small");
        }

        // The size of the frame's body.
        let data_size = size - flag_data_size;

        // If compression is enabled, read the decompressed size of the frame.
        let _decompressed_size =
            if is_compressed { Some(reader.read_be_u32().await?) } else { None };

        // If encryption is enabled, read the encryption ID. A sub-field indicating the frame is
        // encrypted, and its encryption ID will be added to the tag.
        let encryption_id = if is_encrypted { Some(reader.read_byte().await?) } else { None };

        // If frame grouping is enabled, read the group ID of the frame. A sub-field indicating the
        // group will be added to the tag.
        let group_id = if is_grouped { Some(reader.read_byte().await?) } else { None };

        // TODO: Implement zlib DEFLATE decompression.
        if is_compressed {
            reader.ignore_bytes(u64::from(data_size)).await?;

            warn!("'{}' was skipped because compressed frames are not supported", from_ascii(&id));
            return Ok(FrameResult::Skipped);
        }

        // A zero-length frame body is not allowed, but can be skipped.
        if data_size == 0 {
            warn!("'{}' was skipped because it has a size of 0", from_ascii(&id));
            return Ok(FrameResult::Skipped);
        }

        // Read the frame body into a frame buffer.
        let data = reader.read_boxed_slice_exact(data_size as usize).await?;

        // Read the frame from the frame buffer. An error here is not fatal.
        match read_frame(BufReader::new(&data), &id, 3).await {
            Ok(mut result) => {
                // Add the sub-fields to the tag if the frame is grouped or encrypted.
                if is_grouped || is_encrypted {
                    append_tag_sub_fields(&mut result, group_id, encryption_id);
                }
                Ok(result)
            }
            Err(err) => {
                // On error, skip the frame.
                warn!("{err}");
                Ok(FrameResult::Skipped)
            }
        }
    })
}

/// Read an ID3v2.4 frame.
pub fn read_id3v2p4_frame<B: ReadBytes + FiniteStream>(
    reader: &mut B,
) -> Pin<Box<dyn Future<Output = Result<FrameResult>> + Send + '_>> {
    Box::pin(async {
        let id = reader.read_quad_bytes().await?;

        // Check if the frame id contains valid characters. If it does not, then assume the rest of the
        // tag is padding.
        if !validate_frame_id(&id) {
            // As per the specification, padding should be all 0s, but there are some tags which don't
            // obey the specification.
            if id != [0, 0, 0, 0] {
                warn!("padding bytes not zero");
            }

            return Ok(FrameResult::Padding);
        }

        // The size of the frame after encryption, compression, and unsynchronisation.
        let size = read_syncsafe_leq32(reader, 28).await?;

        // Frame-specific flags.
        let flags = reader.read_be_u16().await?;

        // Unused flag bits must be cleared.
        if flags & 0x8fb0 != 0x0 {
            return decode_error("id3v2: unused flag bits are not cleared");
        }

        // Frame-specific flags that are important for reading.
        let is_grouped = flags & 0x40 != 0;
        let is_compressed = flags & 0x08 != 0;
        let is_encrypted = flags & 0x04 != 0;
        let is_unsynchronised = flags & 0x2 != 0;
        let has_indicated_size = flags & 0x1 != 0;

        if is_compressed && !has_indicated_size {
            return decode_error("id3v2: frame compressed without a data length indicator");
        }

        // When some flags are set, the frame header is extended with additional fields. Calculate the
        // size of these fields.
        let flag_data_size = if is_grouped { 1 } else { 0 } // 1-byte group ID.
        + if is_encrypted { 1 } else { 0 } // 1-byte encryption ID.
        + if has_indicated_size { 4 } else { 0 }; // 4-byte data length indicator.

        // If the frame size is too small for the extended header, there is a fatal framing error.
        if size < flag_data_size {
            return decode_error("id3v2: the frame is too small");
        }

        // The size of the frame's body.
        let data_size = size - flag_data_size;

        // Frame group identifier byte. Used to group a set of frames. The frame group will be added as
        // a sub-field to all produced tags.
        let group_id = if is_grouped { Some(reader.read_byte().await?) } else { None };

        // Frame encryption flag. Encryption is vendor-specific. Therefore, an encrypted frame will only
        // be provided as a binary buffer. A sub-field indicating the frame is encrypted will be added.
        let encryption_id = if is_encrypted { Some(reader.read_byte().await?) } else { None };

        // The data length indicator is optional in the frame header. This field indicates the original
        // size of the frame body before compression, encryption, and/or unsynchronisation. It is
        // mandatory if compression is used, but only encouraged for unsynchronisation, and optional for
        // encryption.
        //
        // The indicated size will be added to all produced tags as a sub-field if the frame is
        // encrypted.
        let _indicated_size =
            if has_indicated_size { Some(read_syncsafe_leq32(reader, 28).await?) } else { None };

        // TODO: Implement zlib DEFLATE decompression.
        if is_compressed {
            reader.ignore_bytes(u64::from(data_size)).await?;

            warn!("'{}' was skipped because compressed frames are not supported", from_ascii(&id));
            return Ok(FrameResult::Skipped);
        }

        // A zero-length frame body is not allowed, but can be skipped.
        if data_size == 0 {
            warn!("'{}' was skipped because it has a size of 0", from_ascii(&id));
            return Ok(FrameResult::Skipped);
        }

        // Read the frame body into a frame buffer.
        let mut data = reader.read_boxed_slice_exact(data_size as usize).await?;

        // Read the frame.
        let result = if is_unsynchronised {
            // The frame body has been unsynchronised. Decode the unsynchronised data back to it's
            // unencoded form in-place before parsing.
            let unsync_data = decode_unsynchronisation(&mut data);

            read_frame(BufReader::new(unsync_data), &id, 4).await
        }
        else {
            // The frame body has not been unsynchronised.
            read_frame(BufReader::new(&data), &id, 4).await
        };

        // An error while reading the frame from the frame buffer is not fatal.
        match result {
            Ok(mut result) => {
                // Add the sub-fields to the tag if the frame is grouped or encrypted.
                if is_grouped || is_encrypted {
                    append_tag_sub_fields(&mut result, group_id, encryption_id);
                }
                Ok(result)
            }
            Err(err) => {
                // On error, skip the frame.
                warn!("{err}");
                Ok(FrameResult::Skipped)
            }
        }
    })
}

fn append_tag_sub_fields(result: &mut FrameResult, grp_id: Option<u8>, enc_id: Option<u8>) {
    let add_sub_fields = |tag: &mut Tag| {
        // Take the existing boxed sub-fields, and turn it into a vector. Or, create a new
        // vector.
        let mut sub_fields = tag.raw.sub_fields.take().unwrap_or_default().into_vec();

        if let Some(id) = grp_id {
            sub_fields.push(RawTagSubField::new(GROUP_ID, RawValue::from(id)));
        }

        if let Some(id) = enc_id {
            sub_fields.push(RawTagSubField::new(ENCRYPTION_METHOD_ID, RawValue::from(id)));
        }

        // Put back the extended sub-fields.
        tag.raw.sub_fields.replace(sub_fields.into_boxed_slice());
    };

    match result {
        FrameResult::Tag(tag) => add_sub_fields(tag),
        FrameResult::MultipleTags(tags) => tags.iter_mut().for_each(add_sub_fields),
        _ => (),
    }
}
