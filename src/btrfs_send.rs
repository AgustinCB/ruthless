use std::convert::TryFrom;

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
        Err(BtrfsSendError::InvalidMagicNumber)
    }
}
