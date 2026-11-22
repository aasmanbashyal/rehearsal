use std::{fs, io, path::PathBuf};
use tempfile::{TempDir, TempPath};

pub(crate) struct PendingExport {
    directory: TempDir,
    pub(crate) destination: PathBuf,
    replace: bool,
}

impl PendingExport {
    pub(crate) fn new(destination: PathBuf) -> io::Result<Self> {
        let parent = destination.parent().ok_or(io::ErrorKind::InvalidInput)?;
        let replace = match fs::symlink_metadata(&destination) {
            Ok(_) => true,
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(error) => return Err(error),
        };
        let directory = tempfile::Builder::new()
            .prefix(".rehearsal-export-")
            .tempdir_in(parent)?;
        Ok(Self {
            directory,
            destination,
            replace,
        })
    }

    // WKDownload requires an unused path. Keeping it on the destination volume
    // lets us publish the finished file with a single rename.
    pub(crate) fn staging_path(&self) -> PathBuf {
        self.directory.path().join("download")
    }

    pub(crate) fn finish(&self) -> io::Result<()> {
        let file = TempPath::try_from_path(self.staging_path())?;
        if self.replace {
            file.persist(&self.destination)
        } else {
            file.persist_noclobber(&self.destination)
        }
        .map_err(|error| error.error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publishes_completed_exports_and_preserves_existing_files_on_failure() -> io::Result<()> {
        let folder = tempfile::tempdir()?;
        let destination = folder.path().join("recording.mp4");
        let first = PendingExport::new(destination.clone())?;
        assert!(!first.staging_path().exists());
        fs::write(first.staging_path(), b"original")?;
        first.finish()?;

        let partial = PendingExport::new(destination.clone())?;
        let staging = partial.staging_path();
        fs::write(&staging, b"partial")?;
        drop(partial);
        assert!(!staging.exists());
        assert_eq!(fs::read(&destination)?, b"original");

        let replacement = PendingExport::new(destination.clone())?;
        fs::write(replacement.staging_path(), b"complete")?;
        replacement.finish()?;
        assert_eq!(fs::read(destination)?, b"complete");
        Ok(())
    }

    #[test]
    fn a_new_export_does_not_replace_a_file_created_after_the_panel_closed() -> io::Result<()> {
        let folder = tempfile::tempdir()?;
        let destination = folder.path().join("backup.json");
        let export = PendingExport::new(destination.clone())?;
        fs::write(&destination, b"other export")?;
        fs::write(export.staging_path(), b"this export")?;
        assert!(export.finish().is_err());
        assert_eq!(fs::read(destination)?, b"other export");
        Ok(())
    }
}
