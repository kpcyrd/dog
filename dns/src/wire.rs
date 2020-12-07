//! Parsing the DNS wire protocol.

pub(crate) use std::io::{Cursor, Read};
pub(crate) use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};

use std::io;
use log::*;

use crate::record::{Record, OPT};
use crate::strings::{Labels, ReadLabels, WriteLabels};
use crate::types::*;


impl Request {

    /// Converts this request to a vector of bytes.
    pub fn to_bytes(&self) -> io::Result<Vec<u8>> {
        let mut bytes = Vec::with_capacity(32);

        bytes.write_u16::<BigEndian>(self.transaction_id)?;
        bytes.write_u16::<BigEndian>(self.flags.to_u16())?;

        bytes.write_u16::<BigEndian>(1)?;  // query count
        bytes.write_u16::<BigEndian>(0)?;  // answer count
        bytes.write_u16::<BigEndian>(0)?;  // authority RR count
        bytes.write_u16::<BigEndian>(if self.additional.is_some() { 1 } else { 0 })?;  // additional RR count

        bytes.write_labels(&self.query.qname)?;
        bytes.write_u16::<BigEndian>(self.query.qtype)?;
        bytes.write_u16::<BigEndian>(self.query.qclass.to_u16())?;

        if let Some(opt) = &self.additional {
            bytes.write_u8(0)?;  // usually a name
            bytes.write_u16::<BigEndian>(OPT::RR_TYPE)?;
            bytes.extend(opt.to_bytes()?);
        }

        Ok(bytes)
    }

    /// Returns the OPT record to be sent as part of requests.
    pub fn additional_record() -> OPT {
        OPT {
            udp_payload_size: 512,
            higher_bits: 0,
            edns0_version: 0,
            flags: 0,
            data: Vec::new(),
        }
    }
}


impl Response {

    /// Reads bytes off of the given slice, parsing them into a response.
    #[cfg_attr(feature = "with_mutagen", ::mutagen::mutate)]
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, WireError> {
        info!("Parsing response");
        trace!("Bytes -> {:?}", bytes);
        let mut c = Cursor::new(bytes);

        let transaction_id = c.read_u16::<BigEndian>()?;
        trace!("Read txid -> {:?}", transaction_id);

        let flags = Flags::from_u16(c.read_u16::<BigEndian>()?);
        trace!("Read flags -> {:#?}", flags);

        let query_count      = c.read_u16::<BigEndian>()?;
        let answer_count     = c.read_u16::<BigEndian>()?;
        let authority_count  = c.read_u16::<BigEndian>()?;
        let additional_count = c.read_u16::<BigEndian>()?;

        let mut queries = Vec::new();
        debug!("Reading {}x query from response", query_count);
        for _ in 0 .. query_count {
            let (qname, _) = c.read_labels()?;
            queries.push(Query::from_bytes(qname, &mut c)?);
        }

        let mut answers = Vec::new();
        debug!("Reading {}x answer from response", answer_count);
        for _ in 0 .. answer_count {
            let (qname, _) = c.read_labels()?;
            answers.push(Answer::from_bytes(qname, &mut c)?);
        }

        let mut authorities = Vec::new();
        debug!("Reading {}x authority from response", authority_count);
        for _ in 0 .. authority_count {
            let (qname, _) = c.read_labels()?;
            authorities.push(Answer::from_bytes(qname, &mut c)?);
        }

        let mut additionals = Vec::new();
        debug!("Reading {}x additional answer from response", additional_count);
        for _ in 0 .. additional_count {
            let (qname, _) = c.read_labels()?;
            additionals.push(Answer::from_bytes(qname, &mut c)?);
        }

        Ok(Self { transaction_id, flags, queries, answers, authorities, additionals })
    }
}


impl Query {

    /// Reads bytes from the given cursor, and parses them into a query with
    /// the given domain name.
    #[cfg_attr(feature = "with_mutagen", ::mutagen::mutate)]
    fn from_bytes(qname: Labels, c: &mut Cursor<&[u8]>) -> Result<Self, WireError> {
        let qtype = c.read_u16::<BigEndian>()?;
        trace!("Read qtype -> {:?}", qtype);

        let qclass = QClass::from_u16(c.read_u16::<BigEndian>()?);
        trace!("Read qclass -> {:?}", qtype);

        Ok(Self { qtype, qclass, qname })
    }
}


impl Answer {

    /// Reads bytes from the given cursor, and parses them into an answer with
    /// the given domain name.
    #[cfg_attr(feature = "with_mutagen", ::mutagen::mutate)]
    fn from_bytes(qname: Labels, c: &mut Cursor<&[u8]>) -> Result<Self, WireError> {
        let qtype = c.read_u16::<BigEndian>()?;
        trace!("Read qtype -> {:?}", qtype);

        if qtype == OPT::RR_TYPE {
            let opt = OPT::read(c)?;
            Ok(Self::Pseudo { qname, opt })
        }
        else {
            let qclass = QClass::from_u16(c.read_u16::<BigEndian>()?);
            trace!("Read qclass -> {:?}", qtype);

            let ttl = c.read_u32::<BigEndian>()?;
            trace!("Read TTL -> {:?}", ttl);

            let record_length = c.read_u16::<BigEndian>()?;
            trace!("Read record length -> {:?}", record_length);

            let record = Record::from_bytes(qtype, record_length, c)?;
            Ok(Self::Standard { qclass, qname, record, ttl })
        }

    }
}


impl Record {

    /// Reads at most `len` bytes from the given curser, and parses them into
    /// a record structure depending on the type number, which has already been read.
    #[cfg_attr(feature = "with_mutagen", ::mutagen::mutate)]
    fn from_bytes(qtype: TypeInt, len: u16, c: &mut Cursor<&[u8]>) -> Result<Self, WireError> {
        use crate::record::*;

        if cfg!(feature = "with_mutagen") {
            warn!("Mutation is enabled!");
        }

        macro_rules! try_record {
            ($record:tt) => {
                if $record::RR_TYPE == qtype {
                    info!("Parsing {} record (type {}, len {})", $record::NAME, qtype, len);
                    return Wire::read(len, c).map(Self::$record)
                }
            }
        }

        // Try all the records, one type at a time, returning early if the
        // type number matches.
        try_record!(A);
        try_record!(AAAA);
        try_record!(CAA);
        try_record!(CNAME);
        try_record!(EUI48);
        try_record!(HINFO);
        try_record!(LOC);
        try_record!(MX);
        try_record!(NAPTR);
        try_record!(NS);
        try_record!(OPENPGPKEY);
        // OPT is handled separately
        try_record!(PTR);
        try_record!(SSHFP);
        try_record!(SOA);
        try_record!(SRV);
        try_record!(TLSA);
        try_record!(TXT);
        try_record!(URI);

        // Otherwise, collect the bytes into a vector and return an unknown
        // record type.
        let mut bytes = Vec::new();
        for _ in 0 .. len {
            bytes.push(c.read_u8()?);
        }

        let type_number = UnknownQtype::from(qtype);
        Ok(Self::Other { type_number, bytes })
    }
}


impl QClass {
    fn from_u16(uu: u16) -> Self {
        match uu {
            0x0001 => Self::IN,
            0x0003 => Self::CH,
            0x0004 => Self::HS,
                 _ => Self::Other(uu),
        }
    }

    fn to_u16(self) -> u16 {
        match self {
            Self::IN        => 0x0001,
            Self::CH        => 0x0003,
            Self::HS        => 0x0004,
            Self::Other(uu) => uu,
        }
    }
}


/// Determines the record type number to signify a record with the given name.
pub fn find_qtype_number(record_type: &str) -> Option<TypeInt> {
    use crate::record::*;

    macro_rules! try_record {
        ($record:tt) => {
            if $record::NAME == record_type {
                return Some($record::RR_TYPE);
            }
        }
    }

    try_record!(A);
    try_record!(AAAA);
    try_record!(CAA);
    try_record!(CNAME);
    try_record!(EUI48);
    try_record!(HINFO);
    try_record!(LOC);
    try_record!(MX);
    try_record!(NAPTR);
    try_record!(NS);
    try_record!(OPENPGPKEY);
    // OPT is elsewhere
    try_record!(PTR);
    try_record!(SSHFP);
    try_record!(SOA);
    try_record!(SRV);
    try_record!(TLSA);
    try_record!(TXT);
    try_record!(URI);

    None
}


impl Flags {

    /// The set of flags that represents a query packet.
    pub fn query() -> Self {
        Self::from_u16(0b_0000_0001_0000_0000)
    }

    /// The set of flags that represents a successful response.
    pub fn standard_response() -> Self {
        Self::from_u16(0b_1000_0001_1000_0000)
    }

    /// Converts the flags into a two-byte number.
    pub fn to_u16(self) -> u16 {                 // 0123 4567 89AB CDEF
        let mut                          bits  = 0b_0000_0000_0000_0000;
        if self.response               { bits |= 0b_1000_0000_0000_0000; }
        match self.opcode {
            Opcode::Query     =>       { bits |= 0b_0000_0000_0000_0000; }
            Opcode::Other(_)  =>       { unimplemented!(); }
        }
        if self.authoritative          { bits |= 0b_0000_0100_0000_0000; }
        if self.truncated              { bits |= 0b_0000_0010_0000_0000; }
        if self.recursion_desired      { bits |= 0b_0000_0001_0000_0000; }
        if self.recursion_available    { bits |= 0b_0000_0000_1000_0000; }
        // (the Z bit is reserved)               0b_0000_0000_0100_0000
        if self.authentic_data         { bits |= 0b_0000_0000_0010_0000; }
        if self.checking_disabled      { bits |= 0b_0000_0000_0001_0000; }

        bits
    }

    /// Extracts the flags from the given two-byte number.
    pub fn from_u16(bits: u16) -> Self {
        let has_bit = |bit| { bits & bit == bit };

        Self {
            response:               has_bit(0b_1000_0000_0000_0000),
            opcode:                 Opcode::from_bits((bits.to_be_bytes()[0] & 0b_0111_1000) >> 3),
            authoritative:          has_bit(0b_0000_0100_0000_0000),
            truncated:              has_bit(0b_0000_0010_0000_0000),
            recursion_desired:      has_bit(0b_0000_0001_0000_0000),
            recursion_available:    has_bit(0b_0000_0000_1000_0000),
            authentic_data:         has_bit(0b_0000_0000_0010_0000),
            checking_disabled:      has_bit(0b_0000_0000_0001_0000),
            error_code:             ErrorCode::from_bits(bits & 0b_1111),
        }
    }
}


impl Opcode {

    /// Extracts the opcode from this four-bit number, which should have been
    /// extracted from the packet and shifted to be in the range 0–15.
    fn from_bits(bits: u8) -> Self {
        if bits == 0 {
            Self::Query
        }
        else {
            assert!(bits <= 15, "bits {:#08b} out of range", bits);
            Self::Other(bits)
        }
    }
}


impl ErrorCode {

    /// Extracts the rcode from the last four bits of the flags field.
    fn from_bits(bits: u16) -> Option<Self> {
        if (0x0F01 .. 0x0FFF).contains(&bits) {
            return Some(Self::Private(bits));
        }

        match bits {
            0 => None,
            1 => Some(Self::FormatError),
            2 => Some(Self::ServerFailure),
            3 => Some(Self::NXDomain),
            4 => Some(Self::NotImplemented),
            5 => Some(Self::QueryRefused),
           16 => Some(Self::BadVersion),
            n => Some(Self::Other(n)),
        }
    }
}


/// Trait for decoding DNS record structures from bytes read over the wire.
pub trait Wire: Sized {

    /// This record’s type as a string, such as `"A"` or `"CNAME"`.
    const NAME: &'static str;

    /// The number signifying that a record is of this type.
    /// See <https://www.iana.org/assignments/dns-parameters/dns-parameters.xhtml#dns-parameters-4>
    const RR_TYPE: u16;

    /// Read at most `len` bytes from the given `Cursor`. This cursor travels
    /// throughout the complete data — by this point, we have read the entire
    /// response into a buffer.
    fn read(len: u16, c: &mut Cursor<&[u8]>) -> Result<Self, WireError>;
}


/// Helper macro to get the qtype number of a record type at compile-time.
///
/// # Examples
///
/// ```
/// use dns::{qtype, record::MX};
///
/// assert_eq!(15, qtype!(MX));
/// ```
#[macro_export]
macro_rules! qtype {
    ($type:ty) => {
        <$type as $crate::Wire>::RR_TYPE
    }
}


/// Something that can go wrong deciphering a record.
#[derive(PartialEq, Debug, thiserror::Error)]
pub enum WireError {

    /// There was an IO error reading from the cursor.
    /// Almost all the time, this means that the buffer was too short.
    #[error("Malformed packet: insufficient data")]
    IO,
    // (io::Error is not PartialEq so we don’t propagate it)

    /// When the DNS standard requires records of this type to have a certain
    /// fixed length, but the response specified a different length.
    ///
    /// This error should be returned regardless of the _content_ of the
    /// record, whatever it is.
    #[error("Malformed packet: record length should be {mandated_length}, got {stated_length}")]
    WrongRecordLengthExactly {

        /// The length of the record’s data, as specified in the packet.
        stated_length: u16,

        /// The length of the record that the DNS specification mandates.
        mandated_length: u16,
    },

    /// When the DNS standard requires records of this type to have a certain
    /// fixed length, but the response specified a different length.
    ///
    /// This error should be returned regardless of the _content_ of the
    /// record, whatever it is.
    #[error("Malformed packet: record length should be at least {mandated_length}, got {stated_length}")]
    WrongRecordLengthAtLeast {

        /// The length of the record’s data, as specified in the packet.
        stated_length: u16,

        /// The length of the record that the DNS specification mandates.
        mandated_length: u16,
    },

    /// When the length of this record as specified in the packet differs from
    /// the computed length, as determined by reading labels.
    ///
    /// There are two ways, in general, to read arbitrary-length data from a
    /// stream of bytes: length-prefixed (read the length, then read that many
    /// bytes) or sentinel-terminated (keep reading bytes until you read a
    /// certain value, usually zero). The DNS protocol uses both: each
    /// record’s size is specified up-front in the packet, but inside the
    /// record, there exist arbitrary-length strings that must be read until a
    /// zero is read, indicating there is no more string.
    ///
    /// Consider the case of a packet, with a specified length, containing a
    /// string of arbitrary length (such as the CNAME or TXT records). A DNS
    /// client has to deal with this in one of two ways:
    ///
    /// 1. Read exactly the specified length of bytes from the record, raising
    ///    an error if the contents are too short or a string keeps going past
    ///    the length (assume the length is correct but the contents are wrong).
    ///
    /// 2. Read as many bytes from the record as the string requests, raising
    ///    an error if the number of bytes read at the end differs from the
    ///    expected length of the record (assume the length is wrong but the
    ///    contents are correct).
    ///
    /// Note that no matter which way is picked, the record will still be
    /// incorrect — it only impacts the parsing of records that occur after it
    /// in the packet. Knowing which method should be used requires knowing
    /// what caused the DNS packet to be erroneous, which we cannot know.
    ///
    /// dog picks the second way. If a record ends up reading more or fewer
    /// bytes than it is ‘supposed’ to, it will raise this error, but _after_
    /// having read a different number of bytes than the specified length.
    #[error("Malformed packet: length {stated_length} was specified, but read {length_after_labels} bytes")]
    WrongLabelLength {

        /// The length of the record’s data, as specified in the packet.
        stated_length: u16,

        /// The computed length of the record’s data, based on the number of
        /// bytes consumed by reading labels from the packet.
        length_after_labels: u16,
    },

    /// When the data contained a string containing a cycle of pointers.
    /// Contains the vector of indexes that was being checked.
    #[error("Malformed packet: too much recursion: {0:?}")]
    TooMuchRecursion(Vec<u16>),

    /// When the data contained a string with a pointer to an index outside of
    /// the packet. Contains the invalid index.
    #[error("Malformed packet: out of bounds ({0})")]
    OutOfBounds(u16),

    /// When a record in the packet contained a version field that specifies
    /// the format of its remaining fields, but this version is too recent to
    /// be supported, so we cannot parse it.
    #[error("Malformed packet: record specifies version {stated_version}, expected up to {maximum_supported_version}")]
    WrongVersion {

        /// The version of the record layout, as specified in the packet
        stated_version: u8,

        /// The maximum version that this version of dog supports.
        maximum_supported_version: u8,
    }
}

impl WireError {
    /// Translate a mandated_length error into the right WireError variant
    #[inline(always)]
    pub fn wrong_record_length(stated_length: u16, mandated_length: MandatedLength) -> WireError {
        match mandated_length {
            MandatedLength::Exactly(mandated_length) => WireError::WrongRecordLengthExactly {
                stated_length,
                mandated_length,
            },
            MandatedLength::AtLeast(mandated_length) => WireError::WrongRecordLengthAtLeast {
                stated_length,
                mandated_length,
            },
        }
    }
}

/// The rule for how long a record in a packet should be.
#[derive(PartialEq, Debug, Copy, Clone)]
pub enum MandatedLength {

    /// The record should be exactly this many bytes in length.
    Exactly(u16),

    /// The record should be _at least_ this many bytes in length.
    AtLeast(u16),
}

impl From<io::Error> for WireError {
    fn from(ioe: io::Error) -> Self {
        error!("IO error -> {:?}", ioe);
        Self::IO
    }
}
