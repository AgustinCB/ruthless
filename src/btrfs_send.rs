use std::convert::TryFrom;
use std::mem::transmute;

const MAGIC_NUMBER: &'static [u8] =
    &[0x62, 0x74, 0x72, 0x66, 0x73, 0x2d, 0x73, 0x74, 0x72, 0x65, 0x61, 0x6d, 0x00];

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

struct BtrfsSendHeader {
    version: u32,
}

struct BtrfsSendCommand {
    length: u32,
    command: u16,
    data: Vec<BtrfsSendTlv>,
}

struct BtrfsSendTlv {
    tlv_type: BtrfsTlvType,
    length: u16,
    data: Vec<u8>,
}

struct BtrfsSend {
    header: BtrfsSendHeader,
    commands: Vec<BtrfsSendCommand>,
}

#[derive(Debug)]
enum BtrfsSendError {
    InvalidMagicNumber,
}

impl TryFrom<Vec<u8>> for BtrfsSend {
    type Error = BtrfsSendError;
    fn try_from(source: Vec<u8>) -> Result<BtrfsSend, Self::Error> {
        let magic_number = &source[0..MAGIC_NUMBER.len()];
        if magic_number != MAGIC_NUMBER {
            return Err(BtrfsSendError::InvalidMagicNumber)
        };
        let mut version_numbers_to_convert = [0u8; 4];
        version_numbers_to_convert.copy_from_slice(&source[MAGIC_NUMBER.len()..MAGIC_NUMBER.len()+4]);
        let mut version: u32 = unsafe {
            transmute::<[u8; 4], u32>(version_numbers_to_convert)
        }.to_be();
        Err(BtrfsSendError::InvalidMagicNumber)
    }
}
