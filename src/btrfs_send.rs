use std::convert::TryFrom;
use std::iter::Peekable;
use std::mem::transmute;
use std::path::PathBuf;

const MAGIC_NUMBER: &[u8] = &[
    0x62, 0x74, 0x72, 0x66, 0x73, 0x2d, 0x73, 0x74, 0x72, 0x65, 0x61, 0x6d, 0x00,
];
const BTRFS_UUID_SIZE: usize = 16;

pub(crate) enum BtrfsSendCommandType {
    SUBVOL,
    SNAPSHOT,
    MKFILE,
    MKDIR,
    MKNOD,
    MKFIFO,
    MKSOCK,
    SYMLINK,
    RENAME,
    LINK,
    UNLINK,
    RMDIR,
    SET_XATTR,
    REMOVE_XATTR,
    WRITE,
    CLONE,
    TRUNCATE,
    CHMOD,
    CHOWN,
    UTIMES,
    END,
    UPDATE_EXTENT,
}

impl TryFrom<u16> for BtrfsSendCommandType {
    type Error = BtrfsSendError;
    fn try_from(n: u16) -> Result<BtrfsSendCommandType, Self::Error> {
        match n {
            1 => Ok(BtrfsSendCommandType::SUBVOL),
            2 => Ok(BtrfsSendCommandType::SNAPSHOT),
            3 => Ok(BtrfsSendCommandType::MKFILE),
            4 => Ok(BtrfsSendCommandType::MKDIR),
            5 => Ok(BtrfsSendCommandType::MKNOD),
            6 => Ok(BtrfsSendCommandType::MKFIFO),
            7 => Ok(BtrfsSendCommandType::MKSOCK),
            8 => Ok(BtrfsSendCommandType::SYMLINK),
            9 => Ok(BtrfsSendCommandType::RENAME),
            10 => Ok(BtrfsSendCommandType::LINK),
            11 => Ok(BtrfsSendCommandType::UNLINK),
            12 => Ok(BtrfsSendCommandType::RMDIR),
            13 => Ok(BtrfsSendCommandType::SET_XATTR),
            14 => Ok(BtrfsSendCommandType::REMOVE_XATTR),
            15 => Ok(BtrfsSendCommandType::WRITE),
            16 => Ok(BtrfsSendCommandType::CLONE),
            17 => Ok(BtrfsSendCommandType::TRUNCATE),
            18 => Ok(BtrfsSendCommandType::CHMOD),
            19 => Ok(BtrfsSendCommandType::CHOWN),
            20 => Ok(BtrfsSendCommandType::UTIMES),
            21 => Ok(BtrfsSendCommandType::END),
            22 => Ok(BtrfsSendCommandType::UPDATE_EXTENT),
            _ => Err(BtrfsSendError::InvalidCommandType(n)),
        }
    }
}

pub(crate) struct Timespec {
    secs: u64,
    nsecs: u32,
}

pub(crate) enum BtrfsSendTlv {
    UUID([u8; BTRFS_UUID_SIZE]),
    TRANSID(u64),
    INODE,
    SIZE(u64),
    MODE(u64),
    UID(u64),
    GID(u64),
    RDEV(u64),
    CTIME(Timespec),
    MTIME(Timespec),
    ATIME(Timespec),
    OTIME(Timespec),
    XATTR_NAME(String),
    XATTR_DATA(Vec<u8>),
    PATH(PathBuf),
    PATH_TO(PathBuf),
    PATH_LINK(PathBuf),
    OFFSET(u64),
    DATA(Vec<u8>),
    CLONE_UUID([u8; BTRFS_UUID_SIZE]),
    CLONE_CTRANSID(u64),
    CLONE_PATH(PathBuf),
    CLONE_OFFSET(u64),
    CLONE_LENGTH(u64),
}

struct BtrfsSendHeader {
    version: u32,
}

pub(crate) struct BtrfsSendCommand {
    length: u32,
    pub(crate) command: BtrfsSendCommandType,
    pub(crate) data: Vec<BtrfsSendTlv>,
}

pub(crate) struct BtrfsSend {
    header: BtrfsSendHeader,
    pub(crate) commands: Vec<BtrfsSendCommand>,
}

#[derive(Debug, Fail)]
pub(crate) enum BtrfsSendError {
    #[fail(display = "Invalid checksum: Got {}, expected {}", 1, 0)]
    InvalidChecksume(u32, u32),
    #[fail(display = "Invalid command type {}", 0)]
    InvalidCommandType(u16),
    #[fail(display = "Invalid magic number")]
    InvalidMagicNumber,
    #[fail(display = "Invalid TLV type {}", 0)]
    InvalidTlvType(u16),
    #[fail(display = "Not enough bytes to parse u32")]
    NotEnoughBytesToParseU32,
    #[fail(display = "Not enough bytes to parse u16")]
    NotEnoughBytesToParseU16,
    #[fail(display = "Unexpected length {}", 0)]
    UnexpectedLength(u32),
}

fn parse_u64<I: Iterator<Item = u8>>(source: &mut I) -> Result<u64, BtrfsSendError> {
    let mut length_bytes = [0u8; 8];
    for i in &mut length_bytes {
        *i = source
            .next()
            .ok_or(BtrfsSendError::NotEnoughBytesToParseU32)?;
    }
    Ok(unsafe { transmute::<[u8; 8], u64>(length_bytes) }.to_be())
}

fn parse_u32<I: Iterator<Item = u8>>(source: &mut I) -> Result<u32, BtrfsSendError> {
    let mut length_bytes = [0u8; 4];
    for i in &mut length_bytes {
        *i = source
            .next()
            .ok_or(BtrfsSendError::NotEnoughBytesToParseU32)?;
    }
    Ok(unsafe { transmute::<[u8; 4], u32>(length_bytes) }.to_be())
}

fn parse_u16<I: Iterator<Item = u8>>(source: &mut I) -> Result<u16, BtrfsSendError> {
    let mut u16_bytes = [0u8; 2];
    for i in &mut u16_bytes {
        *i = source
            .next()
            .ok_or(BtrfsSendError::NotEnoughBytesToParseU16)?;
    }
    Ok(unsafe { transmute::<[u8; 2], u16>(u16_bytes) }.to_be())
}

fn parse_data<I: Iterator<Item = u8>>(
    source: &mut I,
    length: u32,
) -> Result<Vec<u8>, BtrfsSendError> {
    let mut data = Vec::new();
    for _ in 0..length {
        if let Some(b) = source.next() {
            data.push(b);
        } else {
            Err(BtrfsSendError::UnexpectedLength(length))?
        }
    }
    Ok(data)
}

fn parse_uuid<I: Iterator<Item = u8>>(
    source: &mut I,
) -> Result<[u8; BTRFS_UUID_SIZE], BtrfsSendError> {
    let mut data = [0; BTRFS_UUID_SIZE];
    for b in data.iter_mut().take(BTRFS_UUID_SIZE) {
        if let Some(nb) = source.next() {
            *b = nb;
        } else {
            Err(BtrfsSendError::UnexpectedLength(BTRFS_UUID_SIZE as u32))?
        };
    }
    Ok(data)
}

fn parse_timespec<I: Iterator<Item = u8>>(source: &mut I) -> Result<Timespec, BtrfsSendError> {
    Ok(Timespec {
        secs: parse_u64(source)?,
        nsecs: parse_u32(source)?,
    })
}

fn parse_string<I: Iterator<Item = u8>>(source: &mut I) -> Result<String, BtrfsSendError> {
    let length = u32::from(parse_u16(source)?);
    let data = parse_data(source, length)?;
    Ok(unsafe { String::from_utf8_unchecked(data) })
}

fn parse_path<I: Iterator<Item = u8>>(source: &mut I) -> Result<PathBuf, BtrfsSendError> {
    Ok(PathBuf::from(parse_string(source)?))
}

fn parse_tlv<I: Iterator<Item = u8>>(
    source: &mut Peekable<I>,
) -> Result<Option<BtrfsSendTlv>, BtrfsSendError> {
    if source.peek().is_none() {
        return Ok(None);
    }
    let tlv_type = parse_u16(source)?;
    match tlv_type {
        1 => Ok(Some(BtrfsSendTlv::UUID(parse_uuid(source)?))),
        2 => Ok(Some(BtrfsSendTlv::TRANSID(parse_u64(source)?))),
        3 => Ok(Some(BtrfsSendTlv::INODE)),
        4 => Ok(Some(BtrfsSendTlv::SIZE(parse_u64(source)?))),
        5 => Ok(Some(BtrfsSendTlv::MODE(parse_u64(source)?))),
        6 => Ok(Some(BtrfsSendTlv::UID(parse_u64(source)?))),
        7 => Ok(Some(BtrfsSendTlv::GID(parse_u64(source)?))),
        8 => Ok(Some(BtrfsSendTlv::RDEV(parse_u64(source)?))),
        9 => Ok(Some(BtrfsSendTlv::CTIME(parse_timespec(source)?))),
        10 => Ok(Some(BtrfsSendTlv::MTIME(parse_timespec(source)?))),
        11 => Ok(Some(BtrfsSendTlv::ATIME(parse_timespec(source)?))),
        12 => Ok(Some(BtrfsSendTlv::OTIME(parse_timespec(source)?))),
        13 => Ok(Some(BtrfsSendTlv::XATTR_NAME(parse_string(source)?))),
        14 => Ok(Some(BtrfsSendTlv::XATTR_DATA({
            let length = u32::from(parse_u16(source)?);
            parse_data(source, length)?
        }))),
        15 => Ok(Some(BtrfsSendTlv::PATH(parse_path(source)?))),
        16 => Ok(Some(BtrfsSendTlv::PATH_TO(parse_path(source)?))),
        17 => Ok(Some(BtrfsSendTlv::PATH_LINK(parse_path(source)?))),
        18 => Ok(Some(BtrfsSendTlv::OFFSET(parse_u64(source)?))),
        19 => Ok(Some(BtrfsSendTlv::DATA({
            let length = u32::from(parse_u16(source)?);
            parse_data(source, length)?
        }))),
        20 => Ok(Some(BtrfsSendTlv::CLONE_UUID(parse_uuid(source)?))),
        21 => Ok(Some(BtrfsSendTlv::CLONE_CTRANSID(parse_u64(source)?))),
        22 => Ok(Some(BtrfsSendTlv::CLONE_PATH(parse_path(source)?))),
        23 => Ok(Some(BtrfsSendTlv::CLONE_OFFSET(parse_u64(source)?))),
        24 => Ok(Some(BtrfsSendTlv::CLONE_LENGTH(parse_u64(source)?))),
        _ => Err(BtrfsSendError::InvalidTlvType(tlv_type)),
    }
}

fn parse_tlvs<I: Iterator<Item = u8>>(
    source: &mut Peekable<I>,
) -> Result<Vec<BtrfsSendTlv>, BtrfsSendError> {
    let mut tlvs = Vec::new();
    while let Some(tlv) = parse_tlv(source)? {
        tlvs.push(tlv);
    }
    Ok(tlvs)
}

fn parse_btrfs_command<I: Iterator<Item = u8>>(
    source: &mut Peekable<I>,
) -> Result<Option<BtrfsSendCommand>, BtrfsSendError> {
    if source.peek().is_none() {
        return Ok(None);
    }
    let length = parse_u32(source)?;
    let type_number = parse_u16(source)?;
    let checksum = parse_u32(source)?;
    let data = parse_data(source, length)?;
    let data_checksum = data.iter().cloned().map(u32::from).sum();
    if data_checksum == checksum {
        Ok(Some(BtrfsSendCommand {
            length,
            command: BtrfsSendCommandType::try_from(type_number)?,
            data: parse_tlvs(source)?,
        }))
    } else {
        Err(BtrfsSendError::InvalidChecksume(checksum, data_checksum))
    }
}

fn parse_btrfs_header(source: &[u8]) -> Result<BtrfsSendHeader, BtrfsSendError> {
    let magic_number = &source[0..MAGIC_NUMBER.len()];
    if magic_number != MAGIC_NUMBER {
        return Err(BtrfsSendError::InvalidMagicNumber);
    };
    let mut version_numbers_to_convert = [0u8; 4];
    version_numbers_to_convert.copy_from_slice(&source[MAGIC_NUMBER.len()..MAGIC_NUMBER.len() + 4]);
    let version: u32 = unsafe { transmute::<[u8; 4], u32>(version_numbers_to_convert) }.to_be();
    Ok(BtrfsSendHeader { version })
}

impl TryFrom<Vec<u8>> for BtrfsSend {
    type Error = BtrfsSendError;
    fn try_from(source: Vec<u8>) -> Result<BtrfsSend, Self::Error> {
        let header = parse_btrfs_header(&source[0..MAGIC_NUMBER.len() + 4])?;
        let mut bytes = source.into_iter().skip(MAGIC_NUMBER.len() + 4).peekable();
        let mut commands = Vec::new();
        while let Some(command) = parse_btrfs_command(&mut bytes)? {
            commands.push(command);
        }
        Ok(BtrfsSend { commands, header })
    }
}
