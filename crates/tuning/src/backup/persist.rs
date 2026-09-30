use crate::{Result, TuningError};

pub(crate) trait DurableFile {
    fn write_all(&mut self, bytes: &[u8]) -> Result<()>;
    fn flush(&mut self) -> Result<()>;
    fn protect(&mut self) -> Result<()>;
    fn read_back(&mut self, maximum: u64) -> Result<Vec<u8>>;
}

pub(crate) fn persist_exact(file: &mut impl DurableFile, bytes: &[u8]) -> Result<()> {
    file.write_all(bytes)?;
    file.flush()?;
    file.protect()?;
    let actual = file.read_back(bytes.len() as u64)?;
    if actual != bytes {
        return Err(TuningError::BackupInvalid(
            "persisted backup bytes differ from source bytes".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeFile {
        bytes: Vec<u8>,
        fail: Option<&'static str>,
        protected: bool,
    }

    impl DurableFile for FakeFile {
        fn write_all(&mut self, bytes: &[u8]) -> Result<()> {
            if self.fail == Some("write") {
                self.bytes.extend_from_slice(&bytes[..bytes.len() / 2]);
                return Err(TuningError::BackupInvalid("injected write failure".into()));
            }
            if self.fail == Some("partial") {
                self.bytes.extend_from_slice(&bytes[..bytes.len() / 2]);
            } else {
                self.bytes.extend_from_slice(bytes);
            }
            Ok(())
        }
        fn flush(&mut self) -> Result<()> {
            if self.fail == Some("flush") {
                Err(TuningError::BackupInvalid("injected flush failure".into()))
            } else {
                Ok(())
            }
        }
        fn protect(&mut self) -> Result<()> {
            if self.fail == Some("protect") {
                Err(TuningError::BackupInvalid("injected ACL failure".into()))
            } else {
                self.protected = true;
                Ok(())
            }
        }
        fn read_back(&mut self, _: u64) -> Result<Vec<u8>> {
            Ok(self.bytes.clone())
        }
    }

    #[test]
    fn production_sequence_rejects_write_and_partial_write() {
        for failure in ["write", "partial"] {
            let mut file = FakeFile {
                fail: Some(failure),
                ..FakeFile::default()
            };
            assert!(persist_exact(&mut file, b"complete bytes").is_err());
        }
    }

    #[test]
    fn production_sequence_rejects_flush_and_acl_failures() {
        for failure in ["flush", "protect"] {
            let mut file = FakeFile {
                fail: Some(failure),
                ..FakeFile::default()
            };
            assert!(persist_exact(&mut file, b"complete bytes").is_err());
        }
    }

    #[test]
    fn production_sequence_requires_protection_before_readback() {
        let mut file = FakeFile::default();
        persist_exact(&mut file, b"complete bytes").unwrap();
        assert!(file.protected);
    }
}
