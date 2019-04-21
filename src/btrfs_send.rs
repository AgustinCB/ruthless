use std::convert::TryFrom;
use std::iter::Peekable;
use std::mem::transmute;

const MAGIC_NUMBER: &'static [u8] = &[
    0x62, 0x74, 0x72, 0x66, 0x73, 0x2d, 0x73, 0x74, 0x72, 0x65, 0x61, 0x6d, 0x00,
];

enum BtrfsSendCommandType {
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

enum BtrfsTlvType {
    UUID,
    TRANSID,
    INODE,
    SIZE,
    MODE,
    UID,
    GID,
    RDEV,
    CTIME,
    MTIME,
    ATIME,
    OTIME,
    XATTR_NAME,
    XATTR_DATA,
    PATH,
    PATH_TO,
    PATH_LINK,
    OFFSET,
    DATA,
    CLONE_UUID,
    CLONE_CTRANSID,
    CLONE_PATH,
    CLONE_OFFSET,
    CLONE_LENGTH,
}

impl TryFrom<u16> for BtrfsTlvType {
    type Error = BtrfsSendError;
    fn try_from(n: u16) -> Result<BtrfsTlvType, BtrfsSendError> {
        match n {
            1 => Ok(BtrfsTlvType::UUID),
            2 => Ok(BtrfsTlvType::TRANSID),
            3 => Ok(BtrfsTlvType::INODE),
            4 => Ok(BtrfsTlvType::SIZE),
            5 => Ok(BtrfsTlvType::MODE),
            6 => Ok(BtrfsTlvType::UID),
            7 => Ok(BtrfsTlvType::GID),
            8 => Ok(BtrfsTlvType::RDEV),
            9 => Ok(BtrfsTlvType::CTIME),
            10 => Ok(BtrfsTlvType::MTIME),
            11 => Ok(BtrfsTlvType::ATIME),
            12 => Ok(BtrfsTlvType::OTIME),
            13 => Ok(BtrfsTlvType::XATTR_NAME),
            14 => Ok(BtrfsTlvType::XATTR_DATA),
            15 => Ok(BtrfsTlvType::PATH),
            16 => Ok(BtrfsTlvType::PATH_TO),
            17 => Ok(BtrfsTlvType::PATH_LINK),
            18 => Ok(BtrfsTlvType::OFFSET),
            19 => Ok(BtrfsTlvType::DATA),
            20 => Ok(BtrfsTlvType::CLONE_UUID),
            21 => Ok(BtrfsTlvType::CLONE_CTRANSID),
            22 => Ok(BtrfsTlvType::CLONE_PATH),
            23 => Ok(BtrfsTlvType::CLONE_OFFSET),
            24 => Ok(BtrfsTlvType::CLONE_LENGTH),
            _ => Err(BtrfsSendError::InvalidTlvType(n)),
        }
    }
}

struct BtrfsSendHeader {
    version: u32,
}

struct BtrfsSendCommand {
    length: u32,
    command: BtrfsSendCommandType,
    data: Vec<BtrfsSendTlv>,
}

struct BtrfsSendTlv {
    tlv_type: BtrfsTlvType,
    length: u16,
    data: Vec<u8>,
}

pub(crate) struct BtrfsSend {
    header: BtrfsSendHeader,
    commands: Vec<BtrfsSendCommand>,
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

fn parse_btrfs_u32<I: Iterator<Item = u8>>(source: &mut I) -> Result<u32, BtrfsSendError> {
    let mut length_bytes = [0u8; 4];
    for i in 0..4 {
        length_bytes[i] = source
            .next()
            .ok_or(BtrfsSendError::NotEnoughBytesToParseU32)?;
    }
    Ok(unsafe { transmute::<[u8; 4], u32>(length_bytes) }.to_be())
}

fn parse_u16<I: Iterator<Item = u8>>(source: &mut I) -> Result<u16, BtrfsSendError> {
    let mut u16_bytes = [0u8; 2];
    for i in 0..2 {
        u16_bytes[i] = source
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

fn parse_tlv<I: Iterator<Item = u8>>(
    source: &mut Peekable<I>,
) -> Result<Option<BtrfsSendTlv>, BtrfsSendError> {
    if let None = source.peek() {
        return Ok(None);
    }
    let tlv_type = BtrfsTlvType::try_from(parse_u16(source)?)?;
    let length = parse_u16(source)?;
    let data = parse_data(source, length as u32)?;
    Ok(Some(BtrfsSendTlv {
        data,
        length,
        tlv_type,
    }))
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
    if let None = source.next() {
        return Ok(None);
    }
    let length = parse_btrfs_u32(source)?;
    let type_number = parse_u16(source)?;
    let checksum = parse_btrfs_u32(source)?;
    let mut data = parse_data(source, length)?;
    let data_checksum = data.iter().cloned().map(|b| b as u32).sum();
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
    let mut version: u32 = unsafe { transmute::<[u8; 4], u32>(version_numbers_to_convert) }.to_be();
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
