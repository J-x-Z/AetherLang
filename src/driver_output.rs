//! Stage outputs on the destination filesystem and publish only complete files.
use std::{fs, io::Write, path::{Path, PathBuf}, sync::atomic::{AtomicU64, Ordering}};

static NEXT_OUTPUT: AtomicU64 = AtomicU64::new(0);

pub struct OutputFile {
    directory: PathBuf,
    pub path: PathBuf,
}

impl OutputFile {
    pub fn new(destination: &Path) -> Result<Self, String> {
        let absolute = if destination.is_absolute() { destination.to_path_buf() } else {
            std::env::current_dir().map_err(|e| format!("locating output directory: {e}"))?.join(destination)
        };
        let parent = absolute.parent().ok_or("output has no parent directory")?;
        let filename = absolute.file_name().ok_or("output must name a file")?;
        for _ in 0..100 {
            let directory = parent.join(format!(".aethc-output-{}-{}", std::process::id(), NEXT_OUTPUT.fetch_add(1, Ordering::Relaxed)));
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)] { use std::os::unix::fs::DirBuilderExt; builder.mode(0o700); }
            match builder.create(&directory) {
                Ok(()) => return Ok(Self { path: directory.join(filename), directory }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(format!("staging output {}: {e}", destination.display())),
            }
        }
        Err(format!("cannot allocate a unique output temporary for {}", destination.display()))
    }

    pub fn publish(self, destination: &Path) -> Result<(), String> {
        let metadata = fs::symlink_metadata(&self.path)
            .map_err(|e| format!("reading completed output {}: {e}", destination.display()))?;
        if !metadata.is_file() {
            return Err(format!("compiler did not produce a regular output file: {}", destination.display()));
        }
        // Catch deferred write errors before replacing any previous artifact.
        fs::File::open(&self.path).and_then(|file| file.sync_all())
            .map_err(|e| format!("syncing output {}: {e}", destination.display()))?;
        fs::rename(&self.path, destination)
            .map_err(|e| format!("publishing output {}: {e}", destination.display()))
    }
}

impl Drop for OutputFile {
    fn drop(&mut self) { let _ = fs::remove_dir_all(&self.directory); }
}

pub fn write(destination: &Path, bytes: &[u8]) -> Result<(), String> {
    let staged = OutputFile::new(destination)?;
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&staged.path)
        .map_err(|e| format!("creating output {}: {e}", destination.display()))?;
    file.write_all(bytes).map_err(|e| format!("writing {}: {e}", destination.display()))?;
    drop(file);
    staged.publish(destination)
}
