use std::convert::TryFrom;
use std::iter::Peekable;
use std::mem::transmute;
use std::path::{PathBuf, Path};

const MAGIC_NUMBER: &[u8] = &[
    0x62, 0x74, 0x72, 0x66, 0x73, 0x2d, 0x73, 0x74, 0x72, 0x65, 0x61, 0x6d, 0x00,
];
const BTRFS_UUID_SIZE: usize = 16;
type BtrfsUuid = [u8; BTRFS_UUID_SIZE];

pub(crate) enum BtrfsSendCommand {
    SUBVOL(PathBuf, BtrfsUuid, u64),
    SNAPSHOT(PathBuf, BtrfsUuid, u64, BtrfsUuid, u64),
    MKFILE(PathBuf),
    MKDIR(PathBuf),
    MKNOD(PathBuf, u64, u64),
    MKFIFO(PathBuf),
    MKSOCK(PathBuf),
    SYMLINK(PathBuf, PathBuf),
    RENAME(PathBuf, PathBuf),
    LINK(PathBuf, PathBuf),
    UNLINK(PathBuf),
    RMDIR(PathBuf),
    SET_XATTR(PathBuf, String, Vec<u8>),
    REMOVE_XATTR(PathBuf, String),
    WRITE(PathBuf, u64, Vec<u8>),
    CLONE(PathBuf, u64, u64, BtrfsUuid, u64, PathBuf, u64),
    TRUNCATE(PathBuf, u64),
    CHMOD(PathBuf, u64),
    CHOWN(PathBuf, u64, u64),
    UTIMES(PathBuf, Timespec, Timespec, Timespec),
    END,
    UPDATE_EXTENT(PathBuf, u64, u64),
}

#[derive(Clone)]
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
    CLONE_UUID(BtrfsUuid),
    CLONE_CTRANSID(u64),
    CLONE_PATH(PathBuf),
    CLONE_OFFSET(u64),
    CLONE_LENGTH(u64),
}

struct BtrfsSendHeader {
    version: u32,
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
    #[fail(display = "Wrong number of tlvs {} {}", 0, 1)]
    WrongNumberOfTlvs(usize, usize),
    #[fail(display = "Unexpected tlv")]
    UnexpectedTlv,
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

fn parse_uuid<I: Iterator<Item = u8>>(source: &mut I) -> Result<BtrfsUuid, BtrfsSendError> {
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

fn parse_string<I: Iterator<Item = u8>>(
    source: &mut I,
    length: u32,
) -> Result<String, BtrfsSendError> {
    let data = parse_data(source, length)?;
    Ok(unsafe { String::from_utf8_unchecked(data) })
}

fn parse_path<I: Iterator<Item = u8>>(
    source: &mut I,
    length: u32,
) -> Result<PathBuf, BtrfsSendError> {
    Ok(PathBuf::from(parse_string(source, length)?))
}

fn parse_tlv<I: Iterator<Item = u8>>(
    source: &mut Peekable<I>,
) -> Result<Option<BtrfsSendTlv>, BtrfsSendError> {
    if source.peek().is_none() {
        return Ok(None);
    }
    let tlv_type = parse_u16(source)?;
    let length = u32::from(parse_u16(source)?);
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
        13 => Ok(Some(BtrfsSendTlv::XATTR_NAME(parse_string(source, length)?))),
        14 => Ok(Some(BtrfsSendTlv::XATTR_DATA(parse_data(source, length)?))),
        15 => Ok(Some(BtrfsSendTlv::PATH(parse_path(source, length)?))),
        16 => Ok(Some(BtrfsSendTlv::PATH_TO(parse_path(source, length)?))),
        17 => Ok(Some(BtrfsSendTlv::PATH_LINK(parse_path(source, length)?))),
        18 => Ok(Some(BtrfsSendTlv::OFFSET(parse_u64(source)?))),
        19 => Ok(Some(BtrfsSendTlv::DATA(parse_data(source, length)?))),
        20 => Ok(Some(BtrfsSendTlv::CLONE_UUID(parse_uuid(source)?))),
        21 => Ok(Some(BtrfsSendTlv::CLONE_CTRANSID(parse_u64(source)?))),
        22 => Ok(Some(BtrfsSendTlv::CLONE_PATH(parse_path(source, length)?))),
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

fn parse_btrfs_type(type_number: u16, data: Vec<u8>) -> Result<BtrfsSendCommand, BtrfsSendError> {
    let tlvs = parse_tlvs(&mut data.into_iter().peekable())?;
    match type_number {
        1 => parse_subvol_command(&tlvs),
        2 => parse_snapshot_command(&tlvs),
        3 => parse_mkfile_command(&tlvs),
        4 => parse_mkdir_command(&tlvs),
        5 => parse_mknod_command(&tlvs),
        6 => parse_mkfifo_command(&tlvs),
        7 => parse_mksock_command(&tlvs),
        8 => parse_symlink_command(&tlvs),
        9 => parse_rename_command(&tlvs),
        10 => parse_link_command(&tlvs),
        11 => parse_unlink_command(&tlvs),
        12 => parse_rmdir_command(&tlvs),
        13 => parse_set_xattr_command(&tlvs),
        14 => parse_rm_xattr_command(&tlvs),
        15 => parse_write_command(&tlvs),
        16 => parse_clone_command(&tlvs),
        17 => parse_truncate_command(&tlvs),
        18 => parse_chmod_command(&tlvs),
        19 => parse_chown_command(&tlvs),
        20 => parse_utimes_command(&tlvs),
        21 => Ok(BtrfsSendCommand::END),
        22 => parse_extent_command(&tlvs),
        _ => Err(BtrfsSendError::InvalidCommandType(type_number)),
    }
}

fn parse_extent_command(tlvs: &Vec<BtrfsSendTlv>) -> Result<BtrfsSendCommand, BtrfsSendError> {
    if tlvs.len() != 3 {
        Err(BtrfsSendError::WrongNumberOfTlvs(tlvs.len(), 3))
    } else {
        let path = if let BtrfsSendTlv::PATH(p) = &tlvs[0] {
            Ok(p.clone())
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        let offset = if let BtrfsSendTlv::OFFSET(offset) = &tlvs[1] {
            Ok(*offset)
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        let size = if let BtrfsSendTlv::SIZE(size) = &tlvs[1] {
            Ok(*size)
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        Ok(BtrfsSendCommand::UPDATE_EXTENT(path, offset, size))
    }
}

fn parse_utimes_command(tlvs: &Vec<BtrfsSendTlv>) -> Result<BtrfsSendCommand, BtrfsSendError> {
    if tlvs.len() != 4 {
        Err(BtrfsSendError::WrongNumberOfTlvs(tlvs.len(), 4))
    } else {
        let path = if let BtrfsSendTlv::PATH(p) = &tlvs[0] {
            Ok(p.clone())
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        let atime = if let BtrfsSendTlv::ATIME(a) = &tlvs[1] {
            Ok(a.clone())
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        let mtime = if let BtrfsSendTlv::MTIME(m) = &tlvs[2] {
            Ok(m.clone())
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        let ctime = if let BtrfsSendTlv::CTIME(c) = &tlvs[3] {
            Ok(c.clone())
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        Ok(BtrfsSendCommand::UTIMES(path, atime, mtime, ctime))
    }
}

fn parse_chown_command(tlvs: &Vec<BtrfsSendTlv>) -> Result<BtrfsSendCommand, BtrfsSendError> {
    if tlvs.len() != 3 {
        Err(BtrfsSendError::WrongNumberOfTlvs(tlvs.len(), 3))
    } else {
        let path = if let BtrfsSendTlv::PATH(p) = &tlvs[0] {
            Ok(p.clone())
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        let uid = if let BtrfsSendTlv::UID(u) = &tlvs[1] {
            Ok(*u)
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        let gid = if let BtrfsSendTlv::GID(g) = &tlvs[2] {
            Ok(*g)
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        Ok(BtrfsSendCommand::CHOWN(path, uid, gid))
    }
}

fn parse_chmod_command(tlvs: &Vec<BtrfsSendTlv>) -> Result<BtrfsSendCommand, BtrfsSendError> {
    if tlvs.len() != 2 {
        Err(BtrfsSendError::WrongNumberOfTlvs(tlvs.len(), 2))
    } else {
        let path = if let BtrfsSendTlv::PATH(p) = &tlvs[0] {
            Ok(p.clone())
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        let mode = if let BtrfsSendTlv::MODE(m) = &tlvs[1] {
            Ok(*m)
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        Ok(BtrfsSendCommand::CHMOD(path, mode))
    }
}

fn parse_truncate_command(tlvs: &Vec<BtrfsSendTlv>) -> Result<BtrfsSendCommand, BtrfsSendError> {
    if tlvs.len() != 2 {
        Err(BtrfsSendError::WrongNumberOfTlvs(tlvs.len(), 2))
    } else {
        let path = if let BtrfsSendTlv::PATH(p) = &tlvs[0] {
            Ok(p.clone())
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        let size = if let BtrfsSendTlv::SIZE(s) = &tlvs[1] {
            Ok(*s)
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        Ok(BtrfsSendCommand::TRUNCATE(path, size))
    }
}

fn parse_clone_command(tlvs: &Vec<BtrfsSendTlv>) -> Result<BtrfsSendCommand, BtrfsSendError> {
    if tlvs.len() != 7 {
        Err(BtrfsSendError::WrongNumberOfTlvs(tlvs.len(), 7))
    } else {
        let path = if let BtrfsSendTlv::PATH(p) = &tlvs[0] {
            Ok(p.clone())
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        let offset = if let BtrfsSendTlv::OFFSET(offset) = &tlvs[1] {
            Ok(*offset)
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        let clone_len = if let BtrfsSendTlv::CLONE_LENGTH(len) = &tlvs[2] {
            Ok(*len)
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        let clone_uuid = if let BtrfsSendTlv::UUID(uuid) = &tlvs[3] {
            Ok(*uuid)
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        let clone_ctransid = if let BtrfsSendTlv::CLONE_CTRANSID(trandid) = &tlvs[4] {
            Ok(*trandid)
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        let clone_path = if let BtrfsSendTlv::CLONE_PATH(p) = &tlvs[5] {
            Ok(p.clone())
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        let clone_offset = if let BtrfsSendTlv::CLONE_OFFSET(o) = &tlvs[6] {
            Ok(*o)
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        Ok(BtrfsSendCommand::CLONE(path, offset, clone_len, clone_uuid, clone_ctransid, clone_path, clone_offset))
    }
}

fn parse_write_command(tlvs: &Vec<BtrfsSendTlv>) -> Result<BtrfsSendCommand, BtrfsSendError> {
    if tlvs.len() != 3 {
        Err(BtrfsSendError::WrongNumberOfTlvs(tlvs.len(), 3))
    } else {
        let path = if let BtrfsSendTlv::PATH(p) = &tlvs[0] {
            Ok(p.clone())
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        let offset = if let BtrfsSendTlv::OFFSET(offset) = &tlvs[1] {
            Ok(*offset)
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        let data = if let BtrfsSendTlv::DATA(data) = &tlvs[2] {
            Ok(data.clone())
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        Ok(BtrfsSendCommand::WRITE(path, offset, data))
    }
}

fn parse_rm_xattr_command(tlvs: &Vec<BtrfsSendTlv>) -> Result<BtrfsSendCommand, BtrfsSendError> {
    if tlvs.len() != 2 {
        Err(BtrfsSendError::WrongNumberOfTlvs(tlvs.len(), 2))
    } else {
        let path = if let BtrfsSendTlv::PATH(p) = &tlvs[0] {
            Ok(p.clone())
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        let name = if let BtrfsSendTlv::XATTR_NAME(name) = &tlvs[1] {
            Ok(name.clone())
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        Ok(BtrfsSendCommand::REMOVE_XATTR(path, name))
    }
}

fn parse_set_xattr_command(tlvs: &Vec<BtrfsSendTlv>) -> Result<BtrfsSendCommand, BtrfsSendError> {
    if tlvs.len() != 3 {
        Err(BtrfsSendError::WrongNumberOfTlvs(tlvs.len(), 3))
    } else {
        let path = if let BtrfsSendTlv::PATH(p) = &tlvs[0] {
            Ok(p.clone())
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        let name = if let BtrfsSendTlv::XATTR_NAME(name) = &tlvs[1] {
            Ok(name.clone())
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        let data = if let BtrfsSendTlv::XATTR_DATA(data) = &tlvs[2] {
            Ok(data.clone())
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        Ok(BtrfsSendCommand::SET_XATTR(path, name, data))
    }
}

fn parse_rmdir_command(tlvs: &Vec<BtrfsSendTlv>) -> Result<BtrfsSendCommand, BtrfsSendError> {
    if tlvs.len() != 1 {
        Err(BtrfsSendError::WrongNumberOfTlvs(tlvs.len(), 1))
    } else {
        let path = if let BtrfsSendTlv::PATH(p) = &tlvs[0] {
            Ok(p.clone())
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        Ok(BtrfsSendCommand::RMDIR(path))
    }
}

fn parse_unlink_command(tlvs: &Vec<BtrfsSendTlv>) -> Result<BtrfsSendCommand, BtrfsSendError> {
    if tlvs.len() != 1 {
        Err(BtrfsSendError::WrongNumberOfTlvs(tlvs.len(), 1))
    } else {
        let path = if let BtrfsSendTlv::PATH(p) = &tlvs[0] {
            Ok(p.clone())
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        Ok(BtrfsSendCommand::UNLINK(path))
    }
}

fn parse_link_command(tlvs: &Vec<BtrfsSendTlv>) -> Result<BtrfsSendCommand, BtrfsSendError> {
    if tlvs.len() != 2 {
        Err(BtrfsSendError::WrongNumberOfTlvs(tlvs.len(), 2))
    } else {
        let path = if let BtrfsSendTlv::PATH(p) = &tlvs[0] {
            Ok(p.clone())
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        let path_link = if let BtrfsSendTlv::PATH_LINK(p) = &tlvs[0] {
            Ok(p.clone())
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        Ok(BtrfsSendCommand::LINK(path, path_link))
    }
}

fn parse_rename_command(tlvs: &Vec<BtrfsSendTlv>) -> Result<BtrfsSendCommand, BtrfsSendError> {
    if tlvs.len() != 2 {
        Err(BtrfsSendError::WrongNumberOfTlvs(tlvs.len(), 2))
    } else {
        let path = if let BtrfsSendTlv::PATH(p) = &tlvs[0] {
            Ok(p.clone())
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        let path_to = if let BtrfsSendTlv::PATH_TO(p) = &tlvs[0] {
            Ok(p.clone())
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        Ok(BtrfsSendCommand::RENAME(path, path_to))
    }
}

fn parse_symlink_command(tlvs: &Vec<BtrfsSendTlv>) -> Result<BtrfsSendCommand, BtrfsSendError> {
    if tlvs.len() != 2 {
        Err(BtrfsSendError::WrongNumberOfTlvs(tlvs.len(), 2))
    } else {
        let path = if let BtrfsSendTlv::PATH(p) = &tlvs[0] {
            Ok(p.clone())
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        let path_link = if let BtrfsSendTlv::PATH_LINK(p) = &tlvs[0] {
            Ok(p.clone())
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        Ok(BtrfsSendCommand::SYMLINK(path, path_link))
    }
}

fn parse_mksock_command(tlvs: &Vec<BtrfsSendTlv>) -> Result<BtrfsSendCommand, BtrfsSendError> {
    if tlvs.len() != 1 {
        Err(BtrfsSendError::WrongNumberOfTlvs(tlvs.len(), 1))
    } else {
        let path = if let BtrfsSendTlv::PATH(p) = &tlvs[0] {
            Ok(p.clone())
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        Ok(BtrfsSendCommand::MKSOCK(path))
    }
}

fn parse_mkfifo_command(tlvs: &Vec<BtrfsSendTlv>) -> Result<BtrfsSendCommand, BtrfsSendError> {
    if tlvs.len() != 1 {
        Err(BtrfsSendError::WrongNumberOfTlvs(tlvs.len(), 1))
    } else {
        let path = if let BtrfsSendTlv::PATH(p) = &tlvs[0] {
            Ok(p.clone())
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        Ok(BtrfsSendCommand::MKFIFO(path))
    }
}

fn parse_mknod_command(tlvs: &Vec<BtrfsSendTlv>) -> Result<BtrfsSendCommand, BtrfsSendError> {
    if tlvs.len() != 3 {
        Err(BtrfsSendError::WrongNumberOfTlvs(tlvs.len(), 3))
    } else {
        let path = if let BtrfsSendTlv::PATH(p) = &tlvs[0] {
            Ok(p.clone())
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        let mode = if let BtrfsSendTlv::MODE(mode) = &tlvs[1] {
            Ok(*mode)
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        let rdev = if let BtrfsSendTlv::RDEV(rdev) = &tlvs[2] {
            Ok(*rdev)
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        Ok(BtrfsSendCommand::MKNOD(path, mode, rdev))
    }
}

fn parse_mkdir_command(tlvs: &Vec<BtrfsSendTlv>) -> Result<BtrfsSendCommand, BtrfsSendError> {
    if tlvs.len() != 1 {
        Err(BtrfsSendError::WrongNumberOfTlvs(tlvs.len(), 1))
    } else {
        let path = if let BtrfsSendTlv::PATH(p) = &tlvs[0] {
            Ok(p.clone())
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        Ok(BtrfsSendCommand::MKDIR(path))
    }
}

fn parse_mkfile_command(tlvs: &Vec<BtrfsSendTlv>) -> Result<BtrfsSendCommand, BtrfsSendError> {
    if tlvs.len() != 1 {
        Err(BtrfsSendError::WrongNumberOfTlvs(tlvs.len(), 1))
    } else {
        let path = if let BtrfsSendTlv::PATH(p) = &tlvs[0] {
            Ok(p.clone())
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        Ok(BtrfsSendCommand::MKFILE(path))
    }
}

fn parse_snapshot_command(tlvs: &Vec<BtrfsSendTlv>) -> Result<BtrfsSendCommand, BtrfsSendError> {
    if tlvs.len() != 5 {
        Err(BtrfsSendError::WrongNumberOfTlvs(tlvs.len(), 5))
    } else {
        let path = if let BtrfsSendTlv::PATH(p) = &tlvs[0] {
            Ok(p.clone())
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        let uuid = if let BtrfsSendTlv::UUID(uuid) = &tlvs[1] {
            Ok(*uuid)
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        let ctransid = if let BtrfsSendTlv::CLONE_CTRANSID(trandid) = &tlvs[2] {
            Ok(*trandid)
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        let clone_uuid = if let BtrfsSendTlv::UUID(uuid) = &tlvs[3] {
            Ok(*uuid)
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        let clone_ctransid = if let BtrfsSendTlv::CLONE_CTRANSID(trandid) = &tlvs[4] {
            Ok(*trandid)
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        Ok(BtrfsSendCommand::SNAPSHOT(path, uuid, ctransid, clone_uuid, clone_ctransid))
    }
}

fn parse_subvol_command(tlvs: &Vec<BtrfsSendTlv>) -> Result<BtrfsSendCommand, BtrfsSendError> {
    if tlvs.len() != 3 {
        Err(BtrfsSendError::WrongNumberOfTlvs(tlvs.len(), 3))
    } else {
        let path = if let BtrfsSendTlv::PATH(p) = &tlvs[0] {
            Ok(p.clone())
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        let uuid = if let BtrfsSendTlv::UUID(uuid) = &tlvs[1] {
            Ok(*uuid)
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        let ctransid = if let BtrfsSendTlv::CLONE_CTRANSID(trandid) = &tlvs[2] {
            Ok(*trandid)
        } else {
            Err(BtrfsSendError::UnexpectedTlv)
        }?;
        Ok(BtrfsSendCommand::SUBVOL(path, uuid, ctransid))
    }
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
        parse_btrfs_type(type_number, data).map(|v| Some(v))
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
